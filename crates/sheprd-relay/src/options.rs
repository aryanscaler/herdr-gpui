//! The relay's settings. Every one is optional: a flag, or the same name as
//! a `RELAY_*` variable (`--max-connections` is `RELAY_MAX_CONNECTIONS`),
//! with flags winning. Secrets come only from variables or files, never
//! from flags, so they stay out of the process list.

use crate::{
    Error, Limits, Result,
    limits::{parse_bytes, parse_duration},
};
use secrecy::SecretString;
use std::{
    ffi::{OsStr, OsString},
    fs,
    net::SocketAddr,
    path::PathBuf,
};

pub const USAGE: &str = "\
sheprd-relay: joins end-to-end encrypted streams between Sheprd peers.

Usage: sheprd-relay [OPTIONS]

Every option is optional, and each can also be set as a RELAY_* variable
(--max-connections is RELAY_MAX_CONNECTIONS). Flags win over variables.

  --listen ADDRESS               HTTP: the WebSocket fallback, /v1/info, and the
                                 dashboard (default 127.0.0.1:8080; with only
                                 PORT set, as platforms like DigitalOcean App
                                 Platform do, 0.0.0.0:$PORT)
  --quic ADDRESS|off             QUIC, the main transport, on UDP
                                 (default 127.0.0.1:8443)
  --quic-port PORT               The UDP port clients reach QUIC on, when a
                                 mapping changes it (default: --quic's)
  --quic-identity-dir DIR        Keep QUIC's certificate here, so its pin
                                 survives restarts and can be published in a
                                 signed relay list (default: a new one each
                                 start)
  --metrics ADDRESS|off          Prometheus /metrics (default 127.0.0.1:9090)
  --public-host HOST[:PORT][/PREFIX]
                                 The name clients reach this relay by, as in
                                 their endpoint (relay.example.org). WebSocket
                                 proofs are checked against it, never against a
                                 request's Host header; without it the
                                 WebSocket fallback is off. QUIC needs nothing.
  --behind-proxy                 Take the caller's address from the last entry
                                 of the last X-Forwarded-For line. Only when
                                 nothing reaches the relay but that proxy, and
                                 it appends to the header or replaces it.
  --client-ip-header NAME        Take it from this header instead, which the
                                 platform in front sets to the caller's address:
                                 do-connecting-ip on DigitalOcean App Platform,
                                 cf-connecting-ip behind Cloudflare. Same
                                 condition as --behind-proxy.
  --max-connections N            Open sockets in total (default 10000)
  --max-connections-per-ip N     Per IPv4 address or IPv6 /64 (default 64)
  --max-listeners N              Computers listening (default 5000)
  --max-listeners-per-ip N       Per IPv4 address or IPv6 /64 (default 16);
                                 a /24 or /48 may hold 16 times this
  --max-sessions N               Sessions and waiting dials (default 5000)
  --max-sessions-per-ip N        Per dialing IPv4 address or IPv6 /64
                                 (default 64); a /24 or /48, 16 times this
  --max-sessions-per-key N       Per dial key (default 4)
  --max-sessions-per-route N     Per computer (default 32)
  --max-buffer BYTES             Bytes held for QUIC peers at once, across
                                 connections (default 512MiB)
  --session-bandwidth BYTES      Per session, each way, per second; 0 is
                                 unlimited (default 4MiB)
  --total-bandwidth BYTES        Across sessions, each way, per second; 0 is
                                 unlimited (default 64MiB)
  --max-frame BYTES              Largest frame (default 64KiB)
  --idle-timeout DURATION        Close a silent session (default 5m)
  --max-session-duration DURATION  Close any session after (default 24h)
  --accept-timeout DURATION      Wait for a listener to accept (default 10s)
  --connections-per-minute N     New QUIC connections per address (default 60)
  --dials-per-minute N           Per address (default 120)
  --listens-per-minute N         Per address (default 30)
  --dashboard-token-file FILE    The dashboard's access token, at least 24
                                 characters; or RELAY_DASHBOARD_TOKEN. Without
                                 one, the dashboard is off. Three wrong tries
                                 block an address for 15 minutes.
  --listen-token-file FILE       Only listeners presenting one of these tokens
                                 may listen: one per line, `NAME: TOKEN` or the
                                 token alone, re-read every 30 s, and a token
                                 taken out closes its listeners. Or one token
                                 in RELAY_LISTEN_TOKEN. Dialers never need it.
  --metrics-token-file FILE      Require `Authorization: Bearer TOKEN` on
                                 /metrics; or RELAY_METRICS_TOKEN.
  --auto-ban on|off              Ban addresses that misbehave: limits hit,
                                 forged proofs, malformed messages, probing
                                 (default on). Private and loopback addresses
                                 are never auto-banned.
  --max-transfer-per-hour BYTES  Slow an address sending more than this in an
                                 hour through its sessions, and ban one sending
                                 four times it; 0 is no budget (default 20GiB)
  --ban RANGES                   Ban these by hand, comma-separated:
                                 203.0.113.7,198.51.100.0/24,2001:db8::/32
  --ban-file FILE                Ban the ranges in this file, one per line, #
                                 comments allowed; re-read every 30 s
  --help                         Show this.

BYTES takes k, M, or G (binary): 64k, 4MiB. DURATION takes ms, s, m, h, or d.";

/// What a setting's value is.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Kind {
    Address,
    Bytes,
    Count,
    Duration,
    File,
    Flag,
    Port,
    Text,
}

