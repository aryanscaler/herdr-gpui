//! Pools every caller shares, which one caller must not fill: listener and
//! session slots per address and per wide block, sessions per dial key,
//! rate limits per block, a listener's messages, and the memory QUIC peers
//! may make the relay hold.

use super::{quic::*, *};
use crate::{
    bucket::{Limiter, Outcome, Rate},
    guard::Guard,
    hub::{Access, DialRefusal, ListenRefusal},
    limits::WIDE_SHARE,
    metrics::Offense,
};
use std::net::{IpAddr, Ipv6Addr};

fn hub(limits: Limits) -> Arc<Hub> {
    Arc::new(Hub::new(limits, Access::Open))
}

/// The `index`th /64 of 2001:db8:7::/48.
fn in_block(index: u16) -> IpAddr {
    IpAddr::from(Ipv6Addr::from(
        (0x2001_0db8_0007_u128 << 80) | (u128::from(index) << 64) | 1,
    ))
}

fn route(seed: u32) -> RouteId {
    let mut bytes = [7; 32];
    bytes[..4].copy_from_slice(&seed.to_be_bytes());
    let key = SigningKey::from_slice(&bytes).unwrap();
    RouteId::of(key.verifying_key())
}

#[tokio::test]
async fn one_address_or_block_cannot_take_every_listener_slot() {
    let hub = hub(Limits::default());
    let per_ip = Limits::default().max_listeners_per_ip;
    let mut held = Vec::new();
    for seed in 0..u32::try_from(per_ip).unwrap() {
        held.push(hub.register(route(seed), in_block(0)).unwrap());
    }
    assert_eq!(
        hub.register(route(1_000), in_block(0)).err(),
        Some(ListenRefusal::TooMany)
    );
    // Rotating through the block's /64s stops at the block's share.
    let block = per_ip * WIDE_SHARE;
    for index in 1..u16::MAX {
        match hub.register(route(2_000 + u32::from(index)), in_block(index)) {
            Ok(registration) => held.push(registration),
            Err(refusal) => {
                assert_eq!(refusal, ListenRefusal::TooMany);
                break;
            }
        }
    }
    assert_eq!(held.len(), block);
    // Elsewhere, listening goes on; and a slot given back is free again.
    assert!(
        hub.register(route(5_000), "198.51.100.70".parse().unwrap())
            .is_ok()
    );
    held.pop();
    assert!(hub.register(route(5_001), in_block(9_999)).is_ok());
}

#[tokio::test]
async fn a_listener_moving_address_keeps_one_count() {
    let hub = hub(Limits {
        max_listeners_per_ip: 1,
        ..Limits::default()
    });
    let first = hub.register(route(1), in_block(0)).unwrap();
    // The same route again from the same address replaces it in place.
    let second = hub.register(route(1), in_block(0)).unwrap();
    drop(first);
    // From another address: that address's count, and the first's freed.
    let third = hub
        .register(route(1), "198.51.100.71".parse().unwrap())
        .unwrap();
    drop(second);
    assert!(hub.register(route(2), in_block(0)).is_ok());
    drop(third);
}

#[tokio::test]
async fn one_dial_key_or_address_cannot_take_every_session_slot() {
    let hub = hub(Limits::default());
    let listener = hub
        .register(route(1), "198.51.100.72".parse().unwrap())
        .unwrap();
    let thief = signing_key(150);
    listener.set_dial_keys(&[public(&thief), public(&phone())]);
    let per_key = Limits::default().max_sessions_per_key;
    let mut waiting = Vec::new();
    for index in 0..u16::try_from(per_key).unwrap() {
        let dialed = hub.dial(route(1), public(&thief), in_block(index));
        waiting.push(dialed.unwrap().unwrap());
    }
    let refused = hub.dial(route(1), public(&thief), in_block(100)).unwrap();
    assert_eq!(refused.err(), Some(DialRefusal::Busy));
    // The computer's own phone still gets through.
    let phone_dial = hub.dial(route(1), public(&phone()), in_block(101)).unwrap();
    assert!(phone_dial.is_ok());
    // A slot given back is the key's again.
    waiting.pop();
    assert!(
        hub.dial(route(1), public(&thief), in_block(102))
            .unwrap()
            .is_ok()
    );
}

#[tokio::test]
async fn sessions_per_dialing_address_are_capped() {
    let hub = hub(Limits {
        max_sessions_per_ip: 2,
        max_sessions_per_key: 100,
        ..Limits::default()
    });
    let listener = hub
        .register(route(1), "198.51.100.73".parse().unwrap())
        .unwrap();
    listener.set_dial_keys(&[public(&phone())]);
    let dialer = in_block(0);
    let first = hub.dial(route(1), public(&phone()), dialer).unwrap();
    let second = hub.dial(route(1), public(&phone()), dialer).unwrap();
    assert!(first.is_ok() && second.is_ok());
    let third = hub.dial(route(1), public(&phone()), dialer).unwrap();
    assert_eq!(third.err(), Some(DialRefusal::Busy));
}

