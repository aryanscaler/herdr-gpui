//! Relay addresses: what is accepted, how it is normalized, and the URLs
//! built from it.

use super::signing_key;
use crate::{
    RouteId, SessionId,
    endpoint::{Endpoint, Error},
};

fn parse(text: &str) -> Endpoint {
    Endpoint::parse(text).unwrap()
}

#[test]
fn a_bare_host_means_https() {
    let endpoint = parse("relay.example.org");
    assert_eq!(endpoint.to_string(), "https://relay.example.org");
    assert!(endpoint.is_secure());
    assert_eq!(endpoint.host(), "relay.example.org");
    assert_eq!(
        endpoint.listen().as_str(),
        "wss://relay.example.org/v1/listen"
    );
    assert_eq!(
        endpoint.info().as_str(),
        "https://relay.example.org/v1/info"
    );
}

#[test]
fn ports_prefixes_and_wss_are_kept() {
    let endpoint = parse("wss://Relay.Example.org:8443/sheprd/");
    assert_eq!(
        endpoint.to_string(),
        "https://relay.example.org:8443/sheprd"
    );
    assert_eq!(endpoint.host(), "relay.example.org:8443");
    let route = RouteId::of(signing_key(1).verifying_key());
    assert_eq!(
        endpoint.dial(&route).as_str(),
        format!("wss://relay.example.org:8443/sheprd/v1/dial/{route}")
    );
    let session = SessionId::random().unwrap();
    assert_eq!(
        endpoint.accept(&session).as_str(),
        format!("wss://relay.example.org:8443/sheprd/v1/accept/{session}")
    );
    // The default port is dropped, as a WebSocket client's Host header does.
    assert_eq!(
        parse("https://relay.example.org:443").host(),
        "relay.example.org"
    );
}

#[test]
fn plain_http_is_only_for_local_relays() {
    for local in [
        "http://localhost:8080",
        "http://127.0.0.1:8080",
        "ws://192.168.1.20:8080",
        "http://10.0.0.2",
        "http://[::1]:8080",
        "http://[fd00::2]:8080",
        "http://relay.local",
    ] {
        let endpoint = parse(local);
        assert!(!endpoint.is_secure(), "{local}");
        assert!(endpoint.listen().as_str().starts_with("ws://"), "{local}");
    }
    assert_eq!(parse("http://[::1]:8080").host(), "[::1]:8080");
    for public in [
        "http://relay.example.org",
        "ws://203.0.113.7:8080",
        "http://8.8.8.8",
    ] {
        assert!(
            matches!(Endpoint::parse(public), Err(Error::Insecure)),
            "{public}"
        );
    }
}

#[test]
fn other_shapes_are_refused_with_a_reason() {
    assert!(matches!(
        Endpoint::parse("ftp://relay.example.org"),
        Err(Error::Scheme { .. })
    ));
    assert!(matches!(
        Endpoint::parse("https://user:pass@relay.example.org"),
        Err(Error::Credentials)
    ));
    assert!(matches!(
        Endpoint::parse("https://relay.example.org/?token=x"),
        Err(Error::Extra)
    ));
    assert!(matches!(
        Endpoint::parse("https://relay.example.org/#x"),
        Err(Error::Extra)
    ));
    assert!(matches!(Endpoint::parse(""), Err(Error::Url(_))));
    assert!(matches!(Endpoint::parse("https://"), Err(Error::Url(_))));
}

#[test]
fn it_round_trips_as_a_string() {
    let endpoint = parse("relay.example.org:8443");
    let json = format!("\"{endpoint}\"");
    let back: Endpoint = serde_json::from_str(&json).unwrap();
    assert_eq!(back, endpoint);
    assert!(serde_json::from_str::<Endpoint>("\"http://relay.example.org\"").is_err());
}

#[test]
fn errors_nest_under_the_crate_error() {
    let error: crate::Error = Endpoint::parse("http://relay.example.org")
        .unwrap_err()
        .into();
    assert!(matches!(error, crate::Error::Endpoint(Error::Insecure)));
}
