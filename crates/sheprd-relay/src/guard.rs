//! Banning toxic clients, with nothing to set up.
//!
//! Each address (an IPv4 address, or an IPv6 /64, as the rate limits key
//! them) earns points for each [`Offense`]: hitting limits, probing,
//! forged proofs, malformed messages, and moving far too much. Points fade by one a minute; at [`BAN_POINTS`] the address is banned,
//! for 10 minutes, then an hour, then a day if it keeps at it.
//!
//! A banned address is refused before any new work (before the QUIC
//! handshake, before any HTTP route). Whether the hub also ends what it
//! has open depends on why: a ban that any [`severe`] offense led to ends
//! its sessions and listeners, but one earned only by running into limits
//! leaves them alone, since a household or a carrier's NAT can run into
//! limits together without anyone misbehaving.
//!
//! Each side of a session is charged for the bytes it sends. An address
//! over its hourly budget has its sessions slowed to [`SLOW`], not banned;
//! only one at [`OVERRUN`] budgets is banned.
//! Loopback and private addresses are never banned automatically: behind a
//! reverse proxy that is misconfigured, every caller can look like the
//! proxy, and banning it would lock everyone out. Operators can still ban
//! any range by hand ([`Cidr`]).

mod cidr;

pub use cidr::{Cidr, parse_list};

use crate::{bucket::source_key, metrics::Offense};
use serde::Serialize;
use std::{
    collections::HashMap,
    net::IpAddr,
    sync::{Mutex, MutexGuard, RwLock},
    time::{Duration, Instant},
};

/// Points that ban an address.
pub const BAN_POINTS: u32 = 10;
/// How long a ban lasts, by how many the address had lately.
pub const BANS: [Duration; 3] = [
    Duration::from_secs(10 * 60),
    Duration::from_secs(60 * 60),
    Duration::from_secs(24 * 60 * 60),
];
/// A clean day after a ban ends forgets the bans before it.
const FORGIVE: Duration = Duration::from_secs(24 * 60 * 60);
/// The window the transfer budget counts over.
const HOUR: Duration = Duration::from_secs(60 * 60);
/// Points fade by one each of these.
const FADE: Duration = Duration::from_secs(60);
/// The most addresses tracked at once, which bounds memory.
pub const MAX_RECORDS: usize = 65_536;
/// The most wide blocks tracked when the table is full of bans.
pub const MAX_WIDE: usize = 4_096;
/// A wide block's transfer budget, in address budgets: a /48 or /24 can
/// hold many unrelated users, judged together only while bans fill the
/// table.
pub const WIDE_BUDGET: u64 = 16;
/// The most bans the dashboard lists.
const MAX_ROWS: usize = 50;

/// An address over its hourly transfer budget has each session slowed to
/// this many bytes a second each way, until the hour's window ends.
pub const SLOW: u64 = 256 * 1024;
/// An address that moves this many hourly budgets in the hour anyway, as
/// many sessions slowed at once can, is banned.
pub const OVERRUN: u64 = 4;

/// What one offense costs.
#[must_use]
pub const fn points(offense: Offense) -> u32 {
    match offense {
        // One point, fading one a minute: only a burst adds up. The rate
        // limits catch a phone retrying too fast, so a refused dial costs
        // nothing of its own: a phone whose computer is off, retrying with
        // backoff, never gets its household's shared address banned.
        Offense::ConnectionCap | Offense::Limited => 1,
        Offense::Probe => 2,
        Offense::BadProof | Offense::Malformed => 3,
        Offense::Bandwidth => BAN_POINTS,
    }
}

/// Whether `offense` shows a caller misbehaving, rather than a busy address
/// running into a limit. A ban any of these led to ends what the address
/// has open; one earned by limits alone only refuses new work.
#[must_use]
pub const fn severe(offense: Offense) -> bool {
    !matches!(offense, Offense::ConnectionCap | Offense::Limited)
}

#[derive(Clone, Copy, Debug)]
struct Record {
    banned_until: Option<Instant>,
    /// Bans lately, which sets the next one's length.
    bans: u32,
    /// Whether the current ban ends the address's open work ([`severe`]).
    evicts: bool,
    /// Whether the points held now include a [`severe`] offense's.
    hostile: bool,
    last_ban_end: Option<Instant>,
    points: u32,
    reason: Offense,
    updated: Instant,
    window_bytes: u64,
    window_start: Instant,
}

impl Record {
    fn new(now: Instant) -> Self {
        Self {
            banned_until: None,
            bans: 0,
            evicts: false,
            hostile: false,
            last_ban_end: None,
            points: 0,
            reason: Offense::Limited,
            updated: now,
            window_bytes: 0,
            window_start: now,
        }
    }

