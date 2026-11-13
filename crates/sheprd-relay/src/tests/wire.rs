//! The QUIC protocol's binary requests and messages: exact encodings,
//! partial reads, and malformed input.

use super::{public, signing_key};
use crate::{
    RouteId,
    wire::{
        MAX_DIAL_KEYS, MAX_MESSAGE, MAX_PAIRING_SECS, MAX_REQUEST, Malformed, Message, Parse,
        Request, Status,
    },
};

fn listen(token: &[u8]) -> Request {
    Request::Listen {
        key: [2; 33],
        signature: [7; 64],
        token: token.to_vec(),
    }
}

#[test]
fn a_dial_is_its_kind_route_key_and_signature() {
    let route = RouteId::of(signing_key(1).verifying_key());
    let dial = Request::Dial {
        key: public(&signing_key(2)),
        route,
        signature: [9; 64],
    };
    let bytes = dial.encode().unwrap();
    assert_eq!(bytes.len(), 130);
    assert_eq!(bytes[0], 0x02);
    assert_eq!(&bytes[1..33], route.as_bytes());
    assert_eq!(Request::parse(&bytes), Ok(Parse::Done(dial, 130)));
    assert_eq!(Request::parse(&bytes[..40]), Ok(Parse::Need(130)));
}

#[test]
fn a_listen_round_trips_with_and_without_a_token() {
    for token in [&b""[..], b"secret", &[9; 255]] {
        let request = listen(token);
        let bytes = request.encode().unwrap();
        assert_eq!(bytes.len(), 99 + token.len());
        assert_eq!(
            Request::parse(&bytes),
            Ok(Parse::Done(request, bytes.len()))
        );
    }
    assert_eq!(listen(&[0; 255]).encode().unwrap().len(), MAX_REQUEST);
    assert_eq!(listen(&[0; 256]).encode(), Err(Malformed::Token));
}

#[test]
fn partial_bytes_ask_for_exactly_what_is_missing() {
    let bytes = listen(b"abc").encode().unwrap();
    assert_eq!(Request::parse(&[]), Ok(Parse::Need(1)));
    assert_eq!(Request::parse(&bytes[..1]), Ok(Parse::Need(99)));
    assert_eq!(Request::parse(&bytes[..99]), Ok(Parse::Need(102)));
    assert_eq!(Request::parse(&bytes[..101]), Ok(Parse::Need(102)));
    // Bytes after a request are the session's, never part of it.
    let mut longer = bytes.clone();
    longer.extend_from_slice(b"session bytes");
    assert_eq!(
        Request::parse(&longer),
        Ok(Parse::Done(listen(b"abc"), bytes.len()))
    );
}

#[test]
fn dial_keys_round_trip_up_to_their_limit() {
    for count in [0, 1, MAX_DIAL_KEYS] {
        let keys = (0..count).map(|index| [index as u8; 33]).collect();
        let message = Message::DialKeys(keys);
        let bytes = message.encode().unwrap();
        assert_eq!(bytes.len(), 2 + 33 * count);
        assert_eq!(
            Message::parse(&bytes),
            Ok(Parse::Done(message, bytes.len()))
        );
    }
    assert_eq!(Message::parse(&[0x20, 3, 1, 2]), Ok(Parse::Need(2 + 99)));
    let too_many = Message::DialKeys(vec![[0; 33]; MAX_DIAL_KEYS + 1]);
    assert_eq!(too_many.encode(), Err(Malformed::Keys));
    assert_eq!(Message::parse(&[0x20, 65]), Err(Malformed::Keys));
    assert_eq!(
        Message::DialKeys(vec![[0; 33]; MAX_DIAL_KEYS])
            .encode()
            .unwrap()
            .len(),
        MAX_MESSAGE
    );
}

#[test]
fn a_pairing_key_carries_its_lifetime() {
    let message = Message::PairingKey {
        key: [4; 33],
        seconds: 120,
    };
    let bytes = message.encode().unwrap();
    assert_eq!(bytes.len(), 36);
    assert_eq!(&bytes[34..], &120u16.to_be_bytes());
    assert_eq!(Message::parse(&bytes), Ok(Parse::Done(message, 36)));
    for seconds in [0, MAX_PAIRING_SECS + 1] {
        let message = Message::PairingKey {
            key: [4; 33],
            seconds,
        };
        assert_eq!(message.encode(), Err(Malformed::Lifetime));
        let mut bytes = vec![0x21];
        bytes.extend_from_slice(&[4; 33]);
        bytes.extend_from_slice(&seconds.to_be_bytes());
        assert_eq!(Message::parse(&bytes), Err(Malformed::Lifetime));
    }
}

#[test]
fn unknown_kinds_are_refused() {
    for kind in [0x00, 0x03, 0x10, 0xff] {
        assert_eq!(Request::parse(&[kind]), Err(Malformed::Kind(kind)));
    }
    for kind in [0x01, 0x02, 0x22] {
        assert_eq!(Message::parse(&[kind]), Err(Malformed::Kind(kind)));
    }
}

#[test]
fn statuses_round_trip_and_unknown_bytes_are_none() {
    for byte in 0..=9 {
        assert_eq!(
            Status::from_byte(byte).map(|status| status as u8),
            Some(byte)
        );
    }
    assert_eq!(Status::from_byte(10), None);
}
