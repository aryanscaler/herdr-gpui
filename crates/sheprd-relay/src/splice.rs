//! Carries bytes between a dialer and the listener that accepted it, each
//! way on its own so a throttled direction never stalls the other. Bytes
//! are opaque ciphertext: they are counted, never read or logged.
//!
//! When either side ends, the session ends: a WebSocket has no half-close,
//! and the peers' own protocol says goodbye before closing.

mod peer;

pub use peer::Peer;

use crate::{
    bucket::Throttle,
    guard::SLOW,
    hub::Session,
    metrics::{Direction, End, Offense, Throttled},
};
use peer::{Next, Sink, Source};
use std::{
    sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed},
    time::Duration,
};
use tokio::time::{Instant, sleep_until, timeout};

/// How long ending a side may take once the session is over.
const CLOSE_TIMEOUT: Duration = Duration::from_secs(1);
/// How often the watchdog checks for idleness.
const WATCH: Duration = Duration::from_secs(1);

/// WebSocket close codes a peer may see from the relay, in the private
/// range. QUIC peers get [`crate::wire::code`] instead.
pub mod code {
    /// The other side closed or broke.
    pub const PEER_GONE: u16 = 4000;
    /// No route by that name, or no listener on it.
    pub const OFFLINE: u16 = 4404;
    /// Nothing crossed the session for the idle timeout.
    pub const IDLE: u16 = 4408;
    /// Another socket proved the same key and took over the route.
    pub const REPLACED: u16 = 4409;
    /// The session reached its longest allowed length.
    pub const EXPIRED: u16 = 4410;
    /// A text frame, or a frame larger than allowed.
    pub const PROTOCOL: u16 = 4400;
    /// The listener's proof or token was refused.
    pub const REFUSED: u16 = 4401;
    /// The relay holds as many listeners as it may.
    pub const FULL: u16 = 4503;
    /// The listener stopped allowing the dial key this session used.
    pub const REVOKED: u16 = 4403;
    /// One side's address was banned for misbehaving.
    pub const BANNED: u16 = 4429;
}

/// Bounds one session runs under.
#[derive(Clone, Copy, Debug)]
pub struct Bounds {
    /// Bytes per second each way; zero is unlimited.
    pub bandwidth: u64,
    pub idle: Duration,
    pub longest: Duration,
    /// The most bytes read from a stream at once.
    pub chunk: usize,
}

/// What each side of a session has been charged for so far, and whether
/// either side is over its hourly transfer budget. Kept outside the
/// watchdog, which is dropped when a pump finishes, so the rest is settled
/// after.
#[derive(Debug, Default)]
pub(crate) struct Ledger {
    from_dialer: AtomicU64,
    from_listener: AtomicU64,
    /// Either side is over budget, so both directions run at [`SLOW`].
    slow: AtomicBool,
}

/// The throttles one direction runs under.
#[derive(Clone, Copy, Debug)]
struct Lane<'a> {
    /// The session's bandwidth, if limited.
    session: Option<&'a Throttle>,
    /// [`SLOW`], while the ledger says a side is over budget.
    slow: Option<&'a Throttle>,
    ledger: &'a Ledger,
}

/// Runs a session until a peer leaves, it idles, it expires, the listener
/// revokes the key it was dialed with, or either side is banned. A side
/// that breaks the protocol is counted toward a ban, and every second each
/// side's bytes are charged to its hourly transfer budget.
pub async fn splice(dialer: Peer, listener: Peer, session: Session) {
    let bounds = session.bounds();
    let (from_dialer, mut to_dialer) = dialer.split();
    let (from_listener, mut to_listener) = listener.split();
    let last = AtomicU64::new(0);
    let ledger = Ledger::default();
    let started = Instant::now();
    let up = Throttle::new(bounds.bandwidth);
    let down = Throttle::new(bounds.bandwidth);
    let slow_up = Throttle::new(SLOW);
    let slow_down = Throttle::new(SLOW);
    let end = {
        let pump_up = pump(
            from_dialer,
            &mut to_listener,
            Direction::FromDialer,
            Lane {
                session: up.as_ref(),
                slow: slow_up.as_ref(),
                ledger: &ledger,
            },
            &session,
            &last,
            started,
            bounds.chunk,
        );
        let pump_down = pump(
            from_listener,
            &mut to_dialer,
            Direction::FromListener,
            Lane {
                session: down.as_ref(),
                slow: slow_down.as_ref(),
                ledger: &ledger,
            },
            &session,
            &last,
            started,
            bounds.chunk,
        );
        tokio::select! {
            end = pump_up => end,
            end = pump_down => end,
            end = watchdog(&session, &ledger, &last, started, bounds.idle, bounds.longest) => end,
            () = session.revoked() => End::Revoked,
            () = session.banned() => End::Banned,
        }
    };
    settle(&session, &ledger);
    let _ = tokio::join!(
        timeout(CLOSE_TIMEOUT, to_dialer.finish(end)),
        timeout(CLOSE_TIMEOUT, to_listener.finish(end)),
    );
    session.metrics().ended(end, session.started.elapsed());
}

