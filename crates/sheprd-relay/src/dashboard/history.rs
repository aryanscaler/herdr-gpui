//! Throughput over the last ten minutes, sampled on the relay so the
//! dashboard draws a full chart as soon as it opens.

use serde::Serialize;
use std::{
    collections::VecDeque,
    sync::{Mutex, Weak},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

/// How often throughput is sampled.
pub const INTERVAL: Duration = Duration::from_secs(5);
/// Ten minutes of samples.
pub const SAMPLES: usize = 120;

/// One sample: bytes per second each way over the interval before `at_ms`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct Sample {
    pub at_ms: u64,
    pub from_dialers: u64,
    pub from_listeners: u64,
}

/// The samples, oldest first, and the totals the next one is measured from.
#[derive(Debug, Default)]
pub struct History {
    last: Option<(u64, u64, u64)>,
    samples: VecDeque<Sample>,
}

impl History {
    /// Records the totals at `at_ms`, turning the change since the last call
    /// into a rate. Totals that went back (a restart) count as nothing.
    pub fn record(&mut self, at_ms: u64, from_dialers: u64, from_listeners: u64) {
        if let Some((then, dialers, listeners)) = self.last {
            let elapsed = at_ms.saturating_sub(then).max(1);
            let rate =
                |now: u64, before: u64| now.saturating_sub(before).saturating_mul(1000) / elapsed;
            self.samples.push_back(Sample {
                at_ms,
                from_dialers: rate(from_dialers, dialers),
                from_listeners: rate(from_listeners, listeners),
            });
            while self.samples.len() > SAMPLES {
                self.samples.pop_front();
            }
        }
        self.last = Some((at_ms, from_dialers, from_listeners));
    }

    #[must_use]
    pub fn samples(&self) -> Vec<Sample> {
        self.samples.iter().copied().collect()
    }
}

/// Samples `metrics` into `history` every [`INTERVAL`] until the dashboard
/// is dropped.
pub async fn run(history: Weak<Mutex<History>>, metrics: impl Fn() -> (u64, u64)) {
    let mut ticks = tokio::time::interval(INTERVAL);
    loop {
        ticks.tick().await;
        let Some(history) = history.upgrade() else {
            return;
        };
        let (dialers, listeners) = metrics();
        let at_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| {
                u64::try_from(since.as_millis()).unwrap_or(u64::MAX)
            });
        match history.lock() {
            Ok(mut history) => history.record(at_ms, dialers, listeners),
            Err(poisoned) => poisoned.into_inner().record(at_ms, dialers, listeners),
        }
    }
}
