//! The QUIC transport end to end: listening with a proof bound to the TLS
//! session, dialing, sessions as streams, pinning, and mixing transports.

use super::*;
use crate::{
    RouteId,
    metrics::{Gauge, Kind},
    quic::{
        client_transport,
        tls::{Pin, SERVER_NAME, client_crypto},
    },
    wire::{self, EXPORTER_LABEL, EXPORTER_LEN, INCOMING, Key, Message, Request, Status},
};
use quinn::{ClientConfig, Connection, Endpoint, RecvStream, SendStream};

async fn connect(relay: &Relay) -> Connection {
    connect_pinned(relay.quic, relay.pin).await.unwrap()
}

async fn connect_pinned(
    address: SocketAddr,
    pin: Pin,
) -> Result<Connection, quinn::ConnectionError> {
    connect_with(address, pin, client_transport()).await
}

/// A client connection with its own transport settings.
pub(super) async fn connect_with(
    address: SocketAddr,
    pin: Pin,
    transport: quinn::TransportConfig,
) -> Result<Connection, quinn::ConnectionError> {
    let mut endpoint = Endpoint::client("127.0.0.1:0".parse().unwrap()).unwrap();
    let mut config = ClientConfig::new(Arc::new(client_crypto(pin).unwrap()));
    config.transport_config(Arc::new(transport));
    endpoint.set_default_client_config(config);
    endpoint.connect(address, SERVER_NAME).unwrap().await
}

pub(super) async fn status(recv: &mut RecvStream) -> Status {
    let mut byte = [0; 1];
    tokio::time::timeout(Duration::from_secs(5), recv.read_exact(&mut byte))
        .await
        .expect("no status in time")
        .unwrap();
    Status::from_byte(byte[0]).unwrap()
}

fn exported(connection: &Connection) -> [u8; EXPORTER_LEN] {
    let mut exported = [0; EXPORTER_LEN];
    connection
        .export_keying_material(&mut exported, EXPORTER_LABEL, b"")
        .unwrap();
    exported
}

/// The listen request `key` makes on `connection`.
pub(super) fn listen_request(connection: &Connection, key: &SigningKey, token: &[u8]) -> Request {
    Request::Listen {
        key: public(key),
        signature: sign(key, &wire::listen_signed(&exported(connection))),
        token: token.to_vec(),
    }
}

/// Listens for `key`, allowing [`phone`] to dial; the connection and its
/// open listen stream.
pub(super) async fn quic_listen(
    relay: &Relay,
    key: &SigningKey,
) -> (Connection, SendStream, RecvStream) {
    let (connection, mut send, mut recv) = quic_listen_only(relay, key).await;
    let status = quic_message(
        &mut send,
        &mut recv,
        Message::DialKeys(vec![public(&phone())]),
    )
    .await;
    assert_eq!(status, Status::Ok);
    (connection, send, recv)
}

/// Listens for `key`, allowing no one yet.
pub(super) async fn quic_listen_only(
    relay: &Relay,
    key: &SigningKey,
) -> (Connection, SendStream, RecvStream) {
    let connection = connect(relay).await;
    let (mut send, mut recv) = connection.open_bi().await.unwrap();
    let request = listen_request(&connection, key, b"");
    send.write_all(&request.encode().unwrap()).await.unwrap();
    assert_eq!(status(&mut recv).await, Status::Ok);
    (connection, send, recv)
}

/// Sends a listener message and reads the relay's answer.
pub(super) async fn quic_message(
    send: &mut SendStream,
    recv: &mut RecvStream,
    message: Message,
) -> Status {
    send.write_all(&message.encode().unwrap()).await.unwrap();
    status(recv).await
}

/// Dials `route` as [`phone`].
async fn quic_dial(connection: &Connection, route: RouteId) -> (Status, SendStream, RecvStream) {
    quic_dial_as(connection, route, &phone()).await
}

/// Dials `route` as `dialer`, signing over this connection.
pub(super) async fn quic_dial_as(
    connection: &Connection,
    route: RouteId,
    dialer: &SigningKey,
) -> (Status, SendStream, RecvStream) {
    let (mut send, mut recv) = connection.open_bi().await.unwrap();
    let request = Request::Dial {
        key: public(dialer),
        route,
        signature: sign(dialer, &wire::dial_signed(&exported(connection), &route)),
    };
    send.write_all(&request.encode().unwrap()).await.unwrap();
    (status(&mut recv).await, send, recv)
}

/// The listener's side of the next dial, and the dialer's key.
pub(super) async fn quic_accept_from(listener: &Connection) -> (SendStream, RecvStream, Key) {
    let (send, mut recv) = tokio::time::timeout(Duration::from_secs(5), listener.accept_bi())
        .await
        .expect("no incoming stream in time")
        .unwrap();
    let mut header = [0; 34];
    recv.read_exact(&mut header).await.unwrap();
    assert_eq!(header[0], INCOMING);
    (send, recv, header[1..].try_into().unwrap())
}

