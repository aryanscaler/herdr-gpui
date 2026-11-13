//! The relay's QUIC protocol (ALPN `sheprd-relay/1`): a few fixed-size
//! binary headers, then raw bytes. No codegen and no length-prefixed
//! frames inside a session: a session is one QUIC stream, which already
//! carries ordered, flow-controlled bytes.
//!
//! Every stream a peer opens starts with one request:
//!
//! | Request | Bytes |
//! | --- | --- |
//! | Listen | `0x01`, key (33), signature (64), token length (1), token (0–255) |
//! | Dial | `0x02`, route (32), dial key (33), signature (64) |
//!
//! Keys are SEC1 compressed P-256, signatures `r ‖ s`. The relay answers
//! with one [`Status`] byte. After `Ok` on a dial, the stream is the
//! session.
//!
//! **Who may dial.** A listener names the keys allowed to dial it, on its
//! listen stream, which stays open for as long as it listens:
//!
//! | Message | Bytes |
//! | --- | --- |
//! | Dial keys | `0x20`, count (1, at most [`MAX_DIAL_KEYS`]), keys (33 each) |
//! | Pairing key | `0x21`, key (33), lifetime in seconds (2, big-endian, at most [`MAX_PAIRING_SECS`]) |
//!
//! Dial keys replace the whole set; sessions of a key no longer in it end
//! at once. A pairing key works for one dial before it expires. The relay
//! answers each message with a status byte. Closing the stream stops
//! listening.
//!
//! A dial signed by a key the listener did not name answers
//! [`Status::Offline`], exactly as if no one listened, so a stranger who
//! learns a route cannot tell whether its computer is online, and its dial
//! never reaches the computer.
//!
//! For each accepted dial, the relay opens a stream to the listener that
//! starts with [`INCOMING`] and the dialer's key (33), so the computer
//! knows which device it is before the end-to-end handshake; after that,
//! it is the session.
//!
//! Proofs sign a context and 32 bytes exported from the signer's own TLS
//! session (RFC 5705, label [`EXPORTER_LABEL`]). The export differs on
//! every connection, so a proof cannot be replayed or passed through
//! another relay, and it costs no round trip.

use crate::RouteId;

/// The ALPN protocol id peers and relay agree on.
pub const ALPN: &[u8] = b"sheprd-relay/1";
/// The TLS exporter label for listen and dial proofs.
pub const EXPORTER_LABEL: &[u8] = b"EXPORTER-sheprd-relay-listen";
/// How many exported bytes a proof covers.
pub const EXPORTER_LEN: usize = 32;
/// What a listener signs ahead of the exported bytes.
pub const LISTEN_CONTEXT: &[u8] = b"sheprd-relay/listen/quic/v1\0";
/// What a dialer signs ahead of the exported bytes and the route.
pub const DIAL_CONTEXT: &[u8] = b"sheprd-relay/dial/quic/v1\0";
/// The first byte of a stream the relay opens to a listener.
pub const INCOMING: u8 = 0x10;
/// The most dial keys one listener may name: its paired devices.
pub const MAX_DIAL_KEYS: usize = 64;
/// The longest a pairing key may live.
pub const MAX_PAIRING_SECS: u16 = 600;

/// A SEC1 compressed P-256 public key.
pub type Key = [u8; KEY_LEN];
/// An ECDSA P-256 signature, `r ‖ s`.
pub type Signature = [u8; SIGNATURE_LEN];

const LISTEN: u8 = 0x01;
const DIAL: u8 = 0x02;
const DIAL_KEYS: u8 = 0x20;
const PAIRING_KEY: u8 = 0x21;
pub const KEY_LEN: usize = 33;
pub const SIGNATURE_LEN: usize = 64;
const DIAL_LEN: usize = 1 + 32 + KEY_LEN + SIGNATURE_LEN;
/// The longest request: a listen with a 255-byte token.
pub const MAX_REQUEST: usize = 1 + KEY_LEN + SIGNATURE_LEN + 1 + 255;
/// The longest listener message: a full set of dial keys.
pub const MAX_MESSAGE: usize = 2 + KEY_LEN * MAX_DIAL_KEYS;

/// What a peer asks for on a new stream.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Request {
    /// Listen on the route of `key`, proved by `signature`.
    Listen {
        key: Key,
        signature: Signature,
        token: Vec<u8>,
    },
    /// Reach the listener on `route`, as the holder of `key`.
    Dial {
        key: Key,
        route: RouteId,
        signature: Signature,
    },
}

