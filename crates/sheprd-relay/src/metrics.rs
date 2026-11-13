//! Counters for Prometheus and the dashboard. No metric carries a per-user
//! label: no address, route, or session appears in a series.

use serde::Serialize;
use std::{
    fmt::Write as _,
    sync::atomic::{AtomicU64, Ordering::Relaxed},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

/// Declares a label set once: the enum, its `ALL` list, and each label.
macro_rules! labels {
    ($(#[$meta:meta])* $name:ident { $($variant:ident => $label:literal,)+ }) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum $name { $($variant,)+ }

        impl $name {
            pub const ALL: &[Self] = &[$(Self::$variant,)+];

            #[must_use]
            pub const fn label(self) -> &'static str {
                match self { $(Self::$variant => $label,)+ }
            }

            const fn index(self) -> usize {
                self as usize
            }
        }
    };
}

labels! {
    /// What a socket is for.
    Kind {
        Accept => "accept",
        Dial => "dial",
        Listen => "listen",
        Quic => "quic",
    }
}

labels! {
    /// How a dial ended before its session began, or that it began.
    Dial {
        BadRoute => "bad_route",
        Busy => "busy",
        Limited => "limited",
        Offline => "offline",
        Spliced => "spliced",
        Timeout => "timeout",
        Unauthorized => "unauthorized",
    }
}

labels! {
    /// How a listen attempt went.
    Listen {
        BadProof => "bad_proof",
        Forbidden => "forbidden",
        Full => "full",
        Limited => "limited",
        Ready => "ready",
        Replaced => "replaced",
        Timeout => "timeout",
    }
}

labels! {
    /// Why a socket was refused before it opened.
    Refused {
        Banned => "banned",
        PerAddress => "per_address",
        Total => "total",
    }
}

labels! {
    /// Why a session ended.
    End {
        Closed => "closed",
        Expired => "expired",
        Idle => "idle",
        Protocol => "protocol",
        Revoked => "revoked",
        Banned => "banned",
    }
}

labels! {
    /// Which throttle held a frame back; `budget` is a side over its
    /// hourly transfer budget (`guard::SLOW`).
    Throttled {
        Budget => "budget",
        Session => "session",
        Total => "total",
    }
}

labels! {
    /// How a dashboard login went.
    Login {
        Blocked => "blocked",
        Failed => "failed",
        Limited => "limited",
        Succeeded => "succeeded",
    }
}

labels! {
    /// What counts against an address toward a ban (`guard`).
    Offense {
        BadProof => "bad_proof",
        Bandwidth => "bandwidth",
        ConnectionCap => "connection_cap",
        Limited => "limited",
        Malformed => "malformed",
        Probe => "probe",
    }
}

labels! {
    /// Which way bytes crossed a session.
    Direction {
        FromDialer => "from_dialer",
        FromListener => "from_listener",
    }
}

/// Upper bounds of the session duration histogram, in seconds.
const DURATION_BUCKETS: [u64; 8] = [1, 10, 60, 300, 1_800, 3_600, 21_600, 86_400];

/// Every counter and gauge the relay keeps.
#[derive(Debug)]
pub struct Metrics {
    bytes: [AtomicU64; Direction::ALL.len()],
    connections: [AtomicU64; Kind::ALL.len()],
    dials: [AtomicU64; Dial::ALL.len()],
    duration_buckets: [AtomicU64; DURATION_BUCKETS.len()],
    duration_ms_sum: AtomicU64,
    ended: [AtomicU64; End::ALL.len()],
    frames: [AtomicU64; Direction::ALL.len()],
    listeners: AtomicU64,
    listens: [AtomicU64; Listen::ALL.len()],
    bans: [AtomicU64; Offense::ALL.len()],
    logins: [AtomicU64; Login::ALL.len()],
    strikes: [AtomicU64; Offense::ALL.len()],
    pending: AtomicU64,
    refused: [AtomicU64; Refused::ALL.len()],
    sessions: AtomicU64,
    started: Instant,
    started_unix: u64,
    throttled_us: [AtomicU64; Throttled::ALL.len()],
}

impl Default for Metrics {
    fn default() -> Self {
        Self {
            bytes: Default::default(),
            connections: Default::default(),
            dials: Default::default(),
            duration_buckets: Default::default(),
            duration_ms_sum: AtomicU64::default(),
            ended: Default::default(),
            frames: Default::default(),
            listeners: AtomicU64::default(),
            listens: Default::default(),
            bans: Default::default(),
            logins: Default::default(),
            strikes: Default::default(),
            pending: AtomicU64::default(),
            refused: Default::default(),
            sessions: AtomicU64::default(),
            started: Instant::now(),
            started_unix: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |since| since.as_secs()),
            throttled_us: Default::default(),
        }
    }
}

