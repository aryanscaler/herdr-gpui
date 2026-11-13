//! The three races a review found: a listener message cut off by a dial,
//! a key revoked while its dial waits, and forged source addresses.

use super::{quic::*, *};
const LOCAL: std::net::IpAddr = std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST);

use crate::{
    hub::Access,
    metrics::Gauge,
    quic::Messages,
    wire::{MAX_DIAL_KEYS, Message},
};

#[test]
fn a_message_cut_anywhere_is_finished_on_the_next_chunk() {
    let first = Message::DialKeys(vec![public(&phone()), public(&signing_key(90))]);
    let second = Message::PairingKey {
        key: public(&signing_key(91)),
        seconds: 60,
    };
    let bytes = [first.encode().unwrap(), second.encode().unwrap()].concat();
    for cut in 0..=bytes.len() {
        let mut messages = Messages::default();
        let mut taken = messages.take(&bytes[..cut]).unwrap();
        taken.extend(messages.take(&bytes[cut..]).unwrap());
        assert_eq!(taken, vec![first.clone(), second.clone()], "cut at {cut}");
    }
    // One byte at a time, too.
    let mut messages = Messages::default();
    let taken: Vec<_> = bytes
        .iter()
        .flat_map(|byte| messages.take(&[*byte]).unwrap())
        .collect();
    assert_eq!(taken, vec![first, second]);
}

#[test]
fn bytes_that_are_not_messages_stop_the_listener() {
    assert!(Messages::default().take(&[0x7f]).is_none());
    assert!(
        Messages::default()
            .take(&[0x20, (MAX_DIAL_KEYS + 1) as u8])
            .is_none()
    );
}

#[tokio::test]
async fn a_message_sent_in_pieces_while_dials_arrive_still_applies() {
    let relay = start_both(&options(Limits::default())).await;
    let key = signing_key(92);
    let route = RouteId::of(key.verifying_key());
    let (listener, mut send, mut recv) = quic_listen(&relay, &key).await;
    let tablet = signing_key(93);
    let bytes = Message::DialKeys(vec![public(&phone()), public(&tablet)])
        .encode()
        .unwrap();
    let (head, tail) = bytes.split_at(20);
    send.write_all(head).await.unwrap();
    // A dial lands while the message is half sent.
    let phone_connection = connect_to(&relay).await;
    let dial = tokio::spawn({
        let phone_connection = phone_connection.clone();
        async move { quic_dial_as(&phone_connection, route, &phone()).await }
    });
    let (_to, _from, _) = quic_accept_from(&listener).await;
    assert_eq!(dial.await.unwrap().0, Status::Ok);
    send.write_all(tail).await.unwrap();
    let mut answer = [0; 1];
    recv.read_exact(&mut answer).await.unwrap();
    assert_eq!(Status::from_byte(answer[0]), Some(Status::Ok));
    // The whole message applied: the tablet may dial now.
    let dial = tokio::spawn({
        let phone_connection = phone_connection.clone();
        async move { quic_dial_as(&phone_connection, route, &tablet).await }
    });
    let (_to, _from, dialer) = quic_accept_from(&listener).await;
    assert_eq!(dialer, public(&signing_key(93)));
    assert_eq!(dial.await.unwrap().0, Status::Ok);
}

#[tokio::test]
async fn a_key_revoked_while_its_dial_waits_never_opens_a_session() {
    let hub = Arc::new(Hub::new(Limits::default(), Access::Open));
    let route = RouteId::of(signing_key(94).verifying_key());
    let registration = hub.register(route, LOCAL).unwrap();
    registration.set_dial_keys(&[public(&phone())]);
    let waiting = hub.dial(route, public(&phone()), LOCAL).unwrap().unwrap();
    // Revoked after the dial's check, before its session opens.
    registration.set_dial_keys(&[]);
    assert!(hub.open(waiting.slot).is_none());
    assert_eq!(hub.metrics.read(Gauge::Sessions), 0);
}

#[tokio::test]
async fn a_pairing_slot_opens_after_its_key_is_spent() {
    let hub = Arc::new(Hub::new(Limits::default(), Access::Open));
    let route = RouteId::of(signing_key(95).verifying_key());
    let registration = hub.register(route, LOCAL).unwrap();
    let pairing = public(&signing_key(96));
    assert!(registration.add_pairing_key(pairing, Duration::from_secs(60)));
    let waiting = hub.dial(route, pairing, LOCAL).unwrap().unwrap();
    assert!(hub.open(waiting.slot).is_some());
}

#[tokio::test]
async fn new_quic_connections_prove_their_address_first() {
    // The client completes the stateless retry on its own; a connection
    // that could not would never be counted.
    let relay = start_both(&options(Limits::default())).await;
    let _connection = connect_to(&relay).await;
    assert_eq!(
        relay
            .hub
            .metrics
            .read(Gauge::Connections(crate::metrics::Kind::Quic)),
        1
    );
}
