//! The relay's state: who listens and who may dial them, which dials wait,
//! which sessions run, and how many sockets each address holds. Everything
//! is in memory and bounded by [`Limits`]. Every slot is held by a guard
//! (`guards`) whose `Drop` gives it back, so an early return or a dropped
//! task can never leak one.
//!
//! The relay keeps users apart without accounts: a route belongs to whoever
//! proves its key, only the dial keys its listener names may reach it, and
//! what crosses is end-to-end encrypted.

pub mod access;
mod guards;
mod rates;
pub(crate) mod tally;

pub use access::Access;
pub use guards::{Buffer, Permit, Registration, Session, Slot, Unclaimed, Waiting};

use crate::{
    Limits, RouteId, SessionId,
    bucket::{Outcome, Throttle, source_key},
    guard::Guard,
    metrics::{Direction, Gauge, Kind, Metrics, Offense, Refused},
    splice::Peer,
    wire::{Key, MAX_DIAL_KEYS},
};
use rates::PerAddress;
use serde::Serialize;
use std::{
    collections::{HashMap, HashSet},
    net::IpAddr,
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicU64, Ordering::Relaxed},
    },
    time::{Duration, Instant},
};
use tally::{Counts, Tally};
use tokio::sync::{Notify, mpsc, oneshot};

/// Incoming dials a listener may have unread before more are refused.
const INCOMING_QUEUE: usize = 16;
/// The most sessions the dashboard lists.
const MAX_ROWS: usize = 50;
/// Pairing keys one listener may hold at once: a pairing dialog or two.
pub const MAX_PAIRING_KEYS: usize = 4;
/// The longest a session opened with a pairing key may last.
pub const PAIRING_SESSION: Duration = Duration::from_secs(10 * 60);

/// Everything the relay's handlers share.
#[derive(Debug)]
pub struct Hub {
    pub access: Access,
    /// Wakes every QUIC connection when an address is banned, so the
    /// banned one closes itself; each checks its own address.
    bans: tokio::sync::broadcast::Sender<()>,
    /// New QUIC connections, per address and wide block.
    connects: PerAddress,
    dials: PerAddress,
    /// Bans, earned and manual.
    pub guard: Guard,
    pub limits: Limits,
    listens: PerAddress,
    pub metrics: Metrics,
    next: AtomicU64,
    state: Mutex<State>,
    /// One throttle each way, as `total_bandwidth` promises.
    total: [Option<Throttle>; 2],
}

#[derive(Debug, Default)]
struct State {
    addresses: HashMap<IpAddr, usize>,
    /// Bytes granted to QUIC connections' windows, against `max_buffer`:
    /// in all, and by each address and block.
    buffer: Buffered,
    connections: usize,
    /// Dials and sessions held by each dialing address and block.
    dialers: Tally,
    /// Dials and sessions held with each dial key.
    keys: Counts<Key>,
    listeners: HashMap<RouteId, Listener>,
    /// Listeners held by each address and block.
    listening: Tally,
    /// Each route's open sessions, so a change of its dial keys looks at
    /// those only.
    on_route: HashMap<RouteId, HashSet<u64>>,
    pending: HashMap<SessionId, Pending>,
    /// Dials and sessions held against each route, for its limit.
    routes: HashMap<RouteId, usize>,
    sessions: HashMap<u64, Live>,
    /// Dials and sessions in total.
    slots: usize,
}

/// Bytes of window granted, in all and per caller.
#[derive(Debug, Default)]
struct Buffered {
    blocks: Counts<IpAddr>,
    sources: Counts<IpAddr>,
    total: u64,
}

/// One public address may hold this share of `max_buffer`, and its wide
/// block twice that, so one caller cannot spend everyone's.
const BUFFER_SHARE: u64 = 4;

impl State {
    /// Removes `route`'s listener, giving back its address's count.
    fn remove_listener(&mut self, route: &RouteId) -> Option<Listener> {
        let listener = self.listeners.remove(route)?;
        self.listening.remove(listener.address);
        Some(listener)
    }
}