/// Every setting, once: its flag name and kind. The variable is derived.
pub const SETTINGS: &[(&str, Kind)] = &[
    // keep-sorted start
    ("accept-timeout", Kind::Duration),
    ("auto-ban", Kind::Text),
    ("ban", Kind::Text),
    ("ban-file", Kind::File),
    ("behind-proxy", Kind::Flag),
    ("client-ip-header", Kind::Text),
    ("connections-per-minute", Kind::Count),
    ("dashboard-token-file", Kind::File),
    ("dials-per-minute", Kind::Count),
    ("idle-timeout", Kind::Duration),
    ("listen", Kind::Address),
    ("listen-token-file", Kind::File),
    ("listens-per-minute", Kind::Count),
    ("max-buffer", Kind::Bytes),
    ("max-connections", Kind::Count),
    ("max-connections-per-ip", Kind::Count),
    ("max-frame", Kind::Bytes),
    ("max-listeners", Kind::Count),
    ("max-listeners-per-ip", Kind::Count),
    ("max-session-duration", Kind::Duration),
    ("max-sessions", Kind::Count),
    ("max-sessions-per-ip", Kind::Count),
    ("max-sessions-per-key", Kind::Count),
    ("max-sessions-per-route", Kind::Count),
    ("max-transfer-per-hour", Kind::Bytes),
    ("metrics", Kind::Address),
    ("metrics-token-file", Kind::File),
    ("public-host", Kind::Text),
    ("quic", Kind::Address),
    ("quic-identity-dir", Kind::Text),
    ("quic-port", Kind::Port),
    ("session-bandwidth", Kind::Bytes),
    ("total-bandwidth", Kind::Bytes),
    // keep-sorted end
];

/// Secrets read only from variables, beside their `*_FILE` settings.
const SECRET_VARIABLES: [&str; 3] = [
    "RELAY_DASHBOARD_TOKEN",
    "RELAY_LISTEN_TOKEN",
    "RELAY_METRICS_TOKEN",
];

/// The smallest frame a peer can work with, and the largest one the relay
/// will buffer.
const FRAME_RANGE: std::ops::RangeInclusive<u64> = 1024..=(16 * 1024 * 1024);

/// What the relay was started with. `Debug` redacts the secrets.
#[derive(Clone, Debug)]
pub struct Options {
    /// Ranges banned by hand, from `--ban`.
    pub bans: Vec<crate::guard::Cidr>,
    /// A file of ranges banned by hand, re-read every 30 seconds.
    pub ban_file: Option<PathBuf>,
    pub behind_proxy: bool,
    /// The header the proxy in front puts the caller's address in.
    pub client_ip_header: Option<String>,
    /// The dashboard's access token; without one, the dashboard is off.
    pub dashboard_token: Option<SecretString>,
    pub limits: Limits,
    pub listen: SocketAddr,
    pub listen_token: Option<SecretString>,
    /// Where the listen tokens came from, re-read while the relay runs.
    pub listen_token_file: Option<PathBuf>,
    /// Where `/metrics` is served, or `None` when it is off.
    pub metrics: Option<SocketAddr>,
    pub metrics_token: Option<SecretString>,
    /// The relay's own name, `HOST[:PORT][/PREFIX]` as clients' endpoints give it.
    /// WebSocket proofs are checked against it; without it, the WebSocket
    /// fallback is off.
    pub public_host: Option<String>,
    /// Where QUIC listens, or `None` when it is off.
    pub quic: Option<SocketAddr>,
    /// Where QUIC's certificate is kept across restarts, if anywhere.
    pub quic_identity: Option<PathBuf>,
    /// The port clients reach QUIC on, when it differs from `quic`'s.
    pub quic_port: Option<u16>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            bans: Vec::new(),
            ban_file: None,
            behind_proxy: false,
            client_ip_header: None,
            dashboard_token: None,
            limits: Limits::default(),
            listen: SocketAddr::from(([127, 0, 0, 1], 8080)),
            listen_token: None,
            listen_token_file: None,
            metrics: Some(SocketAddr::from(([127, 0, 0, 1], 9090))),
            metrics_token: None,
            public_host: None,
            quic: Some(SocketAddr::from(([127, 0, 0, 1], 8443))),
            quic_identity: None,
            quic_port: None,
        }
    }
}

