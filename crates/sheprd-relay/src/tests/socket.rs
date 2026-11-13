//! The WebSocket fallback's messages: proofs bound to host, nonce, and
//! route, and listener messages checked against the same bounds as QUIC.

use super::{hello, public, signing_key};
use crate::{
    RouteId,
    route::prove,
    socket::{FromListener, ToPeer, dial_signed, listen_signed},
    wire::{MAX_DIAL_KEYS, MAX_PAIRING_SECS, Message, Status},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};

#[test]
fn proofs_bind_host_nonce_and_route() {
    let key = signing_key(4);
    let route = RouteId::of(signing_key(5).verifying_key());
    let signed = dial_signed("relay.example:443", &[7; 32], &route);
    let (public_key, signature) = hello(&key, &signed).decode().unwrap();
    assert!(prove(&public_key, &signature, &signed).is_ok());
    let other_route = RouteId::of(signing_key(6).verifying_key());
    for other in [
        dial_signed("other.example:443", &[7; 32], &route),
        dial_signed("relay.example:443", &[8; 32], &route),
        dial_signed("relay.example:443", &[7; 32], &other_route),
        listen_signed("relay.example:443", &[7; 32]),
    ] {
        assert!(prove(&public_key, &signature, &other).is_err());
    }
}

#[test]
fn listener_messages_decode_within_bounds() {
    let key = URL_SAFE_NO_PAD.encode(public(&signing_key(9)));
    let keys = FromListener::DialKeys {
        keys: vec![key.clone()],
    };
    assert_eq!(
        keys.decode(),
        Some(Message::DialKeys(vec![public(&signing_key(9))]))
    );
    let too_many = FromListener::DialKeys {
        keys: vec![key.clone(); MAX_DIAL_KEYS + 1],
    };
    assert_eq!(too_many.decode(), None);
    let bad_key = FromListener::DialKeys {
        keys: vec!["AAAA".into()],
    };
    assert_eq!(bad_key.decode(), None);
    for seconds in [0, MAX_PAIRING_SECS + 1] {
        let pairing = FromListener::PairingKey {
            key: key.clone(),
            seconds,
        };
        assert_eq!(pairing.decode(), None);
    }
}

#[test]
fn messages_read_as_documented() {
    let status: ToPeer = serde_json::from_str(r#"{"t":"status","status":"offline"}"#).unwrap();
    assert_eq!(
        status,
        ToPeer::Status {
            status: Status::Offline
        }
    );
    let keys: FromListener = serde_json::from_str(r#"{"t":"dial_keys","keys":[]}"#).unwrap();
    assert_eq!(keys, FromListener::DialKeys { keys: Vec::new() });
}
