//! Bans: points that fade, escalating bans, the transfer budget, manual
//! ranges, and the addresses never banned automatically.

use crate::{
    guard::{BAN_POINTS, BANS, Cidr, Guard, exempt, parse_list, points},
    metrics::Offense,
};
use std::{
    net::IpAddr,
    time::{Duration, Instant},
};

const PUBLIC: &str = "203.0.113.50";
const GIB: u64 = 1024 * 1024 * 1024;

fn ip(text: &str) -> IpAddr {
    text.parse().unwrap()
}

fn guard() -> Guard {
    Guard::new(true, 20 * GIB)
}

/// Offends until banned; how many offenses that took.
fn offend_until_banned(guard: &Guard, address: IpAddr, offense: Offense, now: Instant) -> u32 {
    for count in 1..=100 {
        if guard.offend(address, offense, now).is_some() {
            return count;
        }
    }
    panic!("never banned");
}

#[test]
fn points_add_up_to_a_ban() {
    let guard = guard();
    let now = Instant::now();
    let address = ip(PUBLIC);
    let needed = offend_until_banned(&guard, address, Offense::Malformed, now);
    assert_eq!(needed, BAN_POINTS.div_ceil(points(Offense::Malformed)));
    assert!(guard.banned(address, now));
    assert!(guard.banned(address, now + BANS[0] - Duration::from_secs(1)));
    assert!(!guard.banned(address, now + BANS[0]));
    // Overrunning the transfer budget many times bans at once.
    assert!(guard.charge(ip("203.0.113.51"), 81 * GIB, now).is_some());
}

#[test]
fn points_fade_a_point_a_minute() {
    let guard = guard();
    let start = Instant::now();
    let address = ip(PUBLIC);
    // One a minute never adds up: a phone retrying too fast, say.
    for minute in 0..120 {
        let now = start + Duration::from_secs(60 * minute);
        assert!(guard.offend(address, Offense::Limited, now).is_none());
    }
    // A burst does.
    let now = start + Duration::from_secs(60 * 120);
    assert!(offend_until_banned(&guard, address, Offense::Limited, now) <= BAN_POINTS);
}

#[test]
fn bans_grow_then_are_forgiven() {
    let guard = guard();
    let address = ip(PUBLIC);
    let mut now = Instant::now();
    for length in BANS.iter().chain([BANS[2]].iter()) {
        offend_until_banned(&guard, address, Offense::Malformed, now);
        assert!(guard.banned(address, now + *length - Duration::from_secs(1)));
        assert!(!guard.banned(address, now + *length));
        now += *length;
    }
    // A clean day after the last ban: back to the shortest.
    now += Duration::from_secs(24 * 60 * 60);
    offend_until_banned(&guard, address, Offense::Malformed, now);
    assert!(!guard.banned(address, now + BANS[0]));
}

#[test]
fn the_transfer_budget_slows_when_spent_and_bans_when_overrun() {
    use crate::guard::OVERRUN;
    let guard = Guard::new(true, 10 * GIB);
    let now = Instant::now();
    let address = ip(PUBLIC);
    assert!(guard.charge(address, 10 * GIB, now).is_none());
    assert!(!guard.over_budget(address, now));
    // Over budget: slowed, not banned.
    assert!(guard.charge(address, 1, now).is_none());
    assert!(guard.over_budget(address, now));
    assert!(!guard.banned(address, now));
    // Moving many budgets anyway is a ban, and one that ends its work.
    assert!(guard.charge(address, OVERRUN * 10 * GIB, now).is_some());
    assert!(guard.banned(address, now));
    assert!(guard.evicts(address, now));
    // The budget is per hour.
    let other = ip("203.0.113.52");
    guard.charge(other, 11 * GIB, now);
    assert!(guard.over_budget(other, now));
    let later = now + Duration::from_secs(3600);
    assert!(!guard.over_budget(other, later));
    assert!(guard.charge(other, 9 * GIB, later).is_none());
    assert!(!guard.over_budget(other, later));
    // No budget, no ban.
    let unlimited = Guard::new(true, 0);
    assert!(unlimited.charge(address, u64::MAX, now).is_none());
}

#[test]
fn private_addresses_and_auto_off_are_never_banned_automatically() {
    let guard = guard();
    let now = Instant::now();
    for private in [
        "127.0.0.1",
        "10.0.0.1",
        "192.168.1.20",
        "172.17.0.2",
        "::1",
        "fd00::1",
        "fe80::1",
    ] {
        let address = ip(private);
        assert!(exempt(address), "{private}");
        for _ in 0..50 {
            assert!(
                guard.offend(address, Offense::Malformed, now).is_none(),
                "{private}"
            );
        }
        guard.charge(address, 100 * GIB, now);
        assert!(!guard.banned(address, now), "{private}");
        assert!(!guard.over_budget(address, now), "{private}");
    }
    assert!(!exempt(ip(PUBLIC)));
    let off = Guard::new(false, 20 * GIB);
    for _ in 0..50 {
        assert!(off.offend(ip(PUBLIC), Offense::Malformed, now).is_none());
    }
    assert!(!off.banned(ip(PUBLIC), now));
}