#[tokio::test]
async fn rate_limits_hold_a_whole_block_back() {
    let hub = hub(Limits::default());
    let per_minute = Limits::default().listens_per_minute;
    let burst = Rate::per_minute(per_minute.saturating_mul(16)).burst;
    let mut allowed = 0;
    for index in 0..u16::MAX {
        if hub.allow_listen(in_block(index)) == Outcome::Allowed {
            allowed += 1;
        } else {
            break;
        }
    }
    assert_eq!(allowed, burst);
    // Another block is not held back.
    assert_eq!(
        hub.allow_listen("198.51.100.74".parse().unwrap()),
        Outcome::Allowed
    );
}

#[test]
fn a_full_limiter_refuses_without_blaming_the_newcomer() {
    let mut limiter = Limiter::bounded(Rate::per_minute(60), 1);
    assert_eq!(limiter.check(1, 0), Outcome::Allowed);
    assert_eq!(limiter.check(2, 0), Outcome::Full);
    for _ in 0..100 {
        limiter.check(1, 0);
    }
    assert_eq!(limiter.check(1, 0), Outcome::Limited);
}

#[test]
fn hitting_limits_never_bans_a_block() {
    let guard = Guard::new(true, 0);
    let now = std::time::Instant::now();
    // Fill the address table with bans, so newcomers fall to their block.
    for index in 0..u32::try_from(crate::guard::MAX_RECORDS).unwrap() {
        let address = IpAddr::from(Ipv6Addr::from(
            (0x2001_0db8_u128 << 96) | (u128::from(index) << 64),
        ));
        while guard.offend(address, Offense::Malformed, now).is_none() {}
    }
    // A busy neighbourhood running into limits bans no one ...
    for last in 1..=200_u8 {
        let busy: IpAddr = format!("192.0.2.{last}").parse().unwrap();
        assert!(guard.offend(busy, Offense::Limited, now).is_none());
    }
    assert!(!guard.banned("192.0.2.250".parse().unwrap(), now));
    // ... while misbehaving still bans its block.
    let mut banned = false;
    for last in 1..=10_u8 {
        let prober: IpAddr = format!("198.18.0.{last}").parse().unwrap();
        banned |= guard.offend(prober, Offense::BadProof, now).is_some();
    }
    assert!(banned);
}

#[tokio::test]
async fn a_listener_may_not_flood_its_messages() {
    let hub = hub(Limits::default());
    let listener = hub.register(route(1), in_block(0)).unwrap();
    let mut allowed = 0;
    while listener.allow_message() {
        allowed += 1;
        assert!(allowed < 100, "never limited");
    }
    assert!(allowed >= 8, "{allowed}");
    let message = crate::wire::Message::DialKeys(vec![public(&phone())]);
    let status = crate::broker::apply(&hub, &listener, message);
    assert_eq!(status, Status::Limited);
}

#[tokio::test]
async fn the_buffer_budget_is_shared_and_given_back() {
    let hub = hub(Limits {
        max_buffer: 1_000,
        ..Limits::default()
    });
    let private: IpAddr = "10.0.0.2".parse().unwrap();
    let first = hub.reserve_buffer(private, 600).unwrap();
    assert!(hub.reserve_buffer(private, 600).is_none());
    let second = hub.reserve_buffer(private, 400).unwrap();
    drop(first);
    assert!(hub.reserve_buffer(private, 600).is_some());
    drop(second);
}

#[tokio::test]
async fn one_address_or_block_takes_only_its_share_of_the_buffer() {
    let hub = hub(Limits {
        max_buffer: 4_000,
        ..Limits::default()
    });
    // An address may hold a quarter, its block half.
    let mine = hub.reserve_buffer(in_block(0), 1_000).unwrap();
    assert!(hub.reserve_buffer(in_block(0), 1).is_none());
    let neighbour = hub.reserve_buffer(in_block(1), 1_000).unwrap();
    assert!(hub.reserve_buffer(in_block(2), 1).is_none());
    // Others still get theirs.
    let elsewhere = hub.reserve_buffer("198.51.100.80".parse().unwrap(), 1_000);
    assert!(elsewhere.is_some());
    drop((mine, neighbour));
    assert!(hub.reserve_buffer(in_block(2), 1_000).is_some());
}

#[tokio::test]
async fn a_relay_out_of_buffer_budget_takes_no_more_connections_or_sessions() {
    // Room for one connection's base windows, and no session.
    let relay = start_both(&options(Limits {
        max_buffer: 2 * 64 * 1024,
        ..Limits::default()
    }))
    .await;
    let computer = signing_key(151);
    let (connection, _send, _recv) = quic_listen(&relay, &computer).await;
    let second = connect_with(relay.quic, relay.pin, crate::quic::client_transport()).await;
    assert!(second.is_err(), "a second connection was taken");
    let route = RouteId::of(computer.verifying_key());
    let (status, _send, _recv) = quic_dial_as(&connection, route, &phone()).await;
    assert_eq!(status, Status::Busy);
}

#[tokio::test]
async fn one_address_held_back_does_not_drain_its_blocks_share() {
    let hub = hub(Limits::default());
    for _ in 0..10_000 {
        hub.allow_listen(in_block(0));
    }
    assert_eq!(hub.allow_listen(in_block(0)), Outcome::Limited);
    assert_eq!(hub.allow_listen(in_block(1)), Outcome::Allowed);
}