/// A gauge moved up and down in step by its owner.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Gauge {
    Connections(Kind),
    Listeners,
    Pending,
    Sessions,
}

impl Metrics {
    fn gauge(&self, gauge: Gauge) -> &AtomicU64 {
        match gauge {
            Gauge::Connections(kind) => &self.connections[kind.index()],
            Gauge::Listeners => &self.listeners,
            Gauge::Pending => &self.pending,
            Gauge::Sessions => &self.sessions,
        }
    }

    pub fn raise(&self, gauge: Gauge) {
        self.gauge(gauge).fetch_add(1, Relaxed);
    }

    /// Lowers a gauge, never below zero.
    pub fn lower(&self, gauge: Gauge) {
        let _ = self
            .gauge(gauge)
            .try_update(Relaxed, Relaxed, |value| Some(value.saturating_sub(1)));
    }

    #[must_use]
    pub fn read(&self, gauge: Gauge) -> u64 {
        self.gauge(gauge).load(Relaxed)
    }

    pub fn dial(&self, outcome: Dial) {
        self.dials[outcome.index()].fetch_add(1, Relaxed);
    }

    pub fn listen(&self, outcome: Listen) {
        self.listens[outcome.index()].fetch_add(1, Relaxed);
    }

    pub fn refuse(&self, reason: Refused) {
        self.refused[reason.index()].fetch_add(1, Relaxed);
    }

    /// An offense counted against an address.
    pub fn strike(&self, offense: Offense) {
        self.strikes[offense.index()].fetch_add(1, Relaxed);
    }

    /// An address banned for `offense`.
    pub fn ban(&self, offense: Offense) {
        self.bans[offense.index()].fetch_add(1, Relaxed);
    }

    pub fn login(&self, outcome: Login) {
        self.logins[outcome.index()].fetch_add(1, Relaxed);
    }

    pub fn carried(&self, direction: Direction, bytes: usize) {
        self.bytes[direction.index()].fetch_add(bytes as u64, Relaxed);
        self.frames[direction.index()].fetch_add(1, Relaxed);
    }

    pub fn throttled(&self, scope: Throttled, wait: Duration) {
        let micros = u64::try_from(wait.as_micros()).unwrap_or(u64::MAX);
        self.throttled_us[scope.index()].fetch_add(micros, Relaxed);
    }

    /// Records a session that lasted `length` and why it ended.
    pub fn ended(&self, end: End, length: Duration) {
        self.ended[end.index()].fetch_add(1, Relaxed);
        let millis = u64::try_from(length.as_millis()).unwrap_or(u64::MAX);
        self.duration_ms_sum.fetch_add(millis, Relaxed);
        let seconds = length.as_secs_f64();
        for (bucket, bound) in self.duration_buckets.iter().zip(DURATION_BUCKETS) {
            if seconds <= bound as f64 {
                bucket.fetch_add(1, Relaxed);
            }
        }
    }

    #[must_use]
    pub fn uptime(&self) -> Duration {
        self.started.elapsed()
    }

