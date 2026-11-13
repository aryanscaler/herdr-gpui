//! The dashboard lockout: each try is reserved before its token is
//! checked, three block an address for a while, a block runs out, and
//! signing in clears the count.

use crate::dashboard::lockout::{BLOCK, Lockout, TRIES, Verdict};
use std::{
    net::IpAddr,
    time::{Duration, Instant},
};

fn address(last: u8) -> IpAddr {
    IpAddr::from([192, 0, 2, last])
}

#[test]
fn three_tries_are_checked_then_the_address_is_blocked() {
    let lockout = Lockout::default();
    let now = Instant::now();
    for _ in 0..TRIES {
        assert_eq!(lockout.attempt(address(1), now), Verdict::Allowed);
    }
    // The third try was checked; it left the address blocked.
    assert!(lockout.exhausted(address(1), now));
    assert_eq!(lockout.attempt(address(1), now), Verdict::Blocked(BLOCK));
    let later = now + Duration::from_secs(60);
    assert_eq!(
        lockout.check(address(1), later),
        Verdict::Blocked(BLOCK - Duration::from_secs(60))
    );
    // Others are untouched.
    assert_eq!(lockout.attempt(address(2), now), Verdict::Allowed);
}

#[test]
fn tries_sent_at_once_get_three_guesses_between_them() {
    let lockout = std::sync::Arc::new(Lockout::default());
    let now = Instant::now();
    let checked: usize = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..64)
            .map(|_| {
                let lockout = std::sync::Arc::clone(&lockout);
                scope.spawn(move || lockout.attempt(address(3), now) == Verdict::Allowed)
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| usize::from(handle.join().unwrap()))
            .sum()
    });
    assert_eq!(checked, TRIES as usize);
}

#[test]
fn a_block_runs_out_and_starts_the_count_again() {
    let lockout = Lockout::default();
    let now = Instant::now();
    for _ in 0..TRIES {
        lockout.attempt(address(4), now);
    }
    let after = now + BLOCK;
    assert_eq!(lockout.check(address(4), after), Verdict::Allowed);
    assert_eq!(lockout.attempt(address(4), after), Verdict::Allowed);
    assert!(!lockout.exhausted(address(4), after));
}

#[test]
fn old_wrong_tries_are_forgotten() {
    let lockout = Lockout::default();
    let now = Instant::now();
    for _ in 1..TRIES {
        lockout.attempt(address(5), now);
    }
    // Long after, one more try starts a fresh count.
    lockout.attempt(address(5), now + BLOCK);
    assert!(!lockout.exhausted(address(5), now + BLOCK));
}

#[test]
fn signing_in_clears_the_count() {
    let lockout = Lockout::default();
    let now = Instant::now();
    for _ in 1..TRIES {
        lockout.attempt(address(6), now);
    }
    // The last allowed try is the right token.
    assert_eq!(lockout.attempt(address(6), now), Verdict::Allowed);
    lockout.succeed(address(6));
    for _ in 0..TRIES {
        assert_eq!(lockout.attempt(address(6), now), Verdict::Allowed);
    }
}

#[test]
fn a_full_table_makes_room_and_never_allows_unlimited_guesses() {
    use crate::dashboard::lockout::{MAX_ADDRESSES, OVERFLOW};
    let now = Instant::now();
    let guesser = |index: u32| {
        IpAddr::from(std::net::Ipv6Addr::from(
            (0x2001_0db8_u128 << 96) | (u128::from(index) << 64),
        ))
    };
    let slots = u32::try_from(MAX_ADDRESSES).unwrap();
    // Full of addresses with one wrong try: the oldest makes room.
    let lockout = Lockout::default();
    for index in 0..slots {
        lockout.attempt(guesser(index), now);
    }
    assert_eq!(
        lockout.attempt("198.51.100.94".parse().unwrap(), now),
        Verdict::Allowed
    );
    // Full of blocks: newcomers are checked, but share a few tries.
    let blocked = Lockout::default();
    for index in 0..slots {
        for _ in 0..TRIES {
            blocked.attempt(guesser(index), now);
        }
    }
    let newcomer = |last: u8| IpAddr::from([198, 51, 100, last]);
    let checked = (0..50_u8)
        .filter(|last| blocked.attempt(newcomer(*last), now) == Verdict::Untracked)
        .count();
    assert_eq!(checked, usize::try_from(OVERFLOW.burst).unwrap());
    assert!(matches!(
        blocked.attempt(newcomer(200), now),
        Verdict::Blocked(_)
    ));
    // Another block keeps its own share: draining one locks no one else out.
    assert_eq!(
        blocked.attempt(IpAddr::from([203, 0, 113, 5]), now),
        Verdict::Untracked
    );
}
