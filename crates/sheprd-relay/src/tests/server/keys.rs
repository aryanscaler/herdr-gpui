//! Who may dial: only keys the listener names, a pairing key once, and a
//! revoked key's sessions end at once. A stranger hears `offline` and never
//! reaches the computer, over either transport.

use super::{
    quic::{quic_accept_from, quic_dial_as, quic_listen, quic_message},
    *,
};
use crate::{
    hub::MAX_PAIRING_KEYS,
    metrics::Gauge,
    splice::code,
    wire::{self, Message},
};

/// Whether `listener` hears of any dial within a short wait.
async fn hears_a_dial(listener: &mut Socket) -> bool {
    tokio::time::timeout(Duration::from_millis(300), next(listener))
        .await
        .is_ok_and(|frame| matches!(frame, Some(Frame::Text(_))))
}

#[tokio::test]
async fn a_stranger_hears_offline_and_never_reaches_the_computer() {
    let (address, hub) = start(&options(Limits::default())).await;
    let (mut listener, route) = listen(address, &signing_key(30), None).await;
    let (mut socket, status) = dial_as(address, &route, &signing_key(31)).await;
    assert_eq!(status, Status::Offline);
    assert_eq!(close_code(&mut socket).await, Some(code::OFFLINE));
    assert!(!hears_a_dial(&mut listener).await);
    assert!(
        hub.metrics
            .render()
            .contains("sheprd_relay_dials_total{outcome=\"unauthorized\"} 1")
    );
    // The allowed device still gets through.
    let (_dialer, _accepted) = session(address, &mut listener, &route).await;
}

#[tokio::test]
async fn no_one_may_dial_before_the_listener_names_keys() {
    let (address, _hub) = start(&options(Limits::default())).await;
    let (mut listener, route) = listen_only(address, &signing_key(32), None).await;
    let (_socket, status) = dial_as(address, &route, &phone()).await;
    assert_eq!(status, Status::Offline);
    assert!(!hears_a_dial(&mut listener).await);
}

#[tokio::test]
async fn revoking_a_key_ends_its_sessions_at_once() {
    let (address, hub) = start(&options(Limits::default())).await;
    let tablet = signing_key(33);
    let (mut listener, route) = listen_only(address, &signing_key(34), None).await;
    let both = [public(&phone()), public(&tablet)];
    assert_eq!(allow(&mut listener, &both).await, Status::Ok);
    let (mut phone_side, mut phone_computer) = session(address, &mut listener, &route).await;
    let (mut tablet_side, _tablet_computer) =
        session_as(address, &mut listener, &route, &tablet).await;

    assert_eq!(allow(&mut listener, &[public(&tablet)]).await, Status::Ok);
    assert_eq!(close_code(&mut phone_side).await, Some(code::REVOKED));
    assert_eq!(close_code(&mut phone_computer).await, Some(code::REVOKED));
    // The tablet's session is untouched.
    tablet_side
        .send(Frame::Binary(b"still here".to_vec().into()))
        .await
        .unwrap();
    let (_socket, status) = dial_as(address, &route, &phone()).await;
    assert_eq!(status, Status::Offline);
    assert!(
        hub.metrics
            .render()
            .contains("sheprd_relay_sessions_ended_total{reason=\"revoked\"} 1")
    );
}

#[tokio::test]
async fn a_pairing_key_works_once() {
    let (address, hub) = start(&options(Limits::default())).await;
    let pairing = signing_key(35);
    let (mut listener, route) = listen(address, &signing_key(36), None).await;
    assert_eq!(
        allow_pairing(&mut listener, public(&pairing), 60).await,
        Status::Ok
    );
    let (mut new_phone, mut computer) = session_as(address, &mut listener, &route, &pairing).await;
    assert!(hub.sessions().iter().any(|row| row.pairing));

    // Approving the new device names its own key; the pairing session,
    // still open, carries on.
    let new_key = signing_key(37);
    let keys = [public(&phone()), public(&new_key)];
    assert_eq!(allow(&mut listener, &keys).await, Status::Ok);
    new_phone
        .send(Frame::Binary(b"approved".to_vec().into()))
        .await
        .unwrap();
    assert_eq!(
        next(&mut computer).await,
        Some(Frame::Binary(b"approved".to_vec().into()))
    );

    // The pairing key is spent.
    let (_socket, status) = dial_as(address, &route, &pairing).await;
    assert_eq!(status, Status::Offline);
    let (_new_side, _new_computer) = session_as(address, &mut listener, &route, &new_key).await;
}