/// The variable a setting reads: `max-frame` is `RELAY_MAX_FRAME`.
#[must_use]
pub fn variable(name: &str) -> String {
    format!("RELAY_{}", name.to_ascii_uppercase().replace('-', "_"))
}

impl Options {
    /// The options from `variables`, then `arguments`, or `None` when they
    /// ask for help. `variables` is `std::env::var_os` outside tests.
    pub fn parse(
        arguments: impl IntoIterator<Item = OsString>,
        variables: impl Fn(&str) -> Option<OsString>,
    ) -> Result<Option<Self>> {
        let mut options = Self::default();
        // Platforms that choose the port, such as DigitalOcean App Platform,
        // say so in PORT and route to every interface. RELAY_LISTEN and
        // --listen still win.
        if let Some(port) = variables("PORT") {
            let port = port
                .to_str()
                .and_then(|port| port.parse::<u16>().ok())
                .filter(|port| *port > 0)
                .ok_or(Error::Invalid {
                    name: "PORT",
                    value: port.clone(),
                })?;
            options.listen = SocketAddr::from(([0, 0, 0, 0], port));
        }
        for (name, kind) in SETTINGS {
            if let Some(value) = variables(&variable(name)) {
                options.apply(name, *kind, Some(&value))?;
            }
        }
        for name in SECRET_VARIABLES {
            if let Some(value) = variables(name) {
                options.secret(name, value)?;
            }
        }
        let mut arguments = arguments.into_iter();
        while let Some(argument) = arguments.next() {
            let text = argument.to_str().unwrap_or_default();
            if text == "--help" || text == "-h" {
                return Ok(None);
            }
            let Some((name, kind)) = text
                .strip_prefix("--")
                .and_then(|name| SETTINGS.iter().find(|(known, _)| *known == name))
            else {
                return Err(Error::Argument { argument });
            };
            let value = match kind {
                Kind::Flag => None,
                _ => Some(arguments.next().ok_or(Error::Argument {
                    argument: argument.clone(),
                })?),
            };
            options.apply(name, *kind, value.as_deref())?;
        }
        Ok(Some(options))
    }

    fn secret(&mut self, name: &'static str, value: OsString) -> Result<()> {
        let text = value
            .into_string()
            .map_err(|value| Error::Invalid { name, value })?;
        let text = Some(text.trim())
            .filter(|text| !text.is_empty())
            .map(SecretString::from);
        match name {
            "RELAY_DASHBOARD_TOKEN" => self.dashboard_token = text,
            "RELAY_LISTEN_TOKEN" => self.listen_token = text,
            _ => self.metrics_token = text,
        }
        Ok(())
    }