#[derive(Debug)]
struct Listener {
    /// Where it connects from, in full: bans match it by range, and by
    /// the address limits key it under.
    address: IpAddr,
    dial_keys: HashSet<Key>,
    generation: u64,
    /// The listen token that admitted it, closed with it when revoked.
    token: access::Admitted,
    incoming: mpsc::Sender<Incoming>,
    pairing: Vec<(Key, Instant)>,
}

/// A dial the listener is told about: its id, to accept it over a
/// WebSocket, and the key it was signed with, so the computer knows which
/// device calls before the end-to-end handshake.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Incoming {
    pub key: Key,
    pub session: SessionId,
}

#[derive(Debug)]
struct Pending {
    accepted: oneshot::Sender<Accepted>,
}

/// The listener's side of one dial. A WebSocket accept holds its own
/// connection slot; a QUIC stream shares its connection's, and widens its
/// windows for the session.
#[derive(Debug)]
pub struct Accepted {
    pub peer: Peer,
    pub permit: Option<Permit>,
    pub window: Option<crate::quic::Widened>,
}

#[derive(Debug)]
struct Live {
    /// Ends the session when either side is banned.
    ban: Arc<Notify>,
    /// Both sides' addresses, in full, so a manual ban of one IPv6 host
    /// matches; automatic bans compare the keys limits use.
    dialer: IpAddr,
    key: Key,
    listener: IpAddr,
    pairing: bool,
    revoke: Arc<Notify>,
    route: RouteId,
    started: Instant,
    traffic: Arc<Traffic>,
}

/// Bytes one session has carried each way.
#[derive(Debug, Default)]
pub struct Traffic {
    pub from_dialer: AtomicU64,
    pub from_listener: AtomicU64,
}

/// Why a dial could not wait for its listener.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DialRefusal {
    /// The route or the relay holds as many sessions as it may.
    Busy,
    /// No one listens on the route.
    Offline,
    /// Someone listens, but not for this key. Peers are told `Offline`.
    Unauthorized,
}

/// Why a listener could not register.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ListenRefusal {
    /// The address is banned.
    Banned,
    /// The relay holds as many listeners as it may.
    Full,
    /// The listen token that admitted it was revoked since.
    Revoked,
    /// The address, or its wide block, holds as many as it may.
    TooMany,
}

/// One row of the dashboard's session table.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SessionRow {
    pub bytes_from_dialer: u64,
    pub bytes_from_listener: u64,
    pub pairing: bool,
    pub route: String,
    pub seconds: u64,
}

impl Hub {
    #[must_use]
    pub fn new(limits: Limits, access: Access) -> Self {
        let origin = Instant::now();
        Self {
            access,
            bans: tokio::sync::broadcast::channel(16).0,
            connects: PerAddress::new(limits.connections_per_minute, origin),
            dials: PerAddress::new(limits.dials_per_minute, origin),
            guard: Guard::new(limits.auto_ban, limits.max_transfer_per_hour),
            limits,
            listens: PerAddress::new(limits.listens_per_minute, origin),
            metrics: Metrics::default(),
            next: AtomicU64::new(1),
            state: Mutex::default(),
            total: [
                Throttle::new(limits.total_bandwidth),
                Throttle::new(limits.total_bandwidth),
            ],
        }
    }

