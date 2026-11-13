//! Rate limits per address, and per wide block. An attacker holding a
//! routed IPv6 /48 has 65,536 /64s, each a fresh address to the narrow
//! limit; the wide one, [`WIDE_SHARE`] times as loose, holds the whole
//! block back while leaving room for the many users a /48 or an IPv4 /24
//! can hold.

use crate::{
    bucket::{Limiter, Outcome, Rate, source_key},
    guard::wide_key,
    limits::WIDE_SHARE,
};
use std::{
    net::IpAddr,
    sync::{Mutex, MutexGuard},
    time::Instant,
};

/// One rate, applied to each address and to its wide block.
#[derive(Debug)]
pub struct PerAddress {
    narrow: Mutex<Limiter<IpAddr>>,
    /// Where the limiters' milliseconds count from: a monotonic clock, so a
    /// wall clock stepping back cannot stop refills.
    origin: Instant,
    wide: Mutex<Limiter<IpAddr>>,
}

impl PerAddress {
    #[must_use]
    pub fn new(per_minute: u32, origin: Instant) -> Self {
        let share = u32::try_from(WIDE_SHARE).unwrap_or(u32::MAX);
        Self {
            narrow: Mutex::new(Limiter::new(Rate::per_minute(per_minute))),
            origin,
            wide: Mutex::new(Limiter::new(Rate::per_minute(
                per_minute.saturating_mul(share),
            ))),
        }
    }

    /// Whether `address` may go ahead now, spending from both buckets only
    /// when both allow it. A refusal tracks nothing new: a caller rotating
    /// through its block's /64s, refused by the block, cannot fill the
    /// address table and lock newcomers out; and one /64 the narrow limit
    /// holds back does not drain its neighbours' share of the block.
    pub fn check(&self, address: IpAddr) -> Outcome {
        let now_ms = u64::try_from(self.origin.elapsed().as_millis()).unwrap_or(u64::MAX);
        let (narrow_key, wide_key) = (source_key(address), wide_key(address));
        let mut wide = lock(&self.wide);
        let peeked = wide.peek(&wide_key, now_ms);
        if peeked != Outcome::Allowed {
            return peeked;
        }
        let mut narrow = lock(&self.narrow);
        let checked = narrow.check(narrow_key, now_ms);
        if checked != Outcome::Allowed {
            return checked;
        }
        wide.check(wide_key, now_ms)
    }
}

fn lock(limiter: &Mutex<Limiter<IpAddr>>) -> MutexGuard<'_, Limiter<IpAddr>> {
    match limiter.lock() {
        Ok(limiter) => limiter,
        Err(poisoned) => poisoned.into_inner(),
    }
}