async fn quic_accept(listener: &Connection) -> (SendStream, RecvStream) {
    let (send, recv, key) = quic_accept_from(listener).await;
    assert_eq!(key, public(&phone()));
    (send, recv)
}

#[tokio::test]
async fn a_session_is_a_stream_and_bytes_cross_untouched() {
    let relay = start_both(&options(Limits::default())).await;
    let key = signing_key(21);
    let route = RouteId::of(key.verifying_key());
    let (listener, _listen_send, _listen_recv) = quic_listen(&relay, &key).await;
    let phone = connect(&relay).await;

    let dial = tokio::spawn({
        let phone = phone.clone();
        async move { quic_dial(&phone, route).await }
    });
    let (mut to_phone, mut from_phone) = quic_accept(&listener).await;
    let (dialed, mut to_computer, mut from_computer) = dial.await.unwrap();
    assert_eq!(dialed, Status::Ok);

    // A megabyte each way, in odd chunks, arrives whole and in order.
    let payload: Vec<u8> = (0..1_048_576u32).map(|byte| (byte % 251) as u8).collect();
    let sending = payload.clone();
    let writer = tokio::spawn(async move {
        for chunk in sending.chunks(7_919) {
            to_computer.write_all(chunk).await.unwrap();
        }
        to_computer
    });
    let mut received = vec![0; payload.len()];
    from_phone.read_exact(&mut received).await.unwrap();
    assert_eq!(received, payload);
    let _to_computer = writer.await.unwrap();

    to_phone.write_all(b"reply").await.unwrap();
    let mut reply = [0; 5];
    from_computer.read_exact(&mut reply).await.unwrap();
    assert_eq!(&reply, b"reply");

    let totals = relay.hub.metrics.totals();
    assert_eq!(totals.bytes_from_dialers, payload.len() as u64);
    assert_eq!(totals.bytes_from_listeners, 5);
    assert_eq!(relay.hub.metrics.read(Gauge::Sessions), 1);
    assert_eq!(relay.hub.metrics.read(Gauge::Connections(Kind::Quic)), 2);
}

#[tokio::test]
async fn one_connection_carries_sessions_to_several_computers() {
    let relay = start_both(&options(Limits::default())).await;
    let keys = [signing_key(22), signing_key(23)];
    let mut listeners = Vec::new();
    for key in &keys {
        listeners.push(quic_listen(&relay, key).await);
    }
    let phone = connect(&relay).await;
    // Dropping a stream ends its session, so each is held to the end.
    let mut open = Vec::new();
    for (key, (listener, ..)) in keys.iter().zip(&listeners) {
        let route = RouteId::of(key.verifying_key());
        let dial = tokio::spawn({
            let phone = phone.clone();
            async move { quic_dial(&phone, route).await }
        });
        let (mut to_phone, from_phone) = quic_accept(listener).await;
        let (dialed, send, mut recv) = dial.await.unwrap();
        assert_eq!(dialed, Status::Ok);
        to_phone.write_all(&route.as_bytes()[..4]).await.unwrap();
        let mut first = [0; 4];
        recv.read_exact(&mut first).await.unwrap();
        assert_eq!(first, route.as_bytes()[..4]);
        open.push((to_phone, from_phone, send, recv));
    }
    assert_eq!(relay.hub.metrics.read(Gauge::Sessions), 2);
}

#[tokio::test]
async fn refusals_answer_with_a_status() {
    let options = Options {
        listen_token: Some(SecretString::from("a token of sixteen chars")),
        ..options(Limits::default())
    };
    let relay = start_both(&options).await;
    let connection = connect(&relay).await;

    let offline = RouteId::of(signing_key(24).verifying_key());
    assert_eq!(quic_dial(&connection, offline).await.0, Status::Offline);
    // A dial signed for another connection is refused like an offline one.
    let other_connection = connect(&relay).await;
    let (mut send, mut recv) = connection.open_bi().await.unwrap();
    let request = Request::Dial {
        key: public(&phone()),
        route: offline,
        signature: sign(
            &phone(),
            &wire::dial_signed(&exported(&other_connection), &offline),
        ),
    };
    send.write_all(&request.encode().unwrap()).await.unwrap();
    assert_eq!(status(&mut recv).await, Status::Offline);

    // Missing token.
    let (mut send, mut recv) = connection.open_bi().await.unwrap();
    let request = listen_request(&connection, &signing_key(25), b"");
    send.write_all(&request.encode().unwrap()).await.unwrap();
    assert_eq!(status(&mut recv).await, Status::Forbidden);

    // A proof made for another connection does not hold on this one.
    let other = connect(&relay).await;
    let (mut send, mut recv) = connection.open_bi().await.unwrap();
    let request = listen_request(&other, &signing_key(25), b"a token of sixteen chars");
    send.write_all(&request.encode().unwrap()).await.unwrap();
    assert_eq!(status(&mut recv).await, Status::Refused);

    // The right token and proof.
    let (mut send, mut recv) = connection.open_bi().await.unwrap();
    let request = listen_request(&connection, &signing_key(25), b"a token of sixteen chars");
    send.write_all(&request.encode().unwrap()).await.unwrap();
    assert_eq!(status(&mut recv).await, Status::Ok);

    // A request kind that does not exist.
    let (mut send, mut recv) = connection.open_bi().await.unwrap();
    send.write_all(&[0x7f]).await.unwrap();
    assert_eq!(status(&mut recv).await, Status::Malformed);
}