    fn state(&self) -> MutexGuard<'_, State> {
        match self.state.lock() {
            Ok(state) => state,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    /// The throttle every session shares, unless bandwidth is unlimited.
    #[must_use]
    pub fn total_throttle(&self, direction: Direction) -> Option<&Throttle> {
        let index = match direction {
            Direction::FromDialer => 0,
            Direction::FromListener => 1,
        };
        self.total[index].as_ref()
    }

    /// Whether `address` may start another dial now.
    pub fn allow_dial(&self, address: IpAddr) -> Outcome {
        self.dials.check(address)
    }

    /// Whether `address` may start another listen now.
    pub fn allow_listen(&self, address: IpAddr) -> Outcome {
        self.listens.check(address)
    }

    /// Whether `address` may start another QUIC connection now.
    pub fn allow_connect(&self, address: IpAddr) -> Outcome {
        self.connects.check(address)
    }

    /// Takes `bytes` from the budget for what QUIC peers may have the relay
    /// hold for `address`, or `None` when that would pass `max_buffer`, or
    /// that address's or block's share of it. A private address, which
    /// only a relay on a private network sees, has only the total.
    pub fn reserve_buffer(self: &Arc<Self>, address: IpAddr, bytes: u64) -> Option<Buffer> {
        let max = self.limits.max_buffer;
        let counted = (!crate::guard::exempt(address)).then_some(address);
        let amount = usize::try_from(bytes).ok()?;
        let mut state = self.state();
        let buffer = &mut state.buffer;
        if buffer.total.saturating_add(bytes) > max {
            return None;
        }
        if let Some(address) = counted {
            let (source, block) = (source_key(address), crate::guard::wide_key(address));
            let share = usize::try_from(max / BUFFER_SHARE).unwrap_or(usize::MAX);
            if buffer.sources.get(&source).saturating_add(amount) > share
                || buffer.blocks.get(&block).saturating_add(amount) > share.saturating_mul(2)
            {
                return None;
            }
            buffer.sources.add_by(source, amount);
            buffer.blocks.add_by(block, amount);
        }
        buffer.total += bytes;
        drop(state);
        Some(Buffer::new(Arc::clone(self), counted, bytes))
    }

    /// Gives back what [`Hub::reserve_buffer`] took.
    fn release_buffer(&self, counted: Option<IpAddr>, bytes: u64) {
        let mut state = self.state();
        let buffer = &mut state.buffer;
        buffer.total = buffer.total.saturating_sub(bytes);
        if let (Some(address), Ok(amount)) = (counted, usize::try_from(bytes)) {
            buffer.sources.remove_by(&source_key(address), amount);
            buffer
                .blocks
                .remove_by(&crate::guard::wide_key(address), amount);
        }
    }

    /// Takes a connection slot for `address`, or says which limit is full.
    pub fn admit(self: &Arc<Self>, address: IpAddr, kind: Kind) -> Result<Permit, Refused> {
        let key = source_key(address);
        let mut state = self.state();
        if state.connections >= self.limits.max_connections {
            drop(state);
            self.metrics.refuse(Refused::Total);
            return Err(Refused::Total);
        }
        let held = state.addresses.get(&key).copied().unwrap_or(0);
        if held >= self.limits.max_connections_per_ip {
            drop(state);
            self.metrics.refuse(Refused::PerAddress);
            self.offend(address, Offense::ConnectionCap);
            return Err(Refused::PerAddress);
        }
        state.connections += 1;
        state.addresses.insert(key, held + 1);
        drop(state);
        self.metrics.raise(Gauge::Connections(kind));
        Ok(Permit::new(Arc::clone(self), key, kind))
    }

    /// Makes this socket the listener for `route`, replacing any other,
    /// unless the relay is full. No one may dial it until it names its dial
    /// keys, and its keys go with it when it leaves.
    pub fn register(
        self: &Arc<Self>,
        route: RouteId,
        address: IpAddr,
    ) -> Result<Registration, ListenRefusal> {
        self.register_with(route, address, None)
    }

    /// [`Hub::register`], for a listener `token` admitted.
    pub fn register_with(
        self: &Arc<Self>,
        route: RouteId,
        address: IpAddr,
        token: access::Admitted,
    ) -> Result<Registration, ListenRefusal> {
        let (sender, incoming) = mpsc::channel(INCOMING_QUEUE);
        let generation = self.next.fetch_add(1, Relaxed);
        let mut state = self.state();
        // Checked under the lock `sweep` takes: a ban either comes
        // first and refuses this listener, or comes after and removes it.
        if self.guard.banned(address, Instant::now()) {
            return Err(ListenRefusal::Banned);
        }
        // Checked under the lock `keep_tokens` takes after the tokens
        // change: a token revoked between its check and here either is
        // refused now, or registers first and is closed by `keep_tokens`.
        if token.is_some_and(|token| !self.access.holds(&token)) {
            return Err(ListenRefusal::Revoked);
        }
        let replaced = state.listeners.get(&route).map(|listener| listener.address);
        if replaced.is_none() && state.listeners.len() >= self.limits.max_listeners {
            return Err(ListenRefusal::Full);
        }
        // The same key from the same address holds its one count; from
        // another, the new address must have room before the old is freed.
        if replaced != Some(address) {
            if !state
                .listening
                .try_add(address, self.limits.max_listeners_per_ip)
            {
                return Err(ListenRefusal::TooMany);
            }
            if let Some(old) = replaced {
                state.listening.remove(old);
            }
        }
        let replacing = replaced.is_some();
        state.listeners.insert(
            route,
            Listener {
                address,
                dial_keys: HashSet::new(),
                generation,
                token,
                incoming: sender,
                pairing: Vec::new(),
            },
        );
        drop(state);
        if !replacing {
            self.metrics.raise(Gauge::Listeners);
        }
        Ok(Registration::new(
            Arc::clone(self),
            (generation, address),
            incoming,
            route,
        ))
    }

    /// Replaces the dial keys of `route`'s listener `generation`, ending
    /// sessions of keys it no longer names. Pairing sessions are left to
    /// end on their own: the computer replaces a pairing key with the new
    /// device's own key while that session is still open.
    fn set_dial_keys(&self, route: RouteId, generation: u64, keys: &[Key]) {
        let mut state = self.state();
        let Some(listener) = state.listeners.get_mut(&route) else {
            return;
        };
        if listener.generation != generation {
            return;
        }
        let keys: HashSet<Key> = keys.iter().copied().take(MAX_DIAL_KEYS).collect();
        if listener.dial_keys == keys {
            return;
        }
        listener.dial_keys = keys;
        let allowed = listener.dial_keys.clone();
        let Some(ids) = state.on_route.get(&route) else {
            return;
        };
        for live in ids.iter().filter_map(|id| state.sessions.get(id)) {
            if !live.pairing && !allowed.contains(&live.key) {
                live.revoke.notify_one();
            }
        }
    }

    /// Allows `key` one dial to `route` within `lifetime`, for pairing.
    fn add_pairing_key(
        &self,
        route: RouteId,
        generation: u64,
        key: Key,
        lifetime: Duration,
    ) -> bool {
        let mut state = self.state();
        let Some(listener) = state.listeners.get_mut(&route) else {
            return false;
        };
        if listener.generation != generation {
            return false;
        }
        let now = Instant::now();
        listener.pairing.retain(|(_, expires)| *expires > now);
        if listener.pairing.len() >= MAX_PAIRING_KEYS {
            return false;
        }
        listener.pairing.push((key, now + lifetime));
        true
    }

    /// Tells `route`'s listener that `key` dials, holding a slot for it, if
    /// the listener allows `key`. A pairing key is used up here.
    pub fn dial(
        self: &Arc<Self>,
        route: RouteId,
        key: Key,
        dialer: IpAddr,
    ) -> crate::Result<Result<Waiting, DialRefusal>> {
        let session = SessionId::random()?;
        let (accepted, receiver) = oneshot::channel();
        let mut state = self.state();
        let Some(listener) = state.listeners.get_mut(&route) else {
            return Ok(Err(DialRefusal::Offline));
        };
        let now = Instant::now();
        listener.pairing.retain(|(_, expires)| *expires > now);
        let pairing = !listener.dial_keys.contains(&key);
        let pairing_index = listener
            .pairing
            .iter()
            .position(|(allowed, _)| *allowed == key);
        if pairing && pairing_index.is_none() {
            return Ok(Err(DialRefusal::Unauthorized));
        }
        let incoming = listener.incoming.clone();
        let listener_address = listener.address;
        let on_route = state.routes.get(&route).copied().unwrap_or(0);
        if on_route >= self.limits.max_sessions_per_route
            || state.slots >= self.limits.max_sessions
            || state.keys.get(&key) >= self.limits.max_sessions_per_key
        {
            return Ok(Err(DialRefusal::Busy));
        }
        if !state
            .dialers
            .try_add(dialer, self.limits.max_sessions_per_ip)
        {
            return Ok(Err(DialRefusal::Busy));
        }
        if incoming.try_send(Incoming { key, session }).is_err() {
            state.dialers.remove(dialer);
            return Ok(Err(DialRefusal::Busy));
        }
        state.keys.add(key);
        if let (true, Some(index), Some(listener)) =
            (pairing, pairing_index, state.listeners.get_mut(&route))
        {
            listener.pairing.swap_remove(index);
        }
        state.pending.insert(session, Pending { accepted });
        state.routes.insert(route, on_route + 1);
        state.slots += 1;
        // Raised before the lock goes, so an `accept` racing in cannot
        // lower it first and leave it a count too high.
        self.metrics.raise(Gauge::Pending);
        drop(state);
        Ok(Ok(Waiting {
            accepted: receiver,
            pending: Unclaimed::new(Arc::clone(self), session),
            slot: Slot::new(
                Arc::clone(self),
                route,
                key,
                pairing,
                (dialer, listener_address),
            ),
        }))
    }

    /// Claims the dial `session`, once: where to hand the accepting socket.
    pub fn accept(&self, session: SessionId) -> Option<oneshot::Sender<Accepted>> {
        let pending = self.state().pending.remove(&session)?;
        self.metrics.lower(Gauge::Pending);
        Some(pending.accepted)
    }

    /// Records a spliced session on `slot`'s route until the guard drops,
    /// unless its key was revoked since the dial was checked: the check and
    /// the record happen under one lock, so a revocation either sees the
    /// session and ends it, or the session never starts. A pairing slot
    /// already spent its one-time key and goes ahead.
    #[must_use]
    pub fn open(self: &Arc<Self>, slot: Slot) -> Option<Session> {
        let id = self.next.fetch_add(1, Relaxed);
        let traffic = Arc::new(Traffic::default());
        let revoke = Arc::new(Notify::new());
        let ban = Arc::new(Notify::new());
        let mut state = self.state();
        // A ban on either side, checked under the lock `sweep` takes,
        // so a session either is refused here or is there to be ended. A
        // listener banned only for running into limits keeps taking dials,
        // as it keeps its route.
        let now = Instant::now();
        if self.guard.banned(slot.dialer, now) || self.guard.evicts(slot.listener, now) {
            return None;
        }
        let allowed = slot.pairing
            || state
                .listeners
                .get(&slot.route)
                .is_some_and(|listener| listener.dial_keys.contains(&slot.key));
        if !allowed {
            return None;
        }
        state.on_route.entry(slot.route).or_default().insert(id);
        state.sessions.insert(
            id,
            Live {
                ban: Arc::clone(&ban),
                dialer: slot.dialer,
                key: slot.key,
                listener: slot.listener,
                pairing: slot.pairing,
                revoke: Arc::clone(&revoke),
                route: slot.route,
                started: Instant::now(),
                traffic: Arc::clone(&traffic),
            },
        );
        drop(state);
        self.metrics.raise(Gauge::Sessions);
        Some(Session::new(
            Arc::clone(self),
            id,
            slot,
            traffic,
            [revoke, ban],
        ))
    }

    /// Counts `offense` against `address`, banning it, and ending its
    /// sessions and listeners, if that was one too many.
    pub fn offend(self: &Arc<Self>, address: IpAddr, offense: Offense) {
        self.metrics.strike(offense);
        if let Some(length) = self.guard.offend(address, offense, Instant::now()) {
            self.banned(offense, length);
        }
    }

    /// Counts `bytes` `address` sent through a session against its hourly
    /// budget, banning it if it overran the budget many times.
    pub fn charge(self: &Arc<Self>, address: IpAddr, bytes: u64) {
        if let Some(length) = self.guard.charge(address, bytes, Instant::now()) {
            self.metrics.strike(Offense::Bandwidth);
            self.banned(Offense::Bandwidth, length);
        }
    }

    /// Told after each ban, to close connections a ban now covers.
    #[must_use]
    pub fn ban_notices(&self) -> tokio::sync::broadcast::Receiver<()> {
        self.bans.subscribe()
    }

    /// Whether `address` is banned now, and so refused any new work.
    #[must_use]
    pub fn is_banned(&self, address: IpAddr) -> bool {
        self.guard.banned(address, Instant::now())
    }

    /// Whether `address`'s ban also ends what it has open
    /// (`guard::severe`).
    #[must_use]
    pub fn is_evicted(&self, address: IpAddr) -> bool {
        self.guard.evicts(address, Instant::now())
    }

    /// Whether `address` is over its hourly transfer budget.
    #[must_use]
    pub fn over_budget(&self, address: IpAddr) -> bool {
        self.guard.over_budget(address, Instant::now())
    }

    /// An address, or its wide block, was just banned: count it and end
    /// what the ban covers. The log line, like every other, names no
    /// address.
    fn banned(&self, offense: Offense, length: Duration) {
        self.metrics.ban(offense);
        tracing::warn!(
            reason = offense.label(),
            minutes = length.as_secs() / 60,
            "an address was banned"
        );
        self.sweep();
    }

    /// Ends every session and listener that a ban now evicts, whatever
    /// banned it: an address, its wide block, or a manual range. A ban
    /// earned by limits alone leaves them be (`guard::severe`). Run after
    /// every ban, so a ban of a whole /48 also ends the work of addresses
    /// in it other than the one that tipped it. Bans are rare, so a full
    /// sweep is cheap; it takes the guard's locks under the state lock, in
    /// the same order `open` and `register` do.
    pub fn sweep(&self) {
        let now = Instant::now();
        let banned = |address: IpAddr| self.guard.evicts(address, now);
        let mut state = self.state();
        for live in state.sessions.values() {
            if banned(live.dialer) || banned(live.listener) {
                live.ban.notify_one();
            }
        }
        let evicted: Vec<RouteId> = state
            .listeners
            .iter()
            .filter(|(_, listener)| banned(listener.address))
            .map(|(route, _)| *route)
            .collect();
        // A removed listener's channel closes, which ends its socket.
        let removed = evicted
            .iter()
            .filter(|route| state.remove_listener(route).is_some())
            .count();
        drop(state);
        for _ in 0..removed {
            self.metrics.lower(Gauge::Listeners);
        }
        // QUIC connections check their own address when told.
        let _ = self.bans.send(());
    }

    /// Closes the listeners a listen token admitted that is not among
    /// `kept` any more: revoking one person's token ends their computers'
    /// listening, and no one else's.
    pub fn keep_tokens(&self, kept: &[[u8; 32]]) {
        let mut state = self.state();
        let revoked: Vec<RouteId> = state
            .listeners
            .iter()
            .filter(|(_, listener)| listener.token.is_some_and(|token| !kept.contains(&token)))
            .map(|(route, _)| *route)
            .collect();
        // The sessions a revoked listener carries end with it, as a ban's
        // do: revoking access ends that access, not only new dials.
        for route in &revoked {
            let ids = state.on_route.get(route).into_iter().flatten();
            for live in ids.filter_map(|id| state.sessions.get(id)) {
                live.revoke.notify_one();
            }
        }
        let removed = revoked
            .iter()
            .filter(|route| state.remove_listener(route).is_some())
            .count();
        drop(state);
        for _ in 0..removed {
            self.metrics.lower(Gauge::Listeners);
        }
    }

    /// The busiest sessions, for the dashboard.
    #[must_use]
    pub fn sessions(&self) -> Vec<SessionRow> {
        let state = self.state();
        let mut rows: Vec<SessionRow> = state
            .sessions
            .values()
            .map(|live| SessionRow {
                bytes_from_dialer: live.traffic.from_dialer.load(Relaxed),
                bytes_from_listener: live.traffic.from_listener.load(Relaxed),
                pairing: live.pairing,
                route: live.route.short(),
                seconds: live.started.elapsed().as_secs(),
            })
            .collect();
        drop(state);
        rows.sort_by_key(|row| std::cmp::Reverse(row.bytes_from_dialer + row.bytes_from_listener));
        rows.truncate(MAX_ROWS);
        rows
    }
}
