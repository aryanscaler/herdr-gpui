//! Live analytics: one snapshot a second as Server-Sent Events, so the page
//! updates without polling. A stream ends when the login it was opened with
//! expires, and only [`MAX_STREAMS`] may be open at once, so viewers cannot
//! cost the relay more than a few small writes a second.

use axum::response::sse::{Event, KeepAlive};
use futures_util::{Stream, stream};
use std::{
    convert::Infallible,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering::Relaxed},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

/// Live streams open at once, across every viewer.
pub const MAX_STREAMS: usize = 16;
/// How often a stream sends a snapshot.
pub const INTERVAL: Duration = Duration::from_secs(1);

/// The open streams, counted.
#[derive(Debug, Default)]
pub struct Streams {
    open: AtomicUsize,
}

/// One open stream's place, given back when the stream ends.
#[derive(Debug)]
pub struct Place {
    streams: Arc<Streams>,
}

impl Drop for Place {
    fn drop(&mut self) {
        self.streams.open.fetch_sub(1, Relaxed);
    }
}

impl Streams {
    /// A place for one more stream, unless [`MAX_STREAMS`] are open.
    pub fn open(self: &Arc<Self>) -> Option<Place> {
        self.open
            .try_update(Relaxed, Relaxed, |open| {
                (open < MAX_STREAMS).then_some(open + 1)
            })
            .ok()
            .map(|_| Place {
                streams: Arc::clone(self),
            })
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

/// Snapshots from `snapshot` every [`INTERVAL`], until `expires` (Unix
/// seconds) passes or the viewer leaves. The first comes at once.
pub fn events(
    place: Place,
    expires: u64,
    snapshot: impl Fn() -> String + Send + 'static,
) -> (impl Stream<Item = Result<Event, Infallible>>, KeepAlive) {
    let mut ticks = tokio::time::interval(INTERVAL);
    ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let state = (ticks, place, snapshot);
    let events = stream::unfold(state, move |(mut ticks, place, snapshot)| async move {
        ticks.tick().await;
        if now_secs() >= expires {
            return None;
        }
        let event = Event::default().event("stats").data(snapshot());
        Some((Ok(event), (ticks, place, snapshot)))
    });
    (events, KeepAlive::default())
}