    /// Totals for the dashboard.
    #[must_use]
    pub fn totals(&self) -> Totals {
        let sum = |counters: &[AtomicU64]| counters.iter().map(|value| value.load(Relaxed)).sum();
        Totals {
            bytes_from_dialers: self.bytes[Direction::FromDialer.index()].load(Relaxed),
            bytes_from_listeners: self.bytes[Direction::FromListener.index()].load(Relaxed),
            connections: sum(&self.connections),
            dials: labelled(
                Dial::ALL,
                |outcome| self.dials[outcome.index()].load(Relaxed),
                Dial::label,
            ),
            listeners: self.listeners.load(Relaxed),
            listens: labelled(
                Listen::ALL,
                |outcome| self.listens[outcome.index()].load(Relaxed),
                Listen::label,
            ),
            logins_failed: self.logins[Login::Failed.index()].load(Relaxed),
            pending: self.pending.load(Relaxed),
            refused: sum(&self.refused),
            sessions: self.sessions.load(Relaxed),
            sessions_ended: sum(&self.ended),
            throttled_ms: sum(&self.throttled_us) / 1000,
            uptime_secs: self.uptime().as_secs(),
        }
    }

    /// The Prometheus text exposition, format 0.0.4.
    #[must_use]
    pub fn render(&self) -> String {
        let mut out = String::with_capacity(4096);
        let load = |value: &AtomicU64| value.load(Relaxed);
        family(
            &mut out,
            "sheprd_relay_build_info",
            "gauge",
            "The relay's version.",
        );
        let _ = writeln!(
            out,
            "sheprd_relay_build_info{{version=\"{}\"}} 1",
            env!("CARGO_PKG_VERSION")
        );
        family(
            &mut out,
            "sheprd_relay_start_time_seconds",
            "gauge",
            "When the relay started, in Unix seconds.",
        );
        let _ = writeln!(out, "sheprd_relay_start_time_seconds {}", self.started_unix);
        series(
            &mut out,
            "sheprd_relay_connections",
            "gauge",
            "Open sockets by kind.",
            "kind",
            Kind::ALL,
            Kind::label,
            |kind| load(&self.connections[kind.index()]),
        );
        single(
            &mut out,
            "sheprd_relay_listeners",
            "gauge",
            "Routes with a listener.",
            load(&self.listeners),
        );
        single(
            &mut out,
            "sheprd_relay_pending_dials",
            "gauge",
            "Dials waiting for their listener.",
            load(&self.pending),
        );
        single(
            &mut out,
            "sheprd_relay_sessions",
            "gauge",
            "Spliced sessions open now.",
            load(&self.sessions),
        );
        series(
            &mut out,
            "sheprd_relay_bytes_total",
            "counter",
            "Bytes carried, by direction.",
            "direction",
            Direction::ALL,
            Direction::label,
            |direction| load(&self.bytes[direction.index()]),
        );
        series(
            &mut out,
            "sheprd_relay_frames_total",
            "counter",
            "Frames carried, by direction.",
            "direction",
            Direction::ALL,
            Direction::label,
            |direction| load(&self.frames[direction.index()]),
        );
        series(
            &mut out,
            "sheprd_relay_dials_total",
            "counter",
            "Dials by outcome.",
            "outcome",
            Dial::ALL,
            Dial::label,
            |outcome| load(&self.dials[outcome.index()]),
        );
        series(
            &mut out,
            "sheprd_relay_listens_total",
            "counter",
            "Listen attempts by outcome.",
            "outcome",
            Listen::ALL,
            Listen::label,
            |outcome| load(&self.listens[outcome.index()]),
        );
        series(
            &mut out,
            "sheprd_relay_refused_connections_total",
            "counter",
            "Sockets refused by a connection limit.",
            "limit",
            Refused::ALL,
            Refused::label,
            |reason| load(&self.refused[reason.index()]),
        );
        series(
            &mut out,
            "sheprd_relay_sessions_ended_total",
            "counter",
            "Sessions ended, by reason.",
            "reason",
            End::ALL,
            End::label,
            |end| load(&self.ended[end.index()]),
        );
        series(
            &mut out,
            "sheprd_relay_throttled_seconds_total",
            "counter",
            "Time frames waited on a bandwidth limit.",
            "limit",
            Throttled::ALL,
            Throttled::label,
            |scope| load(&self.throttled_us[scope.index()]) as f64 / 1e6,
        );
        series(
            &mut out,
            "sheprd_relay_strikes_total",
            "counter",
            "Offenses counted against addresses, by kind.",
            "offense",
            Offense::ALL,
            Offense::label,
            |offense| load(&self.strikes[offense.index()]),
        );
        series(
            &mut out,
            "sheprd_relay_bans_total",
            "counter",
            "Addresses banned automatically, by the offense that did it.",
            "offense",
            Offense::ALL,
            Offense::label,
            |offense| load(&self.bans[offense.index()]),
        );
        series(
            &mut out,
            "sheprd_relay_dashboard_logins_total",
            "counter",
            "Dashboard logins by outcome.",
            "outcome",
            Login::ALL,
            Login::label,
            |outcome| load(&self.logins[outcome.index()]),
        );
        family(
            &mut out,
            "sheprd_relay_session_duration_seconds",
            "histogram",
            "How long sessions lasted.",
        );
        for (bucket, bound) in self.duration_buckets.iter().zip(DURATION_BUCKETS) {
            let _ = writeln!(
                out,
                "sheprd_relay_session_duration_seconds_bucket{{le=\"{bound}\"}} {}",
                load(bucket)
            );
        }
        let count: u64 = self.ended.iter().map(load).sum();
        let _ = writeln!(
            out,
            "sheprd_relay_session_duration_seconds_bucket{{le=\"+Inf\"}} {count}"
        );
        let _ = writeln!(
            out,
            "sheprd_relay_session_duration_seconds_sum {}",
            load(&self.duration_ms_sum) as f64 / 1000.0
        );
        let _ = writeln!(out, "sheprd_relay_session_duration_seconds_count {count}");
        out
    }
}

