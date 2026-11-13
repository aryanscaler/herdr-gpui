//! Where a relay is: the default ones Sheprd ships or a private one the
//! user runs. Parsed once at the boundary (a setting, a pairing payload),
//! then used to build the relay's URLs.
//!
//! Accepted forms, all normalized to `https://HOST[:PORT][/PREFIX/]`:
//!
//! - `relay.example.org` or `relay.example.org:8443` (https implied)
//! - `https://relay.example.org/sheprd`, behind a proxy that strips the
//!   prefix
//! - `wss://relay.example.org`, the same as `https://`
//! - `http://` or `ws://` only for this computer, a private address, or a
//!   `.local` name. Peers' content is end-to-end encrypted either way, but
//!   plain HTTP would expose the listen token and metadata on the way.

mod error;

pub use error::Error;

use crate::{RouteId, SessionId};
use serde::{Deserialize, Serialize};
use std::{fmt, net::IpAddr, str::FromStr};
use url::{Host, Url};

/// One relay's base address.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Endpoint {
    /// `https` or `http`, with a path ending in `/`.
    base: Url,
}

impl Endpoint {
    /// Reads `text` as a relay address.
    pub fn parse(text: &str) -> Result<Self, Error> {
        let text = text.trim();
        let mut base = if text.contains("://") {
            Url::parse(text)?
        } else {
            Url::parse(&format!("https://{text}"))?
        };
        let secure = match base.scheme() {
            "https" | "wss" => true,
            "http" | "ws" => false,
            scheme => {
                return Err(Error::Scheme {
                    scheme: scheme.to_owned(),
                });
            }
        };
        let local = match base.host() {
            None => return Err(Error::NoHost),
            Some(Host::Domain(name)) => {
                let name = name.trim_end_matches('.').to_ascii_lowercase();
                name == "localhost" || name.ends_with(".localhost") || name.ends_with(".local")
            }
            Some(Host::Ipv4(address)) => private(IpAddr::V4(address)),
            Some(Host::Ipv6(address)) => private(IpAddr::V6(address)),
        };
        if !secure && !local {
            return Err(Error::Insecure);
        }
        if !base.username().is_empty() || base.password().is_some() {
            return Err(Error::Credentials);
        }
        if base.query().is_some() || base.fragment().is_some() {
            return Err(Error::Extra);
        }
        // `set_scheme` refuses only special-to-non-special changes, which
        // these are not, so the base always ends up http or https.
        let scheme = if secure { "https" } else { "http" };
        if base.set_scheme(scheme).is_err() {
            return Err(Error::Scheme {
                scheme: base.scheme().to_owned(),
            });
        }
        if !base.path().ends_with('/') {
            let path = format!("{}/", base.path());
            base.set_path(&path);
        }
        Ok(Self { base })
    }

    /// Whether traffic to the relay itself is encrypted (always, except on
    /// a local network).
    #[must_use]
    pub fn is_secure(&self) -> bool {
        self.base.scheme() == "https"
    }

    /// Whether the relay's host is one the public internet reaches: not
    /// this computer, a private, link-local, shared (CGNAT), or reserved
    /// address, a `.local` or `.internal` name, nor a name such as
    /// `10.0.0.5.sslip.io` that spells out one. It cannot see where a
    /// public-looking name resolves: clients check that before connecting.
    #[must_use]
    pub fn is_public(&self) -> bool {
        match self.base.host() {
            None => false,
            Some(Host::Ipv4(address)) => public(IpAddr::V4(address)),
            Some(Host::Ipv6(address)) => public(IpAddr::V6(address)),
            Some(Host::Domain(name)) => public_name(name),
        }
    }

    /// The host alone, to resolve for QUIC; IPv6 without brackets.
    #[must_use]
    pub fn hostname(&self) -> String {
        match self.base.host() {
            Some(Host::Ipv6(address)) => address.to_string(),
            Some(host) => host.to_string(),
            None => String::new(),
        }
    }

    /// The `Host` its WebSocket client sends.
    #[must_use]
    pub fn host(&self) -> String {
        let host = self.base.host_str().unwrap_or_default();
        match self.base.port() {
            Some(port) => format!("{host}:{port}"),
            None => host.to_owned(),
        }
    }

    /// The name WebSocket proofs sign over: the host, and the path prefix
    /// when there is one (`relay.example.org/sheprd`). Two relays under one
    /// host on different prefixes then have different names, so one cannot
    /// pass the other's challenge through.
    #[must_use]
    pub fn proof_name(&self) -> String {
        let prefix = self.base.path().trim_end_matches('/');
        format!("{}{prefix}", self.host())
    }

    /// `GET`, for checking a relay before using it: `/v1/info`.
    #[must_use]
    pub fn info(&self) -> Url {
        self.http("v1/info")
    }

    /// The listener's control socket.
    #[must_use]
    pub fn listen(&self) -> Url {
        self.socket("v1/listen")
    }

    /// The socket that dials `route`.
    #[must_use]
    pub fn dial(&self, route: &RouteId) -> Url {
        self.socket(&format!("v1/dial/{route}"))
    }

