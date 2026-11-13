//! The second review's findings: bans that race a dial or a listen, exact
//! IPv6 bans, refused dials that must not reveal an occupied route,
//! cross-site logins, and transfers that end between budget checks.

use super::*;
use crate::{
    guard::parse_list,
    hub::{Access, Session},
    metrics::{Gauge, Offense},
    web::same_origin,
};
use axum::http::HeaderMap;
use std::net::IpAddr;

fn ip(text: &str) -> IpAddr {
    text.parse().unwrap()
}

/// Bans `address` on `hub` for malformed messages, a ban that evicts.
fn ban(hub: &Arc<Hub>, address: IpAddr) {
    while !hub.is_banned(address) {
        hub.offend(address, Offense::Malformed);
    }
}

/// A hub whose transfer budget is `budget` bytes an hour.
fn hub(budget: u64) -> Arc<Hub> {
    Arc::new(Hub::new(
        Limits {
            max_transfer_per_hour: budget,
            ..Limits::default()
        },
        Access::Open,
    ))
}

/// A session from `dialer` to a computer at `listener`, on `hub`.
fn session(hub: &Arc<Hub>, listener: IpAddr, dialer: IpAddr, seed: u8) -> Session {
    let route = RouteId::of(signing_key(seed).verifying_key());
    let registration = hub.register(route, listener).unwrap();
    registration.set_dial_keys(&[public(&phone())]);
    let waiting = hub.dial(route, public(&phone()), dialer).unwrap().unwrap();
    let session = hub.open(waiting.slot).unwrap();
    std::mem::forget(registration);
    session
}

#[tokio::test]
async fn a_ban_between_a_dial_and_its_session_is_honoured() {
    let hub = hub(0);
    let route = RouteId::of(signing_key(120).verifying_key());
    let registration = hub.register(route, ip("198.51.100.30")).unwrap();
    registration.set_dial_keys(&[public(&phone())]);
    let toxic = ip("203.0.113.80");
    let waiting = hub.dial(route, public(&phone()), toxic).unwrap().unwrap();
    ban(&hub, toxic);
    assert!(hub.open(waiting.slot).is_none());
    assert_eq!(hub.metrics.read(Gauge::Sessions), 0);
}

#[tokio::test]
async fn a_banned_address_cannot_listen() {
    let hub = hub(0);
    let toxic = ip("203.0.113.81");
    ban(&hub, toxic);
    let route = RouteId::of(signing_key(121).verifying_key());
    assert!(hub.register(route, toxic).is_err());
}

#[tokio::test]
async fn an_exact_ipv6_ban_ends_that_hosts_sessions_and_listener() {
    let hub = hub(0);
    let host = ip("2001:db8::5");
    let session = session(&hub, host, ip("2001:db8:1::9"), 122);
    assert_eq!(hub.metrics.read(Gauge::Listeners), 1);
    let ranges = parse_list("2001:db8::5").0;
    hub.guard.set_manual(ranges.clone());
    hub.sweep();
    tokio::time::timeout(Duration::from_secs(1), session.banned())
        .await
        .expect("the session was not told");
    assert_eq!(hub.metrics.read(Gauge::Listeners), 0);
}

#[tokio::test]
async fn dials_to_an_empty_route_cost_the_same_as_refused_ones_nothing() {
    let empty = hub(0);
    let occupied = hub(0);
    let route = RouteId::of(signing_key(123).verifying_key());
    let registration = occupied.register(route, ip("198.51.100.31")).unwrap();
    registration.set_dial_keys(&[public(&phone())]);
    let stranger = signing_key(124);
    let signed = b"any message the stranger signs".to_vec();
    let signature = sign(&stranger, &signed);
    let caller = ip("203.0.113.82");
    for hub in [&empty, &occupied] {
        for _ in 0..2 * crate::guard::BAN_POINTS {
            let refused =
                crate::broker::dial(hub, caller, route, &public(&stranger), &signature, &signed)
                    .await;
            assert_eq!(refused.err(), Some(Status::Offline));
        }
    }
    // Neither bans, so a ban says nothing about whether anyone listens,
    // and a phone retrying while its computer is off never gets its
    // household banned. The dial rate limit holds a flood back instead.
    assert!(!empty.is_banned(caller));
    assert!(!occupied.is_banned(caller));
}

#[test]
fn only_same_origin_logins_are_counted() {
    let headers = |pairs: &[(&'static str, &'static str)]| {
        let mut headers = HeaderMap::new();
        for (name, value) in pairs {
            headers.insert(*name, value.parse().unwrap());
        }
        headers
    };
    let host = ("host", "relay.example.org");
    assert!(same_origin(&headers(&[host])));
    assert!(same_origin(&headers(&[
        host,
        ("sec-fetch-site", "same-origin")
    ])));
    assert!(same_origin(&headers(&[
        host,
        ("origin", "https://relay.example.org")
    ])));
    assert!(!same_origin(&headers(&[
        host,
        ("sec-fetch-site", "cross-site")
    ])));
    assert!(!same_origin(&headers(&[
        host,
        ("sec-fetch-site", "same-site")
    ])));
    assert!(!same_origin(&headers(&[
        host,
        ("origin", "https://evil.example")
    ])));
    assert!(!same_origin(&headers(&[host, ("origin", "null")])));
}

#[tokio::test]
async fn a_short_transfer_is_charged_when_it_ends() {
    let hub = hub(1_000);
    let dialer = ip("203.0.113.83");
    let session = session(&hub, ip("198.51.100.32"), dialer, 125);
    // A transfer that ends before the watchdog's next turn.
    session
        .traffic
        .from_dialer
        .fetch_add(2_000, std::sync::atomic::Ordering::Relaxed);
    crate::splice::settle(&session, &crate::splice::Ledger::default());
    assert!(hub.over_budget(dialer));
    // Charged to the side that sent it.
    assert!(!hub.over_budget(ip("198.51.100.32")));
}

#[tokio::test]
async fn a_wide_ban_ends_the_whole_blocks_sessions_and_listeners() {
    let hub = hub(0);
    // Fill the address table with bans, so the next ban is of a block.
    let now = std::time::Instant::now();
    for index in 0..u32::try_from(crate::guard::MAX_RECORDS).unwrap() {
        let address = IpAddr::from(std::net::Ipv6Addr::from(
            (0x2001_0db8_u128 << 96) | (u128::from(index) << 64),
        ));
        while hub.guard.offend(address, Offense::Malformed, now).is_none() {}
    }
    // A computer in 203.0.113.0/24, and a session to it.
    let neighbour = session(&hub, ip("203.0.113.10"), ip("198.51.100.40"), 126);
    assert_eq!(hub.metrics.read(Gauge::Listeners), 1);
    // Another address in that /24 earns a ban, which covers the block.
    ban(&hub, ip("203.0.113.99"));
    assert!(hub.is_banned(ip("203.0.113.10")));
    tokio::time::timeout(Duration::from_secs(1), neighbour.banned())
        .await
        .expect("the neighbour's session was not ended");
    assert_eq!(hub.metrics.read(Gauge::Listeners), 0);
}