    /// Fades points and the transfer window to `now`.
    fn fade(&mut self, now: Instant) {
        let minutes = now.saturating_duration_since(self.updated).as_secs() / FADE.as_secs();
        if minutes > 0 {
            self.points = self
                .points
                .saturating_sub(u32::try_from(minutes).unwrap_or(u32::MAX));
            self.updated += FADE * u32::try_from(minutes).unwrap_or(u32::MAX);
            self.hostile &= self.points > 0;
        }
        if now.saturating_duration_since(self.window_start) >= HOUR {
            self.window_start = now;
            self.window_bytes = 0;
        }
        if self
            .last_ban_end
            .is_some_and(|end| now.saturating_duration_since(end) >= FORGIVE)
        {
            self.bans = 0;
            self.last_ban_end = None;
        }
    }

    fn banned(&self, now: Instant) -> bool {
        self.banned_until.is_some_and(|until| until > now)
    }

    /// Bytes moved in the current hour's window.
    fn spent(&self, now: Instant) -> u64 {
        if now.saturating_duration_since(self.window_start) >= HOUR {
            0
        } else {
            self.window_bytes
        }
    }

    /// Bans for `reason`; how long.
    fn ban(&mut self, reason: Offense, now: Instant) -> Duration {
        let index = usize::try_from(self.bans)
            .unwrap_or(usize::MAX)
            .min(BANS.len() - 1);
        let length = BANS[index];
        self.banned_until = Some(now + length);
        self.last_ban_end = Some(now + length);
        self.bans = self.bans.saturating_add(1);
        self.evicts = self.hostile || severe(reason);
        self.hostile = false;
        self.points = 0;
        self.reason = reason;
        length
    }

    /// Turns a ban that only refused new work into one that evicts, for
    /// `reason`, keeping its end: how long it has left, if it changed.
    /// Counting goes on during a ban that leaves open work running, or that
    /// work could misbehave, or move any amount, unseen.
    fn escalate(&mut self, reason: Offense, now: Instant) -> Option<Duration> {
        if self.evicts {
            return None;
        }
        self.evicts = true;
        self.reason = reason;
        self.banned_until
            .map(|until| until.saturating_duration_since(now))
    }

    /// Nothing worth keeping: no ban, no points, no recent transfer.
    fn idle(&self, now: Instant) -> bool {
        !self.banned(now) && self.points == 0 && self.bans == 0 && self.window_bytes == 0
    }
}

/// How the dashboard shows an earned ban's address: enough to tell bans
/// apart and to ban a range by hand, not enough to name a household, so a
/// leaked dashboard leaks no one's address. `203.0.113.x`, or
/// `2001:db8:7:x` for an IPv6 /64.
#[must_use]
pub fn masked(address: IpAddr) -> String {
    match address.to_canonical() {
        IpAddr::V4(address) => {
            let [a, b, c, _] = address.octets();
            format!("{a}.{b}.{c}.x")
        }
        IpAddr::V6(address) => {
            let [a, b, c, ..] = address.segments();
            format!("{a:x}:{b:x}:{c:x}:x")
        }
    }
}

/// A ban the dashboard lists.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct BanRow {
    /// The address, masked ([`masked`]); a wide block or a manual entry's
    /// range as it is.
    pub address: String,
    /// Set by the operator rather than earned.
    pub manual: bool,
    pub reason: &'static str,
    /// Seconds until it lifts; 0 for a manual ban, which lasts.
    pub seconds_left: u64,
}

/// Bans, earned and manual.
#[derive(Debug)]
pub struct Guard {
    auto: bool,
    manual: RwLock<Vec<Cidr>>,
    max_transfer_per_hour: u64,
    records: Mutex<HashMap<IpAddr, Record>>,
    /// Wide blocks (an IPv6 /48, an IPv4 /24), counted and banned like
    /// addresses while every record is an active ban, so newcomers are
    /// still judged then.
    wide: Mutex<HashMap<IpAddr, Record>>,
}

/// The wide block `address` falls in: its IPv6 /48 or IPv4 /24, where an
/// attacker's many addresses usually sit together.
#[must_use]
pub fn wide_key(address: IpAddr) -> IpAddr {
    match address.to_canonical() {
        IpAddr::V4(address) => {
            IpAddr::V4(std::net::Ipv4Addr::from(u32::from(address) & 0xffff_ff00))
        }
        IpAddr::V6(address) => IpAddr::V6(std::net::Ipv6Addr::from(
            u128::from(address) & !((1u128 << 80) - 1),
        )),
    }
}

