//! The relay end to end over real sockets: listening, dialing, splicing,
//! and each limit and refusal.

use super::*;
use crate::{
    RouteId,
    metrics::{Gauge, Kind},
    splice::code,
};

async fn wait_for(mut done: impl FnMut() -> bool) {
    for _ in 0..200 {
        if done() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("condition not reached in time");
}

#[tokio::test]
async fn bytes_cross_both_ways_untouched() {
    let (address, hub) = start(&options(Limits::default())).await;
    let key = signing_key(1);
    let (mut listener, route) = listen(address, &key, None).await;
    assert_eq!(route, RouteId::of(key.verifying_key()).to_string());

    let (mut dialer, mut accepted) = session(address, &mut listener, &route).await;
    dialer
        .send(Frame::Binary(vec![1, 2, 3].into()))
        .await
        .unwrap();
    assert_eq!(
        next(&mut accepted).await,
        Some(Frame::Binary(vec![1, 2, 3].into()))
    );
    accepted
        .send(Frame::Binary(vec![9; 1000].into()))
        .await
        .unwrap();
    assert_eq!(
        next(&mut dialer).await,
        Some(Frame::Binary(vec![9; 1000].into()))
    );

    assert_eq!(hub.metrics.read(Gauge::Sessions), 1);
    let totals = hub.metrics.totals();
    assert_eq!(totals.bytes_from_dialers, 3);
    assert_eq!(totals.bytes_from_listeners, 1000);
    assert_eq!(hub.sessions().len(), 1);

    dialer.close(None).await.unwrap();
    assert_eq!(close_code(&mut accepted).await, Some(code::PEER_GONE));
    wait_for(|| hub.metrics.read(Gauge::Sessions) == 0).await;
    wait_for(|| hub.metrics.read(Gauge::Connections(Kind::Dial)) == 0).await;
    wait_for(|| hub.metrics.read(Gauge::Connections(Kind::Accept)) == 0).await;
    assert!(hub.sessions().is_empty());
}

#[tokio::test]
async fn a_text_frame_ends_the_session() {
    let (address, _hub) = start(&options(Limits::default())).await;
    let (mut listener, route) = listen(address, &signing_key(2), None).await;
    let (mut dialer, mut accepted) = session(address, &mut listener, &route).await;
    dialer.send(Frame::Text("hello".into())).await.unwrap();
    assert_eq!(close_code(&mut accepted).await, Some(code::PROTOCOL));
}

#[tokio::test]
async fn an_offline_route_answers_offline() {
    let (address, hub) = start(&options(Limits::default())).await;
    let route = RouteId::of(signing_key(3).verifying_key()).to_string();
    let (mut socket, status) = dial_as(address, &route, &phone()).await;
    assert_eq!(status, Status::Offline);
    assert_eq!(close_code(&mut socket).await, Some(code::OFFLINE));
    let error = connect(address, "/v1/dial/not-a-route").await.unwrap_err();
    assert!(matches!(error, tungstenite::Error::Http(response) if response.status() == 404));
    wait_for(|| hub.metrics.read(Gauge::Connections(Kind::Dial)) == 0).await;
}

#[tokio::test]
async fn a_listener_that_never_accepts_times_the_dial_out() {
    let limits = Limits {
        accept_timeout: Duration::from_millis(200),
        ..Limits::default()
    };
    let (address, hub) = start(&options(limits)).await;
    let (mut listener, route) = listen(address, &signing_key(4), None).await;
    let (_socket, status) = dial_as(address, &route, &phone()).await;
    assert_eq!(status, Status::Timeout);
    let ToPeer::Incoming { session, .. } = next_control(&mut listener).await else {
        panic!("expected incoming");
    };
    // The dial is gone, so a late accept finds nothing.
    let late = connect(address, &format!("/v1/accept/{session}"))
        .await
        .unwrap_err();
    assert!(matches!(late, tungstenite::Error::Http(response) if response.status() == 404));
    assert_eq!(hub.metrics.read(Gauge::Pending), 0);
}

#[tokio::test]
async fn a_bad_signature_is_refused() {
    let (address, hub) = start(&options(Limits::default())).await;
    let mut socket = connect(address, "/v1/listen").await.unwrap();
    let ToPeer::Challenge { .. } = next_control(&mut socket).await else {
        panic!("expected a challenge");
    };
    // Signed over a different nonce.
    let hello = hello(
        &signing_key(5),
        &socket::listen_signed(&address.to_string(), &[0; NONCE_LEN]),
    );
    socket
        .send(Frame::Text(serde_json::to_string(&hello).unwrap().into()))
        .await
        .unwrap();
    assert_eq!(close_code(&mut socket).await, Some(code::REFUSED));
    assert_eq!(hub.metrics.read(Gauge::Listeners), 0);
}

#[tokio::test]
async fn a_listen_token_is_required_when_set() {
    let options = Options {
        listen_token: Some(SecretString::from("a token of sixteen chars")),
        ..options(Limits::default())
    };
    let (address, _hub) = start(&options).await;
    let (_listener, _route) =
        listen(address, &signing_key(6), Some("a token of sixteen chars")).await;

    let mut socket = connect(address, "/v1/listen").await.unwrap();
    let ToPeer::Challenge { nonce, .. } = next_control(&mut socket).await else {
        panic!("expected a challenge");
    };
    let nonce: [u8; NONCE_LEN] = URL_SAFE_NO_PAD.decode(nonce).unwrap().try_into().unwrap();
    let hello = hello(
        &signing_key(7),
        &socket::listen_signed(&address.to_string(), &nonce),
    );
    socket
        .send(Frame::Text(serde_json::to_string(&hello).unwrap().into()))
        .await
        .unwrap();
    assert_eq!(close_code(&mut socket).await, Some(code::REFUSED));
}

#[tokio::test]
async fn a_second_listener_with_the_same_key_replaces_the_first() {
    let (address, hub) = start(&options(Limits::default())).await;
    let key = signing_key(8);
    let (mut first, route) = listen(address, &key, None).await;
    let (mut second, again) = listen(address, &key, None).await;
    assert_eq!(route, again);
    assert_eq!(close_code(&mut first).await, Some(code::REPLACED));
    assert_eq!(hub.metrics.read(Gauge::Listeners), 1);
    // Dials reach the new listener.
    let (_dialer, _accepted) = session(address, &mut second, &route).await;
}

#[tokio::test]
async fn connections_per_address_are_capped() {
    let limits = Limits {
        max_connections_per_ip: 2,
        ..Limits::default()
    };
    let (address, hub) = start(&options(limits)).await;
    let (_first, _) = listen(address, &signing_key(9), None).await;
    let (_second, _) = listen(address, &signing_key(10), None).await;
    let error = connect(address, "/v1/listen").await.unwrap_err();
    assert!(matches!(error, tungstenite::Error::Http(response) if response.status() == 429));
    assert_eq!(hub.metrics.read(Gauge::Listeners), 2);
}

#[tokio::test]
async fn sessions_per_route_are_capped() {
    let limits = Limits {
        max_sessions_per_route: 1,
        ..Limits::default()
    };
    let (address, _hub) = start(&options(limits)).await;
    let (mut listener, route) = listen(address, &signing_key(11), None).await;
    let (_dialer, _accepted) = session(address, &mut listener, &route).await;
    let (_socket, status) = dial_as(address, &route, &phone()).await;
    assert_eq!(status, Status::Busy);
}

#[tokio::test]
async fn an_idle_session_is_closed() {
    let limits = Limits {
        idle_timeout: Duration::from_millis(300),
        ..Limits::default()
    };
    let (address, hub) = start(&options(limits)).await;
    let (mut listener, route) = listen(address, &signing_key(12), None).await;
    let (mut dialer, mut accepted) = session(address, &mut listener, &route).await;
    assert_eq!(close_code(&mut dialer).await, Some(code::IDLE));
    assert_eq!(close_code(&mut accepted).await, Some(code::IDLE));
    wait_for(|| hub.metrics.read(Gauge::Sessions) == 0).await;
}

#[tokio::test]
async fn an_oversized_frame_ends_the_session() {
    let limits = Limits {
        max_frame: 1024,
        ..Limits::default()
    };
    let (address, _hub) = start(&options(limits)).await;
    let (mut listener, route) = listen(address, &signing_key(13), None).await;
    let (mut dialer, mut accepted) = session(address, &mut listener, &route).await;
    dialer
        .send(Frame::Binary(vec![0; 4096].into()))
        .await
        .unwrap();
    assert_ne!(
        next(&mut accepted).await,
        Some(Frame::Binary(vec![0; 4096].into()))
    );
}

#[tokio::test]
async fn session_bandwidth_is_held_to_its_limit() {
    let limits = Limits {
        session_bandwidth: 64 * 1024,
        ..Limits::default()
    };
    let (address, _hub) = start(&options(limits)).await;
    let (mut listener, route) = listen(address, &signing_key(14), None).await;
    let (mut dialer, mut accepted) = session(address, &mut listener, &route).await;
    let started = std::time::Instant::now();
    // One second's allowance passes at once; the next 64 KiB owe a second.
    for _ in 0..4 {
        dialer
            .send(Frame::Binary(vec![0; 32 * 1024].into()))
            .await
            .unwrap();
    }
    for _ in 0..4 {
        assert!(matches!(next(&mut accepted).await, Some(Frame::Binary(_))));
    }
    assert!(
        started.elapsed() >= Duration::from_millis(900),
        "{:?}",
        started.elapsed()
    );
}

#[tokio::test]
async fn info_says_whether_listening_needs_a_token() {
    for (token, needed) in [(None, false), (Some("a token of sixteen chars"), true)] {
        let options = Options {
            listen_token: token.map(SecretString::from),
            ..options(Limits::default())
        };
        let (address, _hub) = start(&options).await;
        let endpoint = crate::Endpoint::parse(&format!("http://{address}")).unwrap();
        let url = endpoint.info();
        let mut stream = TcpStream::connect(address).await.unwrap();
        let request = format!(
            "GET {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
            url.path(),
            endpoint.host()
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
        assert_eq!(info.listen_token, needed);
        assert_eq!(info.protocol, crate::endpoint::PROTOCOL);
    }
}

#[tokio::test]
async fn a_listener_signs_the_host_its_endpoint_names() {
    let (address, _hub) = start(&options(Limits::default())).await;
    let endpoint = crate::Endpoint::parse(&format!("http://{address}")).unwrap();
    // `listen` signs `address.to_string()`, which must be what the
    // endpoint reports, or a real client's proof would fail.
    assert_eq!(endpoint.host(), address.to_string());
    let (_listener, _route) = listen(address, &signing_key(15), None).await;
}
