//! Route ids, session ids, and the proof check both transports share.

use super::{public, sign, signing_key};
use crate::{
    RouteId, SessionId,
    route::{Refusal, prove},
};

#[test]
fn a_route_round_trips_and_shows_only_its_start_in_debug() {
    let route = RouteId::of(signing_key(1).verifying_key());
    let text = route.to_string();
    assert_eq!(text.len(), 43);
    assert_eq!(RouteId::parse(&text), Some(route));
    assert!(!format!("{route:?}").contains(&text));
    assert_eq!(route.short().len(), 8);
    assert_eq!(RouteId::parse("short"), None);
    assert_eq!(RouteId::parse(&"!".repeat(43)), None);
    assert_eq!(RouteId::from_bytes(*route.as_bytes()), route);
}

#[test]
fn session_ids_are_fresh_and_parse() {
    let one = SessionId::random().unwrap();
    let two = SessionId::random().unwrap();
    assert_ne!(one, two);
    assert_eq!(SessionId::parse(&one.to_string()), Some(one));
    assert_eq!(SessionId::parse("nope"), None);
}

#[test]
fn a_proof_holds_only_for_its_key_and_message() {
    let key = signing_key(2);
    let signature = sign(&key, b"message");
    assert_eq!(
        prove(&public(&key), &signature, b"message"),
        Ok(RouteId::of(key.verifying_key()))
    );
    assert_eq!(
        prove(&public(&key), &signature, b"other"),
        Err(Refusal::Signature)
    );
    assert_eq!(
        prove(&public(&signing_key(3)), &signature, b"message"),
        Err(Refusal::Signature)
    );
    assert_eq!(prove(&[0; 33], &signature, b"message"), Err(Refusal::Key));
    assert_eq!(
        prove(&public(&key), &[0; 10], b"message"),
        Err(Refusal::Signature)
    );
}