    /// Sets `name` from `value`, which is `None` only for a flag.
    pub fn apply(&mut self, name: &str, kind: Kind, value: Option<&OsStr>) -> Result<()> {
        let name: &'static str = SETTINGS
            .iter()
            .find(|(known, _)| *known == name)
            .map_or("setting", |(known, _)| known);
        let invalid = || Error::Invalid {
            name,
            value: value.map(OsStr::to_owned).unwrap_or_default(),
        };
        let text = value.and_then(OsStr::to_str);
        if kind == Kind::Flag {
            // As a variable, a flag is on unless it says otherwise.
            let on = text.is_none_or(|text| !matches!(text, "0" | "false" | "no" | ""));
            self.behind_proxy = on;
            return Ok(());
        }
        let text = text.ok_or_else(invalid)?;
        let count = || {
            text.parse::<usize>()
                .ok()
                .filter(|count| *count > 0)
                .ok_or_else(invalid)
        };
        let per_minute = || {
            text.parse::<u32>()
                .ok()
                .filter(|count| *count > 0)
                .ok_or_else(invalid)
        };
        let bytes = || parse_bytes(text).ok_or_else(invalid);
        let duration = || {
            parse_duration(text)
                .filter(|duration| !duration.is_zero())
                .ok_or_else(invalid)
        };
        let limits = &mut self.limits;
        match name {
            "accept-timeout" => limits.accept_timeout = duration()?,
            "auto-ban" => {
                limits.auto_ban = match text.trim() {
                    "on" | "1" | "true" => true,
                    "off" | "0" | "false" => false,
                    _ => return Err(invalid()),
                }
            }
            "ban" => {
                let (ranges, bad) = crate::guard::parse_list(text);
                if !bad.is_empty() {
                    return Err(invalid());
                }
                self.bans = ranges;
            }
            "ban-file" => self.ban_file = Some(PathBuf::from(text)),
            "dashboard-token-file" => self.dashboard_token = Some(read(name, text)?),
            "client-ip-header" => {
                let name = text.trim().to_ascii_lowercase();
                let token = |byte: u8| byte.is_ascii_alphanumeric() || byte == b'-';
                if name.is_empty() || name.len() > 64 || !name.bytes().all(token) {
                    return Err(invalid());
                }
                self.client_ip_header = Some(name);
            }
            "connections-per-minute" => limits.connections_per_minute = per_minute()?,
            "dials-per-minute" => limits.dials_per_minute = per_minute()?,
            "idle-timeout" => limits.idle_timeout = duration()?,
            "listen" => self.listen = text.parse().map_err(|_| invalid())?,
            "listen-token-file" => {
                self.listen_token = Some(read(name, text)?);
                self.listen_token_file = Some(PathBuf::from(text));
            }
            "listens-per-minute" => limits.listens_per_minute = per_minute()?,
            "max-buffer" => limits.max_buffer = bytes()?,
            "max-connections" => limits.max_connections = count()?,
            "max-connections-per-ip" => limits.max_connections_per_ip = count()?,
            "max-frame" => {
                let frame = bytes()?;
                if !FRAME_RANGE.contains(&frame) {
                    return Err(invalid());
                }
                limits.max_frame = usize::try_from(frame).map_err(|_| invalid())?;
            }
            "max-listeners" => limits.max_listeners = count()?,
            "max-listeners-per-ip" => limits.max_listeners_per_ip = count()?,
            "max-session-duration" => limits.max_session_duration = duration()?,
            "max-sessions" => limits.max_sessions = count()?,
            "max-sessions-per-ip" => limits.max_sessions_per_ip = count()?,
            "max-sessions-per-key" => limits.max_sessions_per_key = count()?,
            "max-sessions-per-route" => limits.max_sessions_per_route = count()?,
            "max-transfer-per-hour" => limits.max_transfer_per_hour = bytes()?,
            "metrics" if text == "off" => self.metrics = None,
            "metrics" => self.metrics = Some(text.parse().map_err(|_| invalid())?),
            "metrics-token-file" => self.metrics_token = Some(read(name, text)?),
            "public-host" => self.public_host = Some(public_host(text).ok_or_else(invalid)?),
            "quic" if text == "off" => self.quic = None,
            "quic" => self.quic = Some(text.parse().map_err(|_| invalid())?),
            "quic-identity-dir" => self.quic_identity = Some(PathBuf::from(text)),
            "quic-port" => {
                self.quic_port = Some(
                    text.parse()
                        .ok()
                        .filter(|port| *port > 0)
                        .ok_or_else(invalid)?,
                );
            }
            "session-bandwidth" => limits.session_bandwidth = bytes()?,
            "total-bandwidth" => limits.total_bandwidth = bytes()?,
            _ => return Err(invalid()),
        }
        Ok(())
    }
}

/// `text` as clients sign it: what [`crate::Endpoint::proof_name`] gives for the
/// same address, so `RELAY.EXAMPLE.ORG:443` becomes `relay.example.org`.
#[must_use]
pub fn public_host(text: &str) -> Option<String> {
    let text = text.trim();
    if text.is_empty() || text.contains("://") || text.contains('@') {
        return None;
    }
    crate::Endpoint::parse(&format!("https://{text}"))
        .ok()
        .map(|endpoint| endpoint.proof_name())
}

/// A secret file's text, trimmed; refuses an empty one.
fn read(name: &'static str, path: &str) -> Result<SecretString> {
    let path = PathBuf::from(path);
    let text = fs::read_to_string(&path).map_err(|source| Error::SecretRead {
        path: path.clone(),
        source,
    })?;
    let text = text.trim();
    if text.is_empty() {
        return Err(Error::Invalid {
            name,
            value: path.into_os_string(),
        });
    }
    Ok(SecretString::from(text))
}