/// Whether an address is one automatic bans leave alone.
#[must_use]
pub fn exempt(address: IpAddr) -> bool {
    match address.to_canonical() {
        IpAddr::V4(address) => {
            address.is_loopback()
                || address.is_private()
                || address.is_link_local()
                || address.is_unspecified()
        }
        IpAddr::V6(address) => {
            address.is_loopback()
                || address.is_unique_local()
                || address.is_unicast_link_local()
                || address.is_unspecified()
        }
    }
}

impl Guard {
    /// A guard that bans automatically when `auto`, and bans an address
    /// moving more than `max_transfer_per_hour` bytes (0 for no budget).
    #[must_use]
    pub fn new(auto: bool, max_transfer_per_hour: u64) -> Self {
        Self {
            auto,
            manual: RwLock::default(),
            max_transfer_per_hour,
            records: Mutex::default(),
            wide: Mutex::default(),
        }
    }

    fn records(&self) -> MutexGuard<'_, HashMap<IpAddr, Record>> {
        match self.records.lock() {
            Ok(records) => records,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    /// Replaces the operator's ranges.
    pub fn set_manual(&self, ranges: Vec<Cidr>) {
        match self.manual.write() {
            Ok(mut manual) => *manual = ranges,
            Err(poisoned) => *poisoned.into_inner() = ranges,
        }
    }

    fn manually_banned(&self, address: IpAddr) -> bool {
        let contains = |ranges: &Vec<Cidr>| ranges.iter().any(|range| range.contains(address));
        match self.manual.read() {
            Ok(manual) => contains(&manual),
            Err(poisoned) => contains(&poisoned.into_inner()),
        }
    }

    /// Whether `address` is banned at `now`, by hand or earned.
    pub fn banned(&self, address: IpAddr, now: Instant) -> bool {
        if self.manually_banned(address) {
            return true;
        }
        if self
            .records()
            .get(&source_key(address))
            .is_some_and(|record| record.banned(now))
        {
            return true;
        }
        self.wide()
            .get(&wide_key(address))
            .is_some_and(|record| record.banned(now))
    }

    /// Whether a ban on `address` at `now` also ends what it has open: a
    /// manual ban, or one a [`severe`] offense led to.
    pub fn evicts(&self, address: IpAddr, now: Instant) -> bool {
        if self.manually_banned(address) {
            return true;
        }
        let evicts = |record: &Record| record.banned(now) && record.evicts;
        if self.records().get(&source_key(address)).is_some_and(evicts) {
            return true;
        }
        self.wide().get(&wide_key(address)).is_some_and(evicts)
    }

    /// Whether `address` moved more than its hourly budget in the current
    /// window, so its sessions are slowed to [`SLOW`].
    pub fn over_budget(&self, address: IpAddr, now: Instant) -> bool {
        let budget = self.max_transfer_per_hour;
        if !self.auto || budget == 0 || exempt(address) {
            return false;
        }
        if self
            .records()
            .get(&source_key(address))
            .is_some_and(|record| record.spent(now) > budget)
        {
            return true;
        }
        let wide_budget = budget.saturating_mul(WIDE_BUDGET);
        self.wide()
            .get(&wide_key(address))
            .is_some_and(|record| record.spent(now) > wide_budget)
    }

    fn wide(&self) -> MutexGuard<'_, HashMap<IpAddr, Record>> {
        match self.wide.lock() {
            Ok(wide) => wide,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    /// Runs `change` on `address`'s record; a ban it caused, newly. When
    /// the table is all bans, it runs on the wide block's record instead,
    /// but only if `wide`: a block holds many users, so hitting limits,
    /// which a busy neighbour does, is never held against it.
    fn record(
        &self,
        address: IpAddr,
        now: Instant,
        wide: bool,
        change: impl FnOnce(&mut Record, u64) -> Option<Duration>,
    ) -> Option<Duration> {
        if !self.auto || exempt(address) {
            return None;
        }
        let key = source_key(address);
        let mut records = self.records();
        if records.len() >= MAX_RECORDS && !records.contains_key(&key) {
            records.retain(|_, record| {
                record.fade(now);
                !record.idle(now)
            });
            if records.len() >= MAX_RECORDS {
                // Still full, as when one attacker holds many /64s. Never
                // lift a ban to make room: drop the unbanned record with
                // the fewest points. If every slot is an active ban, the
                // newcomer is counted by its wide block instead.
                let least = records
                    .iter()
                    .filter(|(_, record)| !record.banned(now))
                    .min_by_key(|(_, record)| (record.points, record.updated))
                    .map(|(key, _)| *key);
                match least {
                    Some(least) => {
                        records.remove(&least);
                    }
                    None if wide => {
                        drop(records);
                        return self.record_wide(address, now, change);
                    }
                    None => return None,
                }
            }
        }
        let record = records.entry(key).or_insert_with(|| Record::new(now));
        record.fade(now);
        change(record, 1)
    }

    /// Runs `change` on the record of `address`'s wide block, when the
    /// address table is all bans. Points add up and fade as for an
    /// address, so repeated offenses still ban; the transfer budget is
    /// [`WIDE_BUDGET`] times an address's, as a block holds many users.
    fn record_wide(
        &self,
        address: IpAddr,
        now: Instant,
        change: impl FnOnce(&mut Record, u64) -> Option<Duration>,
    ) -> Option<Duration> {
        let key = wide_key(address);
        let mut wide = self.wide();
        if wide.len() >= MAX_WIDE && !wide.contains_key(&key) {
            wide.retain(|_, record| {
                record.fade(now);
                !record.idle(now)
            });
            if wide.len() >= MAX_WIDE {
                let least = wide
                    .iter()
                    .filter(|(_, record)| !record.banned(now))
                    .min_by_key(|(_, record)| (record.points, record.updated))
                    .map(|(key, _)| *key);
                // None: tens of thousands of bans and thousands of banned
                // blocks at once; the rate limits still hold every caller
                // back.
                wide.remove(&least?);
            }
        }
        let record = wide.entry(key).or_insert_with(|| Record::new(now));
        record.fade(now);
        change(record, WIDE_BUDGET)
    }

    /// Counts `offense` against `address`; a ban it caused, or made evict,
    /// newly.
    pub fn offend(&self, address: IpAddr, offense: Offense, now: Instant) -> Option<Duration> {
        self.record(address, now, severe(offense), |record, _| {
            if record.banned(now) {
                // Open work kept through a ban for limits: misbehaving now
                // ends it.
                return severe(offense)
                    .then(|| record.escalate(offense, now))
                    .flatten();
            }
            record.points = record.points.saturating_add(points(offense));
            record.hostile |= severe(offense);
            (record.points >= BAN_POINTS).then(|| record.ban(offense, now))
        })
    }

    /// Counts `bytes` `address` sent, banned or not; a ban for moving
    /// [`OVERRUN`] hourly budgets, newly, or a ban for limits made to evict. Going over one budget only slows it
    /// ([`Guard::over_budget`]).
    pub fn charge(&self, address: IpAddr, bytes: u64, now: Instant) -> Option<Duration> {
        if self.max_transfer_per_hour == 0 || bytes == 0 {
            return None;
        }
        let budget = self.max_transfer_per_hour;
        self.record(address, now, true, |record, scale| {
            record.window_bytes = record.window_bytes.saturating_add(bytes);
            let budget = budget.saturating_mul(scale).saturating_mul(OVERRUN);
            if record.window_bytes <= budget {
                return None;
            }
            if record.banned(now) {
                return record.escalate(Offense::Bandwidth, now);
            }
            Some(record.ban(Offense::Bandwidth, now))
        })
    }

    /// Active bans, earned ones first by time left, then manual ranges.
    #[must_use]
    pub fn bans(&self, now: Instant) -> Vec<BanRow> {
        let mut rows: Vec<BanRow> = self
            .records()
            .iter()
            .filter_map(|(address, record)| {
                let until = record.banned_until.filter(|until| *until > now)?;
                Some(BanRow {
                    address: masked(*address),
                    manual: false,
                    reason: record.reason.label(),
                    seconds_left: (until - now).as_secs(),
                })
            })
            .collect();
        rows.extend(self.wide().iter().filter_map(|(block, record)| {
            let until = record.banned_until.filter(|until| *until > now)?;
            Some(BanRow {
                address: match block {
                    IpAddr::V6(_) => format!("{block}/48"),
                    IpAddr::V4(_) => format!("{block}/24"),
                },
                manual: false,
                reason: record.reason.label(),
                seconds_left: (until - now).as_secs(),
            })
        }));
        rows.sort_by_key(|row| std::cmp::Reverse(row.seconds_left));
        let manual = match self.manual.read() {
            Ok(manual) => manual.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        };
        rows.extend(manual.iter().map(|range| BanRow {
            address: range.to_string(),
            manual: true,
            reason: "manual",
            seconds_left: 0,
        }));
        rows.truncate(MAX_ROWS);
        rows
    }

    /// How many addresses are banned now, earned bans only.
    #[must_use]
    pub fn active(&self, now: Instant) -> usize {
        let wide = self
            .wide()
            .values()
            .filter(|record| record.banned(now))
            .count();
        self.records()
            .values()
            .filter(|record| record.banned(now))
            .count()
            + wide
    }
}
