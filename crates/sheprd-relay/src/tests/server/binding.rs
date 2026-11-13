//! WebSocket proofs bind the relay's own configured name, never the
//! request's `Host`: a relay that passes another relay's challenge through
//! cannot get its proof accepted by claiming its own name in the header.

use super::*;
use crate::{metrics::Gauge, splice::code};

/// What a malicious relay "evil.example" would do with relay B's
/// challenge: hand it to a computer that thinks it talks to evil.example,
/// then present that proof to B with `Host: evil.example`.
#[tokio::test]
async fn a_proof_for_another_relay_is_refused_whatever_the_host_header_says() {
    let (address, hub) = start(&options(Limits::default())).await;
    let mut socket = connect_as_host(address, "/v1/listen", Some("evil.example"))
        .await
        .unwrap();
    let nonce = challenge(&mut socket).await;
    let proof = hello(
        &signing_key(80),
        &socket::listen_signed("evil.example", &nonce),
    );
    send_json(&mut socket, &proof).await;
    assert_eq!(close_code(&mut socket).await, Some(code::REFUSED));
    assert_eq!(hub.metrics.read(Gauge::Listeners), 0);

    // A dial proof passed through the same way does not get through either,
    // even to a computer that allows the key.
    let (_listener, route) = listen(address, &signing_key(81), None).await;
    let route_id = RouteId::parse(&route).unwrap();
    let mut socket = connect_as_host(address, &format!("/v1/dial/{route}"), Some("evil.example"))
        .await
        .unwrap();
    let nonce = challenge(&mut socket).await;
    let hello = hello(
        &phone(),
        &socket::dial_signed("evil.example", &nonce, &route_id),
    );
    send_json(&mut socket, &hello).await;
    let ToPeer::Status { status } = next_control(&mut socket).await else {
        panic!("expected a status");
    };
    assert_eq!(status, Status::Offline);
}

#[tokio::test]
async fn a_proof_over_the_relays_own_name_holds_whatever_the_host_header_says() {
    let options = Options {
        public_host: Some("relay.example".to_owned()),
        ..options(Limits::default())
    };
    let (address, hub) = start(&options).await;
    let mut socket = connect_as_host(address, "/v1/listen", Some("other.example"))
        .await
        .unwrap();
    let nonce = challenge(&mut socket).await;
    let hello = hello(
        &signing_key(82),
        &socket::listen_signed("relay.example", &nonce),
    );
    send_json(&mut socket, &hello).await;
    let ToPeer::Ready { .. } = next_control(&mut socket).await else {
        panic!("expected ready");
    };
    assert_eq!(hub.metrics.read(Gauge::Listeners), 1);
}

#[tokio::test]
async fn without_a_public_name_the_websocket_fallback_is_off() {
    let (router, _hub) = app(&options(Limits::default()), None).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(
            listener,
            router.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
    });
    for path in [
        "/v1/listen".to_owned(),
        format!("/v1/dial/{}", RouteId::of(signing_key(83).verifying_key())),
    ] {
        let error = connect(address, &path).await.unwrap_err();
        assert!(
            matches!(error, tungstenite::Error::Http(response) if response.status() == 404),
            "{path}"
        );
    }
    let mut stream = TcpStream::connect(address).await.unwrap();
    let request = format!("GET /v1/info HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n");
    tokio::io::AsyncWriteExt::write_all(&mut stream, request.as_bytes())
        .await
        .unwrap();
    let mut response = String::new();
    tokio::io::AsyncReadExt::read_to_string(&mut stream, &mut response)
        .await
        .unwrap();
    let info: crate::endpoint::Info =
        serde_json::from_str(response.split("\r\n\r\n").nth(1).unwrap()).unwrap();
    assert!(!info.websocket);
}

#[test]
fn the_public_name_is_normalized_like_an_endpoint() {
    use crate::options::public_host;
    assert_eq!(
        public_host("Relay.Example.org").as_deref(),
        Some("relay.example.org")
    );
    assert_eq!(
        public_host("relay.example.org:443").as_deref(),
        Some("relay.example.org")
    );
    assert_eq!(
        public_host("relay.example.org:8443").as_deref(),
        Some("relay.example.org:8443")
    );
    for bad in [
        "",
        "https://relay.example.org/x",
        "user@relay.example.org",
        "relay example",
    ] {
        assert_eq!(public_host(bad), None, "{bad}");
    }
}