/// What a listener sends on its listen stream.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Message {
    /// The keys allowed to dial, replacing the previous set.
    DialKeys(Vec<Key>),
    /// A key allowed one dial within `seconds`, for pairing a new device.
    PairingKey { key: Key, seconds: u16 },
}

/// Why bytes are not a request or message.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Malformed {
    /// The first byte names nothing.
    Kind(u8),
    /// More dial keys than [`MAX_DIAL_KEYS`].
    Keys,
    /// A pairing key's lifetime is zero or over [`MAX_PAIRING_SECS`].
    Lifetime,
    /// A token longer than 255 bytes cannot be encoded.
    Token,
}

/// How far a buffer gets toward a whole request or message.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Parse<T> {
    /// The whole thing, and how many bytes it took.
    Done(T, usize),
    /// At least this many bytes in total are needed.
    Need(usize),
}

fn array<const N: usize>(bytes: &[u8]) -> [u8; N] {
    let mut fixed = [0; N];
    fixed.copy_from_slice(bytes);
    fixed
}

impl Request {
    /// Encodes the request as it goes on the wire.
    pub fn encode(&self) -> Result<Vec<u8>, Malformed> {
        match self {
            Self::Listen {
                key,
                signature,
                token,
            } => {
                let length = u8::try_from(token.len()).map_err(|_| Malformed::Token)?;
                let mut bytes = Vec::with_capacity(1 + KEY_LEN + SIGNATURE_LEN + 1 + token.len());
                bytes.push(LISTEN);
                bytes.extend_from_slice(key);
                bytes.extend_from_slice(signature);
                bytes.push(length);
                bytes.extend_from_slice(token);
                Ok(bytes)
            }
            Self::Dial {
                key,
                route,
                signature,
            } => {
                let mut bytes = Vec::with_capacity(DIAL_LEN);
                bytes.push(DIAL);
                bytes.extend_from_slice(route.as_bytes());
                bytes.extend_from_slice(key);
                bytes.extend_from_slice(signature);
                Ok(bytes)
            }
        }
    }

    /// Reads a request from the start of `bytes`, or says how many bytes
    /// it needs. Never asks for more than [`MAX_REQUEST`].
    pub fn parse(bytes: &[u8]) -> Result<Parse<Self>, Malformed> {
        let Some(&kind) = bytes.first() else {
            return Ok(Parse::Need(1));
        };
        match kind {
            DIAL => {
                let Some(fields) = bytes.get(1..DIAL_LEN) else {
                    return Ok(Parse::Need(DIAL_LEN));
                };
                let (route, rest) = fields.split_at(32);
                let (key, signature) = rest.split_at(KEY_LEN);
                Ok(Parse::Done(
                    Self::Dial {
                        key: array(key),
                        route: RouteId::from_bytes(array(route)),
                        signature: array(signature),
                    },
                    DIAL_LEN,
                ))
            }
            LISTEN => {
                let header = 1 + KEY_LEN + SIGNATURE_LEN + 1;
                let Some(&length) = bytes.get(header - 1) else {
                    return Ok(Parse::Need(header));
                };
                let total = header + usize::from(length);
                let Some(token) = bytes.get(header..total) else {
                    return Ok(Parse::Need(total));
                };
                Ok(Parse::Done(
                    Self::Listen {
                        key: array(&bytes[1..=KEY_LEN]),
                        signature: array(&bytes[1 + KEY_LEN..header - 1]),
                        token: token.to_vec(),
                    },
                    total,
                ))
            }
            other => Err(Malformed::Kind(other)),
        }
    }
}

impl Message {
    /// Encodes the message as it goes on the wire.
    pub fn encode(&self) -> Result<Vec<u8>, Malformed> {
        match self {
            Self::DialKeys(keys) => {
                if keys.len() > MAX_DIAL_KEYS {
                    return Err(Malformed::Keys);
                }
                let mut bytes = Vec::with_capacity(2 + KEY_LEN * keys.len());
                bytes.push(DIAL_KEYS);
                bytes.push(u8::try_from(keys.len()).map_err(|_| Malformed::Keys)?);
                keys.iter().for_each(|key| bytes.extend_from_slice(key));
                Ok(bytes)
            }
            Self::PairingKey { key, seconds } => {
                if *seconds == 0 || *seconds > MAX_PAIRING_SECS {
                    return Err(Malformed::Lifetime);
                }
                let mut bytes = Vec::with_capacity(1 + KEY_LEN + 2);
                bytes.push(PAIRING_KEY);
                bytes.extend_from_slice(key);
                bytes.extend_from_slice(&seconds.to_be_bytes());
                Ok(bytes)
            }
        }
    }