#[tokio::test]
async fn a_pairing_key_expires() {
    let (address, _hub) = start(&options(Limits::default())).await;
    let pairing = signing_key(38);
    let (mut listener, route) = listen(address, &signing_key(39), None).await;
    assert_eq!(
        allow_pairing(&mut listener, public(&pairing), 1).await,
        Status::Ok
    );
    tokio::time::sleep(Duration::from_millis(1_100)).await;
    let (_socket, status) = dial_as(address, &route, &pairing).await;
    assert_eq!(status, Status::Offline);
}

#[tokio::test]
async fn pairing_keys_are_few() {
    let (address, _hub) = start(&options(Limits::default())).await;
    let (mut listener, _route) = listen(address, &signing_key(40), None).await;
    for seed in 0..MAX_PAIRING_KEYS {
        let key = public(&signing_key(41 + seed as u8));
        assert_eq!(allow_pairing(&mut listener, key, 60).await, Status::Ok);
    }
    let one_more = public(&signing_key(60));
    assert_eq!(
        allow_pairing(&mut listener, one_more, 60).await,
        Status::TooMany
    );
}

#[tokio::test]
async fn quic_listeners_name_keys_and_revoke_them() {
    let relay = start_both(&options(Limits::default())).await;
    let key = signing_key(61);
    let route = RouteId::of(key.verifying_key());
    let (listener, mut listen_send, mut listen_recv) = quic_listen(&relay, &key).await;
    let phone_connection = quic::connect_to(&relay).await;

    let stranger = quic_dial_as(&phone_connection, route, &signing_key(62)).await;
    assert_eq!(stranger.0, Status::Offline);

    let dial = tokio::spawn({
        let phone_connection = phone_connection.clone();
        async move { quic_dial_as(&phone_connection, route, &phone()).await }
    });
    let (_to_phone, _from_phone, dialer_key) = quic_accept_from(&listener).await;
    assert_eq!(dialer_key, public(&phone()));
    let (status, _send, mut recv) = dial.await.unwrap();
    assert_eq!(status, Status::Ok);
    assert_eq!(relay.hub.metrics.read(Gauge::Sessions), 1);

    let revoke = Message::DialKeys(Vec::new());
    assert_eq!(
        quic_message(&mut listen_send, &mut listen_recv, revoke).await,
        Status::Ok
    );
    let mut byte = [0; 1];
    let read = tokio::time::timeout(Duration::from_secs(5), recv.read(&mut byte))
        .await
        .unwrap();
    assert!(
        matches!(read, Err(quinn::ReadError::Reset(code)) if code.into_inner() == u64::from(wire::code::REVOKED))
    );
}

#[tokio::test]
async fn a_malformed_listener_message_stops_listening() {
    let relay = start_both(&options(Limits::default())).await;
    let (_connection, mut send, _recv) = quic_listen(&relay, &signing_key(63)).await;
    send.write_all(&[0x7f]).await.unwrap();
    for _ in 0..200 {
        if relay.hub.metrics.read(Gauge::Listeners) == 0 {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("still listening");
}

#[tokio::test]
async fn a_listeners_keys_go_when_it_does() {
    let (address, hub) = start(&options(Limits::default())).await;
    let key = signing_key(64);
    let (listener, route) = listen(address, &key, None).await;
    drop(listener);
    for _ in 0..200 {
        if hub.metrics.read(Gauge::Listeners) == 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(hub.metrics.read(Gauge::Listeners), 0);
    // Back again, the computer starts with no keys until it names them.
    let (mut listener, again) = listen_only(address, &key, None).await;
    assert_eq!(route, again);
    let (_socket, status) = dial_as(address, &route, &phone()).await;
    assert_eq!(status, Status::Offline);
    assert!(!hears_a_dial(&mut listener).await);
}

#[tokio::test]
async fn a_silent_websocket_listener_is_dropped_with_its_keys() {
    let limits = Limits {
        listener_silence: Duration::from_millis(600),
        ..Limits::default()
    };
    let (address, hub) = start(&options(limits)).await;
    // Never read again, so it answers no pings: as a computer that lost
    // its network without closing the connection.
    let (_listener, _route) = listen(address, &signing_key(65), None).await;
    assert_eq!(hub.metrics.read(Gauge::Listeners), 1);
    for _ in 0..300 {
        if hub.metrics.read(Gauge::Listeners) == 0 {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("a silent listener stayed");
}