#[tokio::test]
async fn a_wrong_pin_fails_the_handshake() {
    let relay = start_both(&options(Limits::default())).await;
    assert!(connect_pinned(relay.quic, [0; 32]).await.is_err());
}

#[tokio::test]
async fn a_newer_listener_replaces_the_older() {
    let relay = start_both(&options(Limits::default())).await;
    let key = signing_key(26);
    let (_first, _send, mut first_recv) = quic_listen(&relay, &key).await;
    let (_second, _send2, _recv2) = quic_listen(&relay, &key).await;
    let mut byte = [0; 1];
    let read = tokio::time::timeout(Duration::from_secs(5), first_recv.read(&mut byte))
        .await
        .unwrap();
    assert!(
        matches!(read, Err(quinn::ReadError::Reset(code)) if code.into_inner() == u64::from(wire::code::REPLACED))
    );
    assert_eq!(relay.hub.metrics.read(Gauge::Listeners), 1);
}

#[tokio::test]
async fn closing_the_listen_stream_stops_listening() {
    let relay = start_both(&options(Limits::default())).await;
    let key = signing_key(27);
    let (_connection, mut send, _recv) = quic_listen(&relay, &key).await;
    send.finish().unwrap();
    for _ in 0..200 {
        if relay.hub.metrics.read(Gauge::Listeners) == 0 {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("still listening");
}

#[tokio::test]
async fn a_quic_dialer_reaches_a_websocket_listener() {
    let relay = start_both(&options(Limits::default())).await;
    let key = signing_key(28);
    let (mut listener, route) = listen(relay.http, &key, None).await;
    let phone = connect(&relay).await;
    let route = RouteId::parse(&route).unwrap();
    let dial = tokio::spawn({
        let phone = phone.clone();
        async move { quic_dial(&phone, route).await }
    });
    let ToPeer::Incoming { session, .. } = next_control(&mut listener).await else {
        panic!("expected incoming");
    };
    let mut accepted = super::connect(relay.http, &format!("/v1/accept/{session}"))
        .await
        .unwrap();
    let (dialed, mut send, mut recv) = dial.await.unwrap();
    assert_eq!(dialed, Status::Ok);
    send.write_all(b"over quic").await.unwrap();
    assert_eq!(
        next(&mut accepted).await,
        Some(Frame::Binary(b"over quic".to_vec().into()))
    );
    accepted
        .send(Frame::Binary(b"over ws".to_vec().into()))
        .await
        .unwrap();
    let mut reply = [0; 7];
    recv.read_exact(&mut reply).await.unwrap();
    assert_eq!(&reply, b"over ws");
}

#[tokio::test]
async fn a_websocket_dialer_reaches_a_quic_listener() {
    let relay = start_both(&options(Limits::default())).await;
    let key = signing_key(29);
    let route = RouteId::of(key.verifying_key());
    let (listener, _send, _recv) = quic_listen(&relay, &key).await;
    let address = relay.http;
    let dial = tokio::spawn(async move { dial_as(address, &route.to_string(), &phone()).await });
    let (mut to_phone, mut from_phone) = quic_accept(&listener).await;
    let (mut dialer, dialed) = dial.await.unwrap();
    assert_eq!(dialed, Status::Ok);
    dialer
        .send(Frame::Binary(b"over ws".to_vec().into()))
        .await
        .unwrap();
    let mut first = [0; 7];
    from_phone.read_exact(&mut first).await.unwrap();
    assert_eq!(&first, b"over ws");
    to_phone.write_all(b"over quic").await.unwrap();
    assert_eq!(
        next(&mut dialer).await,
        Some(Frame::Binary(b"over quic".to_vec().into()))
    );
}

#[tokio::test]
async fn info_tells_clients_how_to_reach_quic() {
    let relay = start_both(&options(Limits::default())).await;
    let mut stream = TcpStream::connect(relay.http).await.unwrap();
    let request = format!(
        "GET /v1/info HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
        relay.http
    );
    tokio::io::AsyncWriteExt::write_all(&mut stream, request.as_bytes())
        .await
        .unwrap();
    let mut response = String::new();
    tokio::io::AsyncReadExt::read_to_string(&mut stream, &mut response)
        .await
        .unwrap();
    let body = response.split("\r\n\r\n").nth(1).unwrap();
    let info: crate::endpoint::Info = serde_json::from_str(body).unwrap();
    let quic = info.quic.unwrap();
    assert_eq!(quic.port, relay.quic.port());
    assert_eq!(URL_SAFE_NO_PAD.decode(quic.pin).unwrap(), relay.pin);
}

/// A pinned client connection to `relay`, for other test files.
pub(super) async fn connect_to(relay: &Relay) -> Connection {
    connect(relay).await
}