    /// The listener's socket for one dial.
    #[must_use]
    pub fn accept(&self, session: &SessionId) -> Url {
        self.socket(&format!("v1/accept/{session}"))
    }

    fn http(&self, path: &str) -> Url {
        let mut url = self.base.clone();
        let joined = format!("{}{path}", url.path());
        url.set_path(&joined);
        url
    }

    fn socket(&self, path: &str) -> Url {
        let mut url = self.http(path);
        let scheme = if self.is_secure() { "wss" } else { "ws" };
        // http to ws is a change between special schemes, which `Url` allows.
        let _ = url.set_scheme(scheme);
        url
    }
}

/// Loopback, private, link-local, and unique local addresses.
fn private(address: IpAddr) -> bool {
    match address.to_canonical() {
        IpAddr::V4(address) => {
            address.is_loopback() || address.is_private() || address.is_link_local()
        }
        IpAddr::V6(address) => {
            address.is_loopback() || address.is_unique_local() || address.is_unicast_link_local()
        }
    }
}

/// Whether the public internet routes to `address`: unicast, and not
/// loopback, private, link-local, shared (100.64.0.0/10), documentation,
/// or otherwise reserved.
#[must_use]
pub fn public(address: IpAddr) -> bool {
    match address.to_canonical() {
        IpAddr::V4(address) => {
            let [first, second, ..] = address.octets();
            !(address.is_unspecified()
                || address.is_loopback()
                || address.is_private()
                || address.is_link_local()
                || address.is_broadcast()
                || address.is_documentation()
                || address.is_multicast()
                || first == 0
                || first >= 240
                || (first == 100 && (64..128).contains(&second))
                || (first == 192 && second == 0 && address.octets()[2] == 0)
                || (first == 198 && (18..20).contains(&second)))
        }
        IpAddr::V6(address) => {
            let segments = address.segments();
            !(address.is_unspecified()
                || address.is_loopback()
                || address.is_multicast()
                || address.is_unique_local()
                || address.is_unicast_link_local()
                || (segments[0] == 0x2001 && segments[1] == 0x0db8)
                || segments[0] & 0xe000 != 0x2000)
        }
    }
}

/// Whether a host name could be public: not a local or internal name, and
/// not one that spells out an address that is not public, as wildcard DNS
/// services such as sslip.io and nip.io resolve them (`10.0.0.5.sslip.io`,
/// `10-0-0-5.nip.io`).
fn public_name(name: &str) -> bool {
    let name = name.trim_end_matches('.').to_ascii_lowercase();
    let local = [
        "localhost",
        "local",
        "internal",
        "lan",
        "home.arpa",
        "intranet",
    ];
    if local
        .iter()
        .any(|suffix| name == *suffix || name.ends_with(&format!(".{suffix}")))
    {
        return false;
    }
    let labels: Vec<&str> = name.split('.').collect();
    // Four dotted labels anywhere, or a label ending in four dashed parts,
    // read as IPv4.
    let dotted = labels
        .windows(4)
        .filter_map(|window| window.join(".").parse::<std::net::Ipv4Addr>().ok());
    let dashed = labels.iter().filter_map(|label| {
        let parts: Vec<&str> = label.split('-').collect();
        let tail = parts.get(parts.len().checked_sub(4)?..)?;
        tail.join(".").parse::<std::net::Ipv4Addr>().ok()
    });
    dotted
        .chain(dashed)
        .all(|address| public(IpAddr::V4(address)))
}

impl fmt::Display for Endpoint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = self.base.as_str();
        // Shown and stored without the trailing slash of an empty path.
        formatter.write_str(text.strip_suffix('/').unwrap_or(text))
    }
}

impl FromStr for Endpoint {
    type Err = Error;

    fn from_str(text: &str) -> Result<Self, Error> {
        Self::parse(text)
    }
}

impl Serialize for Endpoint {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Endpoint {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text).map_err(serde::de::Error::custom)
    }
}

/// What `GET /v1/info` answers, so a client can check an endpoint before
/// saving it, and learn how to reach it over QUIC.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Info {
    /// Whether listeners must present the operator's token. Dialers never
    /// need one.
    pub listen_token: bool,
    /// The relay protocol version, 1 today.
    pub protocol: u8,
    /// Where QUIC answers, when it does. Without it, clients use the
    /// WebSocket fallback.
    #[serde(default)]
    pub quic: Option<QuicInfo>,
    /// The relay's build.
    pub version: String,
    /// Whether the WebSocket fallback answers: only on a relay that knows
    /// its own public name.
    #[serde(default)]
    pub websocket: bool,
}

/// The relay's QUIC side: the UDP port on the endpoint's host, and the
/// SHA-256 of the certificate to pin, base64url. The certificate is made
/// at each start, so a client that fails the handshake reads this again.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct QuicInfo {
    pub pin: String,
    pub port: u16,
}

/// The relay protocol version this crate speaks.
pub const PROTOCOL: u8 = 1;