/// What the dashboard shows at a glance.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct Totals {
    pub bytes_from_dialers: u64,
    pub bytes_from_listeners: u64,
    pub connections: u64,
    pub dials: Vec<(&'static str, u64)>,
    pub listeners: u64,
    pub listens: Vec<(&'static str, u64)>,
    pub logins_failed: u64,
    pub pending: u64,
    pub refused: u64,
    pub sessions: u64,
    pub sessions_ended: u64,
    pub throttled_ms: u64,
    pub uptime_secs: u64,
}

fn labelled<L: Copy>(
    all: &[L],
    value: impl Fn(L) -> u64,
    label: fn(L) -> &'static str,
) -> Vec<(&'static str, u64)> {
    all.iter()
        .map(|item| (label(*item), value(*item)))
        .collect()
}

fn family(out: &mut String, name: &str, kind: &str, help: &str) {
    let _ = writeln!(out, "# HELP {name} {help}\n# TYPE {name} {kind}");
}

fn single(out: &mut String, name: &str, kind: &str, help: &str, value: u64) {
    family(out, name, kind, help);
    let _ = writeln!(out, "{name} {value}");
}

#[allow(clippy::too_many_arguments)]
fn series<L: Copy, V: std::fmt::Display>(
    out: &mut String,
    name: &str,
    kind: &str,
    help: &str,
    key: &str,
    all: &[L],
    label: fn(L) -> &'static str,
    value: impl Fn(L) -> V,
) {
    family(out, name, kind, help);
    for item in all {
        let _ = writeln!(out, "{name}{{{key}=\"{}\"}} {}", label(*item), value(*item));
    }
}
