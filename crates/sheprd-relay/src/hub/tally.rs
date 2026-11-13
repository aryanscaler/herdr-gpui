//! Counts of what each address holds, by the address the limits key it
//! under and by its wide block, so one caller cannot fill a pool the whole
//! relay shares: the listener slots, say, from a routed IPv6 /56.

use crate::{bucket::source_key, guard::wide_key, limits::WIDE_SHARE};
use std::{collections::HashMap, hash::Hash, net::IpAddr};

/// How many of something each address, and each wide block, holds.
#[derive(Debug, Default)]
pub struct Tally {
    blocks: Counts<IpAddr>,
    sources: Counts<IpAddr>,
}

impl Tally {
    /// Counts one more for `address`, unless it already holds `per_source`
    /// or its block holds [`WIDE_SHARE`] times that.
    pub fn try_add(&mut self, address: IpAddr, per_source: usize) -> bool {
        let source = source_key(address);
        let block = wide_key(address);
        if self.sources.get(&source) >= per_source
            || self.blocks.get(&block) >= per_source.saturating_mul(WIDE_SHARE)
        {
            return false;
        }
        self.sources.add(source);
        self.blocks.add(block);
        true
    }

    /// Gives back one that `address` held.
    pub fn remove(&mut self, address: IpAddr) {
        self.sources.remove(&source_key(address));
        self.blocks.remove(&wide_key(address));
    }
}

/// A count per key, which forgets keys that reach zero.
#[derive(Debug)]
pub struct Counts<K>(HashMap<K, usize>);

impl<K> Default for Counts<K> {
    fn default() -> Self {
        Self(HashMap::new())
    }
}

impl<K: Eq + Hash> Counts<K> {
    #[must_use]
    pub fn get(&self, key: &K) -> usize {
        self.0.get(key).copied().unwrap_or(0)
    }

    pub fn add(&mut self, key: K) {
        self.add_by(key, 1);
    }

    pub fn add_by(&mut self, key: K, amount: usize) {
        let held = self.0.entry(key).or_insert(0);
        *held = held.saturating_add(amount);
    }

    pub fn remove(&mut self, key: &K) {
        self.remove_by(key, 1);
    }

    pub fn remove_by(&mut self, key: &K, amount: usize) {
        if let Some(held) = self.0.get_mut(key) {
            *held = held.saturating_sub(amount);
            if *held == 0 {
                self.0.remove(key);
            }
        }
    }
}
