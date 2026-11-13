//! What one relay will carry. Every bound has a default safe for a small
//! droplet, so the relay runs with no settings; operators raise or lower
//! them with flags or `RELAY_*` variables (`options`).

use std::time::Duration;

/// How many times an address's share of a limit a wide block (an IPv6
/// /48, an IPv4 /24) may hold together: room for the many users a block
/// can hold, while holding back one caller rotating through its /64s.
pub const WIDE_SHARE: usize = 16;

/// The bounds a relay enforces. A bandwidth of zero means unlimited.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Limits {
    /// How long a dialer waits for the listener to accept.
    pub accept_timeout: Duration,
    /// Dials one address may start per minute; a quarter of it in a burst.
    pub dials_per_minute: u32,
    /// How long nothing may cross a session before it is closed. Peers send
    /// their own keepalives inside the stream.
    pub idle_timeout: Duration,
    /// How long a listener may send nothing, not even a pong or keepalive,
    /// before it is dropped with its keys. Fixed, not a setting: it matches
    /// QUIC's idle timeout.
    pub listener_silence: Duration,
    /// Listens one address may start per minute.
    pub listens_per_minute: u32,
    /// Whether addresses that misbehave are banned automatically (`guard`).
    pub auto_ban: bool,
    /// New QUIC connections one address may start per minute.
    pub connections_per_minute: u32,
    /// Bytes the relay may hold for QUIC peers at once, across every
    /// connection: what peers sent that the far side has not read yet.
    pub max_buffer: u64,
    /// Bytes one address may move in an hour, on either side of its
    /// sessions, before it is banned; 0 for no budget.
    pub max_transfer_per_hour: u64,
    /// Open sockets of every kind.
    pub max_connections: usize,
    /// Open sockets from one IPv4 address or IPv6 /64.
    pub max_connections_per_ip: usize,
    /// The largest WebSocket frame either peer may send, and the most
    /// bytes read from a QUIC stream at once.
    pub max_frame: usize,
    pub max_listeners: usize,
    /// Listeners one IPv4 address or IPv6 /64 may hold.
    pub max_listeners_per_ip: usize,
    /// How long one session may last, however busy.
    pub max_session_duration: Duration,
    pub max_sessions: usize,
    /// Sessions and waiting dials one IPv4 address or IPv6 /64 may hold as
    /// the dialer.
    pub max_sessions_per_ip: usize,
    /// Sessions and waiting dials one dial key may hold: a stolen key
    /// cannot take every slot of its computer.
    pub max_sessions_per_key: usize,
    /// Sessions open at once to one listener.
    pub max_sessions_per_route: usize,
    /// Bytes per second each way within one session.
    pub session_bandwidth: u64,
    /// Bytes per second each way across every session.
    pub total_bandwidth: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            accept_timeout: Duration::from_secs(10),
            dials_per_minute: 120,
            idle_timeout: Duration::from_secs(5 * 60),
            listener_silence: Duration::from_secs(60),
            listens_per_minute: 30,
            auto_ban: true,
            connections_per_minute: 60,
            max_buffer: 512 * 1024 * 1024,
            max_transfer_per_hour: 20 * 1024 * 1024 * 1024,
            max_connections: 10_000,
            max_connections_per_ip: 64,
            max_frame: 64 * 1024,
            max_listeners: 5_000,
            max_listeners_per_ip: 16,
            max_session_duration: Duration::from_secs(24 * 60 * 60),
            max_sessions: 5_000,
            max_sessions_per_ip: 64,
            max_sessions_per_key: 4,
            max_sessions_per_route: 32,
            session_bandwidth: 4 * 1024 * 1024,
            total_bandwidth: 64 * 1024 * 1024,
        }
    }
}

impl Limits {
    /// What each session runs under.
    #[must_use]
    pub fn bounds(&self) -> crate::splice::Bounds {
        crate::splice::Bounds {
            bandwidth: self.session_bandwidth,
            chunk: self.max_frame,
            idle: self.idle_timeout,
            longest: self.max_session_duration,
        }
    }
}

/// Bytes, plain or with a binary suffix: `65536`, `64k`, `4MiB`, `1G`.
#[must_use]
pub fn parse_bytes(text: &str) -> Option<u64> {
    let text = text.trim();
    let digits = text
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(text.len());
    let (number, suffix) = text.split_at(digits);
    let number: u64 = number.parse().ok()?;
    let scale: u64 = match suffix.to_ascii_lowercase().as_str() {
        "" | "b" => 1,
        "k" | "kb" | "kib" => 1 << 10,
        "m" | "mb" | "mib" => 1 << 20,
        "g" | "gb" | "gib" => 1 << 30,
        _ => return None,
    };
    number.checked_mul(scale)
}

/// A duration with a unit: `500ms`, `10s`, `5m`, `24h`, `7d`.
#[must_use]
pub fn parse_duration(text: &str) -> Option<Duration> {
    let text = text.trim();
    let digits = text.find(|c: char| !c.is_ascii_digit())?;
    let (number, unit) = text.split_at(digits);
    let number: u64 = number.parse().ok()?;
    let seconds = |scale: u64| number.checked_mul(scale).map(Duration::from_secs);
    match unit {
        "ms" => Some(Duration::from_millis(number)),
        "s" => seconds(1),
        "m" => seconds(60),
        "h" => seconds(60 * 60),
        "d" => seconds(24 * 60 * 60),
        _ => None,
    }
}
