//! The relay service: a relay on a loopback port, and listener and dialer
//! sockets that speak its protocol.

mod auth;
mod bans;
mod binding;
mod bucket;
mod capped;
mod cross_site;
mod dashboard;
mod guard;
mod history;
mod identity;
mod keys;
mod lockout;
mod metrics;
mod options;
mod pools;
mod quic;
mod races;
mod relay;
mod review;
mod review2;
mod shared_address;
mod tokens;

pub(crate) use super::{hello, public, sign, signing_key};
use crate::{
    Limits, Options, RouteId,
    hub::Hub,
    serve::app,
    socket::{self, FromListener, NONCE_LEN, ToPeer},
    wire::{Key, Status},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use futures_util::{SinkExt, StreamExt};
use p256::ecdsa::SigningKey;
use secrecy::SecretString;
use std::{net::SocketAddr, sync::Arc, time::Duration};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::{
    WebSocketStream, client_async,
    tungstenite::{self, Message as Frame, client::IntoClientRequest},
};

pub(crate) type Socket = WebSocketStream<TcpStream>;

pub(crate) const TOKEN: &str = "a dashboard token of enough length";

pub(crate) fn options(limits: Limits) -> Options {
    Options {
        dashboard_token: Some(SecretString::from(TOKEN)),
        limits,
        ..Options::default()
    }
}

/// A relay serving both transports on loopback ports.
pub(crate) struct Relay {
    pub http: SocketAddr,
    pub hub: Arc<Hub>,
    pub pin: crate::quic::tls::Pin,
    pub quic: SocketAddr,
}

/// Serves a relay with `options` on loopback ports, QUIC included.
pub(crate) async fn start_both(options: &Options) -> Relay {
    let (endpoint, info) = crate::bind_quic("127.0.0.1:0".parse().unwrap(), None, None).unwrap();
    let quic = endpoint.local_addr().unwrap();
    let pin = URL_SAFE_NO_PAD
        .decode(&info.pin)
        .unwrap()
        .try_into()
        .unwrap();
    let (http, hub) = serve_http(options, Some(info)).await;
    tokio::spawn(crate::quic::run(endpoint, Arc::clone(&hub)));
    Relay {
        http,
        hub,
        pin,
        quic,
    }
}

/// Serves a relay with `options` on a loopback port, WebSockets only.
pub(crate) async fn start(options: &Options) -> (SocketAddr, Arc<Hub>) {
    serve_http(options, None).await
}

async fn serve_http(
    options: &Options,
    quic: Option<crate::endpoint::QuicInfo>,
) -> (SocketAddr, Arc<Hub>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    // The relay's name is its loopback address, as clients' endpoints give
    // it, unless the test chose otherwise.
    let options = Options {
        public_host: options
            .public_host
            .clone()
            .or_else(|| Some(address.to_string())),
        ..options.clone()
    };
    let (router, hub) = app(&options, quic).unwrap();
    tokio::spawn(async move {
        axum::serve(
            listener,
            router.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
    });
    (address, hub)
}

pub(crate) async fn connect(address: SocketAddr, path: &str) -> Result<Socket, tungstenite::Error> {
    connect_as_host(address, path, None).await
}

/// Connects with `host` in the `Host` header instead of the address, as a
/// relay passing a proof through would.
pub(crate) async fn connect_as_host(
    address: SocketAddr,
    path: &str,
    host: Option<&str>,
) -> Result<Socket, tungstenite::Error> {
    let mut request = format!("ws://{address}{path}")
        .into_client_request()
        .unwrap();
    if let Some(host) = host {
        request.headers_mut().insert("host", host.parse().unwrap());
    }
    let stream = TcpStream::connect(address).await.unwrap();
    client_async(request, stream)
        .await
        .map(|(socket, _)| socket)
}

/// The next frame that is not a ping or pong, within five seconds.
pub(crate) async fn next(socket: &mut Socket) -> Option<Frame> {
    let read = async {
        loop {
            match socket.next().await? {
                Ok(Frame::Ping(_) | Frame::Pong(_)) => {}
                Ok(frame) => return Some(frame),
                Err(_) => return None,
            }
        }
    };
    tokio::time::timeout(Duration::from_secs(5), read)
        .await
        .expect("no frame in time")
}

pub(crate) async fn next_control(socket: &mut Socket) -> ToPeer {
    match next(socket).await {
        Some(Frame::Text(text)) => serde_json::from_str(&text).unwrap(),
        other => panic!("expected a control message, got {other:?}"),
    }
}

/// The close code a socket ends with.
pub(crate) async fn close_code(socket: &mut Socket) -> Option<u16> {
    match next(socket).await {
        Some(Frame::Close(Some(frame))) => Some(frame.code.into()),
        _ => None,
    }
}

/// The phone every fixture dials as, unless a test says otherwise.
pub(crate) fn phone() -> SigningKey {
    signing_key(200)
}

pub(crate) async fn send_json(socket: &mut Socket, value: &impl serde::Serialize) {
    socket
        .send(Frame::Text(serde_json::to_string(value).unwrap().into()))
        .await
        .unwrap();
}

pub(crate) async fn challenge(socket: &mut Socket) -> [u8; NONCE_LEN] {
    let ToPeer::Challenge { nonce, version } = next_control(socket).await else {
        panic!("expected a challenge");
    };
    assert_eq!(version, 1);
    URL_SAFE_NO_PAD.decode(nonce).unwrap().try_into().unwrap()
}

/// Opens a listener for `key` that allows [`phone`] to dial; its socket
/// and route.
pub(crate) async fn listen(
    address: SocketAddr,
    key: &SigningKey,
    token: Option<&str>,
) -> (Socket, String) {
    let (mut socket, route) = listen_only(address, key, token).await;
    assert_eq!(allow(&mut socket, &[public(&phone())]).await, Status::Ok);
    (socket, route)
}

/// Opens a listener for `key` that allows no one yet.
pub(crate) async fn listen_only(
    address: SocketAddr,
    key: &SigningKey,
    token: Option<&str>,
) -> (Socket, String) {
    let mut socket = connect(address, "/v1/listen").await.unwrap();
    let nonce = challenge(&mut socket).await;
    let mut hello = hello(key, &socket::listen_signed(&address.to_string(), &nonce));
    hello.token = token.map(str::to_owned);
    send_json(&mut socket, &hello).await;
    let ToPeer::Ready { route } = next_control(&mut socket).await else {
        panic!("expected ready");
    };
    (socket, route)
}

/// Tells the relay which keys may dial this listener.
pub(crate) async fn allow(listener: &mut Socket, keys: &[Key]) -> Status {
    let keys = keys.iter().map(|key| URL_SAFE_NO_PAD.encode(key)).collect();
    send_json(listener, &FromListener::DialKeys { keys }).await;
    status_of(listener).await
}

/// Adds a one-time pairing key to this listener.
pub(crate) async fn allow_pairing(listener: &mut Socket, key: Key, seconds: u16) -> Status {
    let key = URL_SAFE_NO_PAD.encode(key);
    send_json(listener, &FromListener::PairingKey { key, seconds }).await;
    status_of(listener).await
}

async fn status_of(socket: &mut Socket) -> Status {
    let ToPeer::Status { status } = next_control(socket).await else {
        panic!("expected a status");
    };
    status
}

/// Dials `route` as `dialer`; the socket and the relay's answer.
pub(crate) async fn dial_as(
    address: SocketAddr,
    route: &str,
    dialer: &SigningKey,
) -> (Socket, Status) {
    let mut socket = connect(address, &format!("/v1/dial/{route}"))
        .await
        .unwrap();
    let nonce = challenge(&mut socket).await;
    let route = RouteId::parse(route).unwrap();
    let signed = socket::dial_signed(&address.to_string(), &nonce, &route);
    send_json(&mut socket, &hello(dialer, &signed)).await;
    let status = status_of(&mut socket).await;
    (socket, status)
}

/// Dials `route` as [`phone`] and accepts on `listener`; the two ends of
/// the session.
pub(crate) async fn session(
    address: SocketAddr,
    listener: &mut Socket,
    route: &str,
) -> (Socket, Socket) {
    session_as(address, listener, route, &phone()).await
}

/// Dials `route` as `dialer` and accepts on `listener`.
pub(crate) async fn session_as(
    address: SocketAddr,
    listener: &mut Socket,
    route: &str,
    dialer: &SigningKey,
) -> (Socket, Socket) {
    let dial = tokio::spawn({
        let route = route.to_owned();
        let dialer = dialer.clone();
        async move { dial_as(address, &route, &dialer).await }
    });
    let ToPeer::Incoming { session, key } = next_control(listener).await else {
        panic!("expected incoming");
    };
    assert_eq!(URL_SAFE_NO_PAD.decode(key).unwrap(), public(dialer));
    let accepted = connect(address, &format!("/v1/accept/{session}"))
        .await
        .unwrap();
    let (dialer, status) = dial.await.unwrap();
    assert_eq!(status, Status::Ok);
    (dialer, accepted)
}
