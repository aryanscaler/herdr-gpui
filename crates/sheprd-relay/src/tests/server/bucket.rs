//! Rate limits and bandwidth throttles.

use crate::bucket::{Limiter, Rate, Throttle, source_key};
use std::{net::IpAddr, time::Duration};

#[test]
fn a_limiter_allows_its_burst_then_refills() {
    let mut limiter = Limiter::new(Rate {
        burst: 2,
        per_minute: 60,
    });
    assert!(limiter.allow("a", 0));
    assert!(limiter.allow("a", 0));
    assert!(!limiter.allow("a", 0));
    assert!(limiter.allow("b", 0));
    assert!(limiter.allow("a", 1_000));
}

#[test]
fn a_full_limiter_refuses_new_keys_rather_than_forgetting_limited_ones() {
    let mut limiter = Limiter::bounded(
        Rate {
            burst: 1,
            per_minute: 1,
        },
        1,
    );
    assert!(limiter.allow(1, 0));
    assert!(!limiter.allow(2, 0));
}

#[test]
fn ipv6_is_limited_by_its_64() {
    let one: IpAddr = "2001:db8::1".parse().unwrap();
    let two: IpAddr = "2001:db8::ffff:1".parse().unwrap();
    assert_eq!(source_key(one), source_key(two));
    let mapped: IpAddr = "::ffff:192.0.2.1".parse().unwrap();
    assert_eq!(source_key(mapped), "192.0.2.1".parse::<IpAddr>().unwrap());
}

#[test]
fn zero_bandwidth_is_unlimited() {
    assert!(Throttle::new(0).is_none());
}

#[tokio::test(start_paused = true)]
async fn a_throttle_lets_one_second_pass_then_charges_time() {
    let throttle = Throttle::new(1000).unwrap();
    assert_eq!(throttle.reserve(1000), Duration::ZERO);
    assert_eq!(throttle.reserve(500), Duration::from_millis(500));
    // Reservations queue: the next 500 wait behind the first.
    assert_eq!(throttle.reserve(500), Duration::from_secs(1));
    tokio::time::advance(Duration::from_secs(1)).await;
    assert_eq!(throttle.reserve(0), Duration::ZERO);
}

#[tokio::test(start_paused = true)]
async fn take_sleeps_off_the_debt() {
    let throttle = Throttle::new(1000).unwrap();
    let started = tokio::time::Instant::now();
    throttle.take(3000).await;
    assert_eq!(started.elapsed(), Duration::from_secs(2));
}