/// Moves bytes from `from` to `to` until either side ends.
#[allow(clippy::too_many_arguments)]
async fn pump(
    mut from: Source,
    to: &mut Sink,
    direction: Direction,
    lane: Lane<'_>,
    session: &Session,
    last: &AtomicU64,
    started: Instant,
    chunk: usize,
) -> End {
    let counter = match direction {
        Direction::FromDialer => &session.traffic.from_dialer,
        Direction::FromListener => &session.traffic.from_listener,
    };
    let metrics = session.metrics();
    loop {
        let bytes = match from.next(chunk).await {
            Next::Data(bytes) => bytes,
            Next::Violation => {
                // Only the side that broke the protocol is counted.
                let culprit = match direction {
                    Direction::FromDialer => session.dialer(),
                    Direction::FromListener => session.listener(),
                };
                session.hub_arc().offend(culprit, Offense::Malformed);
                return End::Protocol;
            }
            Next::End => return End::Closed,
        };
        let length = bytes.len();
        if let Some(throttle) = lane.session {
            let wait = throttle.take(length).await;
            if !wait.is_zero() {
                metrics.throttled(Throttled::Session, wait);
            }
        }
        if let (true, Some(slow)) = (lane.ledger.slow.load(Relaxed), lane.slow) {
            let wait = slow.take(length).await;
            if !wait.is_zero() {
                metrics.throttled(Throttled::Budget, wait);
            }
        }
        if let Some(total) = session.hub().total_throttle(direction) {
            let wait = total.take(length).await;
            if !wait.is_zero() {
                metrics.throttled(Throttled::Total, wait);
            }
        }
        if !to.send(bytes).await {
            return End::Closed;
        }
        counter.fetch_add(length as u64, Relaxed);
        metrics.carried(direction, length);
        let since = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        last.store(since, Relaxed);
    }
}

/// Charges each side for what it sent since the last charge, then notes
/// whether either side is now over its budget. Only the sender is charged:
/// a computer is not banned for what a busy phone sends it, nor the phone
/// for what it downloads.
pub(crate) fn settle(session: &Session, ledger: &Ledger) {
    let hub = session.hub_arc();
    let sides = [
        (
            session.dialer(),
            &session.traffic.from_dialer,
            &ledger.from_dialer,
        ),
        (
            session.listener(),
            &session.traffic.from_listener,
            &ledger.from_listener,
        ),
    ];
    for (address, sent, charged) in sides {
        let sent = sent.load(Relaxed);
        let before = charged.swap(sent, Relaxed);
        if sent > before {
            hub.charge(address, sent - before);
        }
    }
    let slow = hub.over_budget(session.dialer()) || hub.over_budget(session.listener());
    ledger.slow.store(slow, Relaxed);
}

/// Resolves when the session has idled or run too long. Each turn also
/// charges what crossed since the last one to each side's budget.
async fn watchdog(
    session: &Session,
    ledger: &Ledger,
    last: &AtomicU64,
    started: Instant,
    idle: Duration,
    longest: Duration,
) -> End {
    let deadline = started + longest;
    loop {
        settle(session, ledger);
        let quiet_since = started + Duration::from_millis(last.load(Relaxed));
        let idle_at = quiet_since + idle;
        if Instant::now() >= deadline {
            return End::Expired;
        }
        if Instant::now() >= idle_at {
            return End::Idle;
        }
        sleep_until(idle_at.min(deadline).min(Instant::now() + WATCH)).await;
    }
}