    /// Reads a message from the start of `bytes`, or says how many bytes
    /// it needs. Never asks for more than [`MAX_MESSAGE`].
    pub fn parse(bytes: &[u8]) -> Result<Parse<Self>, Malformed> {
        let Some(&kind) = bytes.first() else {
            return Ok(Parse::Need(1));
        };
        match kind {
            DIAL_KEYS => {
                let Some(&count) = bytes.get(1) else {
                    return Ok(Parse::Need(2));
                };
                let count = usize::from(count);
                if count > MAX_DIAL_KEYS {
                    return Err(Malformed::Keys);
                }
                let total = 2 + KEY_LEN * count;
                let Some(keys) = bytes.get(2..total) else {
                    return Ok(Parse::Need(total));
                };
                let keys = keys.as_chunks::<KEY_LEN>().0.to_vec();
                Ok(Parse::Done(Self::DialKeys(keys), total))
            }
            PAIRING_KEY => {
                let total = 1 + KEY_LEN + 2;
                let Some(fields) = bytes.get(1..total) else {
                    return Ok(Parse::Need(total));
                };
                let seconds = u16::from_be_bytes([fields[KEY_LEN], fields[KEY_LEN + 1]]);
                if seconds == 0 || seconds > MAX_PAIRING_SECS {
                    return Err(Malformed::Lifetime);
                }
                Ok(Parse::Done(
                    Self::PairingKey {
                        key: array(&fields[..KEY_LEN]),
                        seconds,
                    },
                    total,
                ))
            }
            other => Err(Malformed::Kind(other)),
        }
    }
}

/// The relay's one-byte answer to a request or message.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
#[repr(u8)]
pub enum Status {
    // Wire values: never renumber.
    Ok = 0,
    /// No one listens on the route, or not for this dial key.
    Offline = 1,
    /// The route or the relay holds as many sessions as it may.
    Busy = 2,
    /// The listener did not accept in time.
    Timeout = 3,
    /// Too many requests from this address.
    Limited = 4,
    /// A proof did not hold.
    Refused = 5,
    /// The listen token was missing or wrong.
    Forbidden = 6,
    /// The relay, or this user, holds as many listeners as it may.
    Full = 7,
    /// The request could not be read.
    Malformed = 8,
    /// More pairing keys at once than allowed.
    TooMany = 9,
}

impl Status {
    #[must_use]
    pub const fn from_byte(byte: u8) -> Option<Self> {
        Some(match byte {
            0 => Self::Ok,
            1 => Self::Offline,
            2 => Self::Busy,
            3 => Self::Timeout,
            4 => Self::Limited,
            5 => Self::Refused,
            6 => Self::Forbidden,
            7 => Self::Full,
            8 => Self::Malformed,
            9 => Self::TooMany,
            _ => return None,
        })
    }
}

/// QUIC application error codes the relay closes connections and resets
/// streams with.
pub mod code {
    /// A normal end: the other side of the session finished.
    pub const DONE: u32 = 0;
    /// The other side of the session went away.
    pub const PEER_GONE: u32 = 1;
    /// Nothing crossed the session for the idle timeout.
    pub const IDLE: u32 = 2;
    /// The session reached its longest allowed length.
    pub const EXPIRED: u32 = 3;
    /// A peer broke the protocol.
    pub const PROTOCOL: u32 = 4;
    /// Another connection proved the same key and took over the route.
    pub const REPLACED: u32 = 5;
    /// The relay is shutting down.
    pub const SHUTDOWN: u32 = 6;
    /// The listener stopped allowing the dial key this session used.
    pub const REVOKED: u32 = 7;
    /// One side's address was banned for misbehaving.
    pub const BANNED: u32 = 8;
}

/// The bytes a listener signs on a connection whose TLS exporter gave
/// `exported`.
#[must_use]
pub fn listen_signed(exported: &[u8; EXPORTER_LEN]) -> Vec<u8> {
    [LISTEN_CONTEXT, exported].concat()
}

/// The bytes a dialer signs to reach `route`, on a connection whose TLS
/// exporter gave `exported`.
#[must_use]
pub fn dial_signed(exported: &[u8; EXPORTER_LEN], route: &RouteId) -> Vec<u8> {
    [DIAL_CONTEXT, exported, route.as_bytes()].concat()
}