#[test]
fn manual_ranges_ban_anyone_in_them() {
    let guard = guard();
    let now = Instant::now();
    guard.set_manual(parse_list("198.51.100.0/24\n# a note\n2001:db8::/32, 127.0.0.1").0);
    for banned in [
        "198.51.100.9",
        "2001:db8:1::5",
        "127.0.0.1",
        "::ffff:198.51.100.200",
    ] {
        assert!(guard.banned(ip(banned), now), "{banned}");
    }
    for allowed in ["198.51.101.1", "2001:db9::1", "127.0.0.2"] {
        assert!(!guard.banned(ip(allowed), now), "{allowed}");
    }
    let rows = guard.bans(now);
    assert_eq!(rows.iter().filter(|row| row.manual).count(), 3);
    guard.set_manual(Vec::new());
    assert!(!guard.banned(ip("198.51.100.9"), now));
}

#[test]
fn ranges_parse_and_refuse_nonsense() {
    let (ranges, bad) = parse_list("10.0.0.0/8, nonsense\n# only a comment\n\n::/0, 1.2.3.4/33");
    assert_eq!(ranges.len(), 2);
    assert_eq!(bad, vec![2, 4]);
    let range: Cidr = "2001:db8::/48".parse().unwrap();
    assert_eq!(range.to_string(), "2001:db8::/48");
    assert!(range.contains(ip("2001:db8:0:ffff::1")));
    assert!(!range.contains(ip("2001:db8:1::1")));
    assert!(!range.contains(ip("10.0.0.1")));
    let everything: Cidr = "0.0.0.0/0".parse().unwrap();
    assert!(everything.contains(ip("8.8.8.8")));
}

#[test]
fn the_dashboard_lists_earned_bans_with_their_reason() {
    let guard = guard();
    let now = Instant::now();
    offend_until_banned(&guard, ip(PUBLIC), Offense::Malformed, now);
    guard.charge(ip("2001:db8::7"), 81 * GIB, now);
    let rows = guard.bans(now);
    assert_eq!(rows.len(), 2);
    // Shown masked, so a leaked dashboard names no household: PUBLIC is
    // 203.0.113.50.
    assert!(
        rows.iter()
            .any(|row| row.address == "203.0.113.x" && row.reason == "malformed")
    );
    // IPv6 is banned as its /64.
    assert!(
        rows.iter()
            .any(|row| row.address == "2001:db8:0:x" && row.reason == "bandwidth")
    );
    assert!(rows.iter().all(|row| !row.address.contains(PUBLIC)));
    assert_eq!(guard.active(now), 2);
}

#[test]
fn a_table_full_of_bans_never_lifts_one_and_bans_wide_instead() {
    use crate::guard::MAX_RECORDS;
    let guard = guard();
    let now = Instant::now();
    let banned_address = |index: u32| {
        IpAddr::from(std::net::Ipv6Addr::from(
            (0x2001_0db8_u128 << 96) | (u128::from(index) << 64),
        ))
    };
    for index in 0..u32::try_from(MAX_RECORDS).unwrap() {
        offend_until_banned(&guard, banned_address(index), Offense::Malformed, now);
    }
    // Ordinary traffic from newcomers bans nothing: a lone point, or a
    // transfer still under budget, which every session charges each second.
    let innocent = ip("198.51.100.99");
    assert!(guard.offend(innocent, Offense::Limited, now).is_none());
    assert!(guard.charge(innocent, GIB, now).is_none());
    assert!(!guard.banned(innocent, now));
    assert!(!guard.banned(ip("198.51.100.1"), now));
    // A newcomer whose offenses would ban it alone is banned by its wide
    // block (an IPv4 /24) ...
    let late = ip("203.0.113.99");
    offend_until_banned(&guard, late, Offense::Malformed, now);
    assert!(guard.banned(late, now));
    assert!(guard.banned(ip("203.0.113.1"), now));
    // Counted among active bans, beside every address's.
    assert_eq!(guard.active(now), MAX_RECORDS + 1);
    // Repeated small offenses still add up, per block: forged proofs from
    // addresses across one /24.
    let mut banned_by_proofs = false;
    for last in 1..=10 {
        let prober = ip(&format!("192.0.2.{last}"));
        banned_by_proofs |= guard.offend(prober, Offense::BadProof, now).is_some();
    }
    assert!(banned_by_proofs);
    assert!(guard.banned(ip("192.0.2.200"), now));
    // So does transfer, against a block's larger budget.
    let heavy = ip("198.18.0.5");
    let block_budget = 20 * GIB * crate::guard::WIDE_BUDGET * crate::guard::OVERRUN;
    assert!(guard.charge(heavy, block_budget, now).is_none());
    assert!(guard.charge(heavy, 1, now).is_some());
    assert!(guard.banned(heavy, now));
    // ... and every earlier ban still stands.
    for index in [0, 1, 1_000, u32::try_from(MAX_RECORDS).unwrap() - 1] {
        assert!(guard.banned(banned_address(index), now), "{index}");
    }
}

#[test]
fn a_full_table_still_bans_new_offenders() {
    use crate::guard::MAX_RECORDS;
    let guard = guard();
    let now = Instant::now();
    // Fill every slot with a different public address holding one point,
    // as an attacker with many /64s could.
    for index in 0..MAX_RECORDS {
        let index = u32::try_from(index).unwrap();
        let address = IpAddr::from(std::net::Ipv6Addr::from(
            (0x2001_0db8_u128 << 96) | (u128::from(index) << 64),
        ));
        guard.offend(address, Offense::Limited, now);
    }
    // A new one is still counted and banned, rather than ignored.
    let late = ip("203.0.113.99");
    offend_until_banned(&guard, late, Offense::Malformed, now);
    assert!(guard.banned(late, now));
}
