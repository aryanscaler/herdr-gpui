//! Sheprd's own relays, listed in one static file on sheprd.org, so they
//! can be added or all removed without a release: [`URL`].
//!
//! ```json
//! {
//!   "version": 1,
//!   "serial": 1794000000,
//!   "expires": 1825536000,
//!   "relays": [
//!     { "endpoint": "https://fra1.relay.sheprd.org", "region": "fra1", "name": "Frankfurt",
//!       "quic_pin": "base64url SHA-256 of its QUIC certificate" }
//!   ]
//! }
//! ```
//!
//! An empty `relays` list is a valid answer: no Sheprd relays are
//! available for now. Clients fetch it from the compiled-in [`URL`] only,
//! over HTTPS, and this parser is strict: bounded size and count, `https`
//! endpoints on public hosts only, duplicates dropped, unknown fields
//! ignored so the file can grow.
//!
//! Clients take the list only when it is signed ([`signed`]): whoever
//! controls it chooses where every computer listens, so would learn every
//! computer's address, route, dial keys, and online times, could cut
//! everyone off, and could aim every daemon at a host of their choosing.

mod error;
#[cfg(feature = "signed-directory")]
pub mod signed;

pub use error::Error;

use crate::Endpoint;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::Deserialize;

/// Where Sheprd's relays are listed. Compiled in, so a paired device or
/// a page cannot redirect a client's relay list.
pub const URL: &str = "https://sheprd.org/relays.json";
/// How to run a relay, with the one-click DigitalOcean deploy: where
/// clients link when no Sheprd relays are available, and next to the
/// private relay setting.
pub const SETUP_URL: &str = "https://sheprd.org/docs/relays/";
/// The format this client reads.
pub const VERSION: u32 = 1;
/// The largest list a client accepts.
pub const MAX_BYTES: usize = 64 * 1024;
/// The most relays a list may name.
pub const MAX_RELAYS: usize = 64;
/// The longest region or name.
const MAX_LABEL: usize = 64;

/// One of Sheprd's relays.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Relay {
    pub endpoint: Endpoint,
    /// A short region code, such as `fra1`, when the list gives one.
    pub region: Option<String>,
    /// A name to show, such as `Frankfurt`, when the list gives one.
    pub name: Option<String>,
    /// The SHA-256 of its QUIC certificate, when the relay keeps one across
    /// restarts. Signed with the list, so a client trusts it over the pin
    /// `/v1/info` reports over ordinary HTTPS.
    pub quic_pin: Option<[u8; 32]>,
}

/// Sheprd's relays as the list names them; possibly none.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Directory {
    /// When the list stops being taken, in Unix seconds; a signed list
    /// always has one.
    pub expires: Option<u64>,
    pub relays: Vec<Relay>,
    /// Which list this is: a client refuses one older than the list it
    /// keeps, so an old signed list cannot be replayed. A signed list
    /// always has one.
    pub serial: Option<u64>,
}

#[derive(Deserialize)]
struct Raw {
    #[serde(default)]
    expires: Option<u64>,
    relays: Vec<RawRelay>,
    #[serde(default)]
    serial: Option<u64>,
    version: u32,
}

#[derive(Deserialize)]
struct RawRelay {
    endpoint: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    quic_pin: Option<String>,
    #[serde(default)]
    region: Option<String>,
}

/// A label to show, if it is short, printable, and not blank.
fn label(text: Option<String>, field: &'static str) -> Result<Option<String>, Error> {
    let Some(text) = text.map(|text| text.trim().to_owned()) else {
        return Ok(None);
    };
    if text.is_empty() {
        return Ok(None);
    }
    if text.chars().count() > MAX_LABEL || text.chars().any(char::is_control) {
        return Err(Error::Label { field });
    }
    Ok(Some(text))
}

impl Directory {
    /// Reads the list from `bytes`, as served at [`URL`].
    pub fn parse(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() > MAX_BYTES {
            return Err(Error::TooLarge);
        }
        let raw: Raw = serde_json::from_slice(bytes).map_err(|_| Error::Format)?;
        if raw.version != VERSION {
            return Err(Error::Version {
                version: raw.version,
            });
        }
        if raw.relays.len() > MAX_RELAYS {
            return Err(Error::TooMany);
        }
        let mut relays: Vec<Relay> = Vec::with_capacity(raw.relays.len());
        for (index, entry) in raw.relays.into_iter().enumerate() {
            let endpoint = Endpoint::parse(&entry.endpoint)
                .map_err(|source| Error::Endpoint { index, source })?;
            if !endpoint.is_secure() {
                return Err(Error::Insecure { index });
            }
            // Signed or not, the list never points clients at their own
            // machine or network.
            if !endpoint.is_public() {
                return Err(Error::Private { index });
            }
            if relays.iter().any(|relay| relay.endpoint == endpoint) {
                continue;
            }
            let quic_pin = entry
                .quic_pin
                .map(|text| {
                    let bytes = URL_SAFE_NO_PAD.decode(text).ok()?;
                    <[u8; 32]>::try_from(bytes).ok()
                })
                .map(|pin| pin.ok_or(Error::Pin { index }))
                .transpose()?;
            relays.push(Relay {
                endpoint,
                name: label(entry.name, "name")?,
                quic_pin,
                region: label(entry.region, "region")?,
            });
        }
        Ok(Self {
            expires: raw.expires,
            relays,
            serial: raw.serial,
        })
    }

    /// Whether Sheprd has no relays available for now.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.relays.is_empty()
    }
}
