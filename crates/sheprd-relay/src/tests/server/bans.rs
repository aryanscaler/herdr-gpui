//! Bans end to end: a banned address meets 403 on HTTP and a refusal on
//! QUIC, its sessions and listeners end at once, the ban file applies
//! without a restart, the dashboard and metrics show it, and a dashboard
//! lockout stays the dashboard's.

use super::{quic::*, *};
use crate::{
    guard::parse_list,
    hub::Access,
    metrics::{Gauge, Offense},
    splice::code,
};
use axum::{
    body::Body,
    extract::connect_info::MockConnectInfo,
    http::{Request, StatusCode},
};
use tower::ServiceExt;

fn loopback_banned() -> Options {
    Options {
        bans: parse_list("127.0.0.1").0,
        ..options(Limits::default())
    }
}

#[tokio::test]
async fn a_manually_banned_address_is_refused_everywhere_but_healthz() {
    let relay = start_both(&loopback_banned()).await;
    let error = connect(relay.http, "/v1/listen").await.unwrap_err();
    assert!(matches!(error, tungstenite::Error::Http(response) if response.status() == 403));
    for (path, status) in [
        ("/healthz", 200),
        ("/v1/info", 403),
        ("/dashboard/login", 403),
    ] {
        let mut stream = TcpStream::connect(relay.http).await.unwrap();
        let request = format!("GET {path} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n");
        tokio::io::AsyncWriteExt::write_all(&mut stream, request.as_bytes())
            .await
            .unwrap();
        let mut response = String::new();
        tokio::io::AsyncReadExt::read_to_string(&mut stream, &mut response)
            .await
            .unwrap();
        assert!(
            response.starts_with(&format!("HTTP/1.1 {status}")),
            "{path}: {response}"
        );
    }
    // QUIC refuses it before the handshake.
    assert!(
        tokio::time::timeout(
            Duration::from_secs(5),
            connect_with(relay.quic, relay.pin, crate::quic::client_transport())
        )
        .await
        .map_or(true, |connected| connected.is_err())
    );
}

#[tokio::test]
async fn a_ban_ends_open_sessions_and_listeners_at_once() {
    let (address, hub) = start(&options(Limits::default())).await;
    let (mut listener, route) = listen(address, &signing_key(110), None).await;
    let (mut dialer, mut accepted) = session(address, &mut listener, &route).await;
    let ranges = parse_list("127.0.0.0/8").0;
    hub.guard.set_manual(ranges.clone());
    hub.sweep();
    assert_eq!(close_code(&mut dialer).await, Some(code::BANNED));
    assert_eq!(close_code(&mut accepted).await, Some(code::BANNED));
    assert!(close_code(&mut listener).await.is_some());
    assert_eq!(hub.metrics.read(Gauge::Listeners), 0);
}

#[tokio::test]
async fn an_earned_ban_ends_that_addresses_sessions() {
    let hub = Arc::new(Hub::new(Limits::default(), Access::Open));
    let computer: std::net::IpAddr = "198.51.100.20".parse().unwrap();
    let toxic: std::net::IpAddr = "203.0.113.60".parse().unwrap();
    let route = RouteId::of(signing_key(111).verifying_key());
    let registration = hub.register(route, computer).unwrap();
    registration.set_dial_keys(&[public(&phone())]);
    let waiting = hub.dial(route, public(&phone()), toxic).unwrap().unwrap();
    let session = hub.open(waiting.slot).unwrap();
    for _ in 0..10 {
        hub.offend(toxic, Offense::Malformed);
    }
    assert!(hub.is_banned(toxic));
    tokio::time::timeout(Duration::from_secs(1), session.banned())
        .await
        .expect("the session was not told");
    // The computer it talked to is not banned.
    assert!(!hub.is_banned(computer));
}

#[tokio::test]
async fn the_ban_file_applies_without_a_restart() {
    let hub = Arc::new(Hub::new(Limits::default(), Access::Open));
    let file = std::env::temp_dir().join(format!("sheprd-relay-bans-{}", std::process::id()));
    std::fs::write(&file, "# toxic\n203.0.113.0/24\n").unwrap();
    let watcher = tokio::spawn(crate::serve::watch_ban_file(
        Arc::clone(&hub),
        file.clone(),
        parse_list("198.51.100.1").0,
    ));
    let mut banned = false;
    for _ in 0..100 {
        banned = hub.is_banned("203.0.113.9".parse().unwrap());
        if banned {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    watcher.abort();
    std::fs::remove_file(&file).unwrap();
    assert!(banned);
    // `--ban` ranges stay alongside the file's.
    assert!(hub.is_banned("198.51.100.1".parse().unwrap()));
    assert!(!hub.is_banned("192.0.2.1".parse().unwrap()));
}

#[tokio::test]
async fn three_wrong_dashboard_tokens_block_the_dashboard_not_the_relay() {
    let options = Options {
        behind_proxy: true,
        ..options(Limits::default())
    };
    let (router, hub) = app(&options, None).unwrap();
    let router = router.layer(MockConnectInfo(SocketAddr::from(([10, 0, 0, 1], 1))));
    let send = |path: &'static str, from: &'static str, body: Option<&'static str>| {
        let router = router.clone();
        async move {
            let request = match body {
                Some(body) => Request::post(path)
                    .header("content-type", "application/x-www-form-urlencoded")
                    .header("x-forwarded-for", from)
                    .body(Body::from(body))
                    .unwrap(),
                None => Request::get(path)
                    .header("x-forwarded-for", from)
                    .body(Body::empty())
                    .unwrap(),
            };
            router.oneshot(request).await.unwrap().status()
        }
    };
    for _ in 0..3 {
        send("/dashboard/login", "203.0.113.70", Some("token=wrong")).await;
    }
    // Everyone behind that address, a carrier's NAT say, keeps the relay:
    // one person's typos must not cost them their computers.
    assert_eq!(send("/v1/info", "203.0.113.70", None).await, StatusCode::OK);
    assert!(!hub.is_banned("203.0.113.70".parse().unwrap()));
    let stats = crate::dashboard::snapshot(&hub, Vec::new());
    assert!(stats.bans.is_empty());
    // The dashboard's own lockout still holds.
    let request = Request::post("/dashboard/login")
        .header("content-type", "application/x-www-form-urlencoded")
        .header("x-forwarded-for", "203.0.113.70")
        .body(Body::from(format!("token={TOKEN}")))
        .unwrap();
    let response = router.clone().oneshot(request).await.unwrap();
    let location = response.headers()["location"].to_str().unwrap();
    assert_eq!(location, "/dashboard/login?blocked");
}

#[tokio::test]
async fn a_ban_closes_open_quic_connections() {
    let relay = start_both(&options(Limits::default())).await;
    let connection = connect_with(relay.quic, relay.pin, crate::quic::client_transport())
        .await
        .unwrap();
    let ranges = parse_list("127.0.0.0/8").0;
    relay.hub.guard.set_manual(ranges.clone());
    relay.hub.sweep();
    let closed = tokio::time::timeout(Duration::from_secs(5), connection.closed())
        .await
        .expect("the connection stayed open");
    assert!(
        matches!(&closed, quinn::ConnectionError::ApplicationClosed(close) if close.error_code.into_inner() == u64::from(crate::wire::code::BANNED)),
        "{closed:?}"
    );
}
