//! Requests a caller does not choose: sockets a web page opens from its
//! visitors' browsers, and addresses a proxy in front reports.

use super::*;
use crate::{
    metrics::Gauge,
    web::{Shared, Source},
};
use axum::http::{HeaderMap, HeaderValue};
use std::net::IpAddr;

/// Opens `path` as a browser on `origin` would, with `Sec-Fetch-Site` when
/// given.
async fn connect_from(
    address: SocketAddr,
    path: &str,
    origin: &str,
    site: Option<&str>,
) -> Result<Socket, tungstenite::Error> {
    let mut request = format!("ws://{address}{path}")
        .into_client_request()
        .unwrap();
    request
        .headers_mut()
        .insert("origin", origin.parse().unwrap());
    if let Some(site) = site {
        request
            .headers_mut()
            .insert("sec-fetch-site", site.parse().unwrap());
    }
    let stream = TcpStream::connect(address).await.unwrap();
    client_async(request, stream)
        .await
        .map(|(socket, _)| socket)
}

fn forbidden(result: Result<Socket, tungstenite::Error>) -> bool {
    matches!(result, Err(tungstenite::Error::Http(response)) if response.status() == 403)
}

#[tokio::test]
async fn a_page_on_another_site_cannot_open_the_sockets() {
    let (address, hub) = start(&options(Limits::default())).await;
    let session = crate::SessionId::random().unwrap();
    for _ in 0..20 {
        for path in [
            "/v1/listen".to_owned(),
            // Neither id parses: without the check each would be a probe.
            "/v1/dial/x".to_owned(),
            "/v1/accept/x".to_owned(),
            format!("/v1/accept/{session}"),
        ] {
            let page = connect_from(address, &path, "https://evil.example", None).await;
            assert!(forbidden(page), "{path}");
            let fetch = connect_from(address, &path, "https://evil.example", Some("cross-site"));
            assert!(forbidden(fetch.await), "{path}");
        }
    }
    // Refused before anything was counted or held.
    let text = hub.metrics.render();
    for offense in ["probe", "malformed", "limited", "connection_cap"] {
        let line = format!("sheprd_relay_strikes_total{{offense=\"{offense}\"}} 0");
        assert!(text.contains(&line), "{text}");
    }
    assert_eq!(hub.metrics.read(Gauge::Listeners), 0);
}

#[tokio::test]
async fn clients_without_a_page_and_the_relays_own_origin_still_connect() {
    let (address, _hub) = start(&options(Limits::default())).await;
    let mut plain = connect(address, "/v1/listen").await.unwrap();
    challenge(&mut plain).await;
    let own = format!("http://{address}");
    let mut same = connect_from(address, "/v1/listen", &own, None)
        .await
        .unwrap();
    challenge(&mut same).await;
}

fn shared(source: Source) -> Shared {
    Shared {
        hub: Arc::new(Hub::new(Limits::default(), crate::hub::Access::Open)),
        public_host: None,
        quic: None,
        source,
    }
}

fn ip(text: &str) -> IpAddr {
    text.parse().unwrap()
}

#[test]
fn the_caller_is_the_last_entry_of_the_last_forwarded_line() {
    let proxy = shared(Source::Proxy);
    let peer = SocketAddr::from(([10, 0, 0, 1], 443));
    let mut headers = HeaderMap::new();
    // A proxy that appends to the caller's line.
    headers.insert(
        "x-forwarded-for",
        HeaderValue::from_static("192.0.2.66, 203.0.113.5"),
    );
    assert_eq!(proxy.address(peer, &headers), ip("203.0.113.5"));
    // A proxy that adds its own line after the caller's spoofed one.
    headers.append("x-forwarded-for", HeaderValue::from_static("203.0.113.6"));
    assert_eq!(proxy.address(peer, &headers), ip("203.0.113.6"));
    // An unreadable last line falls back to the peer, never to the
    // caller's line.
    headers.append(
        "x-forwarded-for",
        HeaderValue::from_static("not an address"),
    );
    assert_eq!(proxy.address(peer, &headers), peer.ip());
    assert_eq!(proxy.address(peer, &HeaderMap::new()), peer.ip());
}
