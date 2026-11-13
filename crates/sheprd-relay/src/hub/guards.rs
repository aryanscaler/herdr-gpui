//! The guards that hold the hub's slots: each gives its slot back when
//! dropped, however its task ends.

use super::{Accepted, Hub, Incoming, PAIRING_SESSION, Traffic};
use crate::{
    RouteId, SessionId,
    bucket::{Limiter, Rate},
    metrics::{Gauge, Kind, Metrics},
    splice::Bounds,
    wire::Key,
};
use std::{
    net::IpAddr,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

/// What one listener may say about who may dial it: plenty for a computer
/// pairing devices, too little to keep the hub's lock busy.
const MESSAGES: Rate = Rate {
    burst: 16,
    per_minute: 60,
};

/// Bytes taken from the hub's buffer budget, given back when dropped.
#[derive(Debug)]
pub struct Buffer {
    bytes: u64,
    /// The address the bytes count against, unless only the total did.
    counted: Option<IpAddr>,
    hub: Arc<Hub>,
}

impl Buffer {
    pub(super) fn new(hub: Arc<Hub>, counted: Option<IpAddr>, bytes: u64) -> Self {
        Self {
            bytes,
            counted,
            hub,
        }
    }
}

impl Drop for Buffer {
    fn drop(&mut self) {
        self.hub.release_buffer(self.counted, self.bytes);
    }
}
use tokio::sync::{Notify, mpsc, oneshot};

/// One open socket or connection, counted against its address until
/// dropped.
#[derive(Debug)]
pub struct Permit {
    hub: Arc<Hub>,
    key: IpAddr,
    kind: Kind,
}

impl Permit {
    pub(super) fn new(hub: Arc<Hub>, key: IpAddr, kind: Kind) -> Self {
        Self { hub, key, kind }
    }
}

impl Drop for Permit {
    fn drop(&mut self) {
        let mut state = self.hub.state();
        state.connections = state.connections.saturating_sub(1);
        if let Some(held) = state.addresses.get_mut(&self.key) {
            *held = held.saturating_sub(1);
            if *held == 0 {
                state.addresses.remove(&self.key);
            }
        }
        drop(state);
        self.hub.metrics.lower(Gauge::Connections(self.kind));
    }
}

/// A route's listener, until dropped or replaced.
#[derive(Debug)]
pub struct Registration {
    /// Where it listens from, to count offenses against.
    pub address: IpAddr,
    generation: u64,
    hub: Arc<Hub>,
    /// Dials for this listener; ends when another replaces it.
    pub incoming: mpsc::Receiver<Incoming>,
    /// Its messages about who may dial, limited.
    messages: Mutex<(Limiter<()>, Instant)>,
    pub route: RouteId,
}

impl Registration {
    pub(super) fn new(
        hub: Arc<Hub>,
        (generation, address): (u64, IpAddr),
        incoming: mpsc::Receiver<Incoming>,
        route: RouteId,
    ) -> Self {
        Self {
            address,
            generation,
            hub,
            incoming,
            messages: Mutex::new((Limiter::bounded(MESSAGES, 1), Instant::now())),
            route,
        }
    }

    /// Whether the listener may send another message now, spending one.
    pub fn allow_message(&self) -> bool {
        let mut messages = match self.messages.lock() {
            Ok(messages) => messages,
            Err(poisoned) => poisoned.into_inner(),
        };
        let now_ms = u64::try_from(messages.1.elapsed().as_millis()).unwrap_or(u64::MAX);
        messages.0.allow((), now_ms)
    }

    /// Replaces the keys allowed to dial; sessions of a key left out end.
    pub fn set_dial_keys(&self, keys: &[Key]) {
        self.hub.set_dial_keys(self.route, self.generation, keys);
    }

    /// Allows `key` one dial within `lifetime`, or says too many are held.
    pub fn add_pairing_key(&self, key: Key, lifetime: Duration) -> bool {
        self.hub
            .add_pairing_key(self.route, self.generation, key, lifetime)
    }
}

impl Drop for Registration {
    fn drop(&mut self) {
        let mut state = self.hub.state();
        let current = state
            .listeners
            .get(&self.route)
            .is_some_and(|listener| listener.generation == self.generation);
        if !current {
            return;
        }
        // The route's dial and pairing keys go with it.
        state.remove_listener(&self.route);
        drop(state);
        self.hub.metrics.lower(Gauge::Listeners);
    }
}

/// A place held for one dial, then its session, on a route.
#[derive(Debug)]
pub struct Slot {
    hub: Arc<Hub>,
    /// The dialer's and listener's addresses, in full.
    pub(super) dialer: IpAddr,
    pub(super) listener: IpAddr,
    pub(super) key: Key,
    pub(super) pairing: bool,
    pub(super) route: RouteId,
}

impl Slot {
    pub(super) fn new(
        hub: Arc<Hub>,
        route: RouteId,
        key: Key,
        pairing: bool,
        (dialer, listener): (IpAddr, IpAddr),
    ) -> Self {
        Self {
            dialer,
            hub,
            listener,
            key,
            pairing,
            route,
        }
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        let mut state = self.hub.state();
        state.slots = state.slots.saturating_sub(1);
        state.dialers.remove(self.dialer);
        state.keys.remove(&self.key);
        if let Some(held) = state.routes.get_mut(&self.route) {
            *held = held.saturating_sub(1);
            if *held == 0 {
                state.routes.remove(&self.route);
            }
        }
    }
}

/// A dial waiting for its listener to accept.
#[derive(Debug)]
pub struct Waiting {
    pub accepted: oneshot::Receiver<Accepted>,
    /// Withdraws the dial when dropped, unless the listener claimed it.
    pub pending: Unclaimed,
    pub slot: Slot,
}

/// A dial the listener has not claimed yet.
#[derive(Debug)]
pub struct Unclaimed {
    hub: Arc<Hub>,
    session: SessionId,
}

impl Unclaimed {
    pub(super) fn new(hub: Arc<Hub>, session: SessionId) -> Self {
        Self { hub, session }
    }

    /// The id the listener accepts with.
    #[must_use]
    pub fn session(&self) -> SessionId {
        self.session
    }
}

impl Drop for Unclaimed {
    fn drop(&mut self) {
        if self.hub.state().pending.remove(&self.session).is_some() {
            self.hub.metrics.lower(Gauge::Pending);
        }
    }
}

/// A spliced session, listed on the dashboard until dropped.
#[derive(Debug)]
pub struct Session {
    hub: Arc<Hub>,
    id: u64,
    /// Fire when the listener revokes the key, and when a side is banned.
    revoke: Arc<Notify>,
    ban: Arc<Notify>,
    slot: Slot,
    pub started: Instant,
    pub traffic: Arc<Traffic>,
}

impl Session {
    pub(super) fn new(
        hub: Arc<Hub>,
        id: u64,
        slot: Slot,
        traffic: Arc<Traffic>,
        [revoke, ban]: [Arc<Notify>; 2],
    ) -> Self {
        Self {
            ban,
            hub,
            id,
            revoke,
            slot,
            started: Instant::now(),
            traffic,
        }
    }

    #[must_use]
    pub fn metrics(&self) -> &Metrics {
        &self.hub.metrics
    }

    #[must_use]
    pub fn hub(&self) -> &Hub {
        &self.hub
    }

    /// The bounds this session runs under: a pairing session is short.
    #[must_use]
    pub fn bounds(&self) -> Bounds {
        let mut bounds = self.hub.limits.bounds();
        if self.slot.pairing {
            bounds.longest = bounds.longest.min(PAIRING_SESSION);
        }
        bounds
    }

    /// Resolves when the listener stops allowing this session's key.
    pub async fn revoked(&self) {
        self.revoke.notified().await;
    }

    /// Resolves when either side's address is banned.
    pub async fn banned(&self) {
        self.ban.notified().await;
    }

    /// The dialer's address, in full.
    #[must_use]
    pub fn dialer(&self) -> IpAddr {
        self.slot.dialer
    }

    /// The listener's address, in full.
    #[must_use]
    pub fn listener(&self) -> IpAddr {
        self.slot.listener
    }

    /// The hub, to count offenses and charge bandwidth.
    #[must_use]
    pub fn hub_arc(&self) -> &Arc<Hub> {
        &self.hub
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let mut state = self.hub.state();
        state.sessions.remove(&self.id);
        if let Some(ids) = state.on_route.get_mut(&self.slot.route) {
            ids.remove(&self.id);
            if ids.is_empty() {
                state.on_route.remove(&self.slot.route);
            }
        }
        drop(state);
        self.hub.metrics.lower(Gauge::Sessions);
    }
}
