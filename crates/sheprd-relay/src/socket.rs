//! The WebSocket fallback's protocol, for networks that block UDP: the
//! same steps as `wire`, as JSON text frames, then binary frames for the
//! session. Proofs sign a nonce the relay sends and the relay's name as
//! the peer's endpoint gives it ([`crate::Endpoint::proof_name`]), since a
//! WebSocket has no TLS exporter. The relay checks that name against its
//! own configured one, never a request's `Host`, which the client sets: a
//! relay passing another relay's challenge through could otherwise claim
//! its own name.
//!
//! Listener, on `GET /v1/listen`:
//!
//! 1. Relay: `{"t":"challenge","nonce","version"}`.
//! 2. Listener: `{"key","signature","token"?}` over [`listen_signed`].
//! 3. Relay: `{"t":"ready","route"}`, or a close code.
//! 4. Listener, any time: `{"t":"dial_keys","keys":[…]}` or
//!    `{"t":"pairing_key","key","seconds"}`; relay: `{"t":"status","status"}`.
//! 5. Relay, per dial: `{"t":"incoming","session","key"}`; the listener
//!    opens `GET /v1/accept/{session}`.
//!
//! Dialer, on `GET /v1/dial/{route}`:
//!
//! 1. Relay: `{"t":"challenge","nonce","version"}`.
//! 2. Dialer: `{"key","signature"}` over [`dial_signed`].
//! 3. Relay: `{"t":"status","status"}`; after `ok`, binary frames are the
//!    session. A key the listener does not allow gets `offline`.

use crate::{
    RouteId,
    wire::{KEY_LEN, Key, MAX_DIAL_KEYS, MAX_PAIRING_SECS, Message, SIGNATURE_LEN, Status},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};

/// What a listener signs ahead of the host and nonce.
pub const LISTEN_CONTEXT: &[u8] = b"sheprd-relay/listen/v1\0";
/// What a dialer signs ahead of the host, nonce, and route.
pub const DIAL_CONTEXT: &[u8] = b"sheprd-relay/dial/v1\0";
/// The challenge's length in bytes.
pub const NONCE_LEN: usize = 32;
/// The longest host a proof covers.
const MAX_HOST: usize = 255;

/// What the relay sends a listener or dialer.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "t")]
pub enum ToPeer {
    /// Sign this to prove the key.
    Challenge { nonce: String, version: u8 },
    /// A dialer waits: open `/v1/accept/{session}`. `key` is the dialer's.
    Incoming { key: String, session: String },
    /// Listening on `route`.
    Ready { route: String },
    /// The answer to a dial or a listener's message.
    Status { status: Status },
}

/// A peer's proof: the first thing it sends.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Hello {
    /// The SEC1 compressed public key, base64url.
    pub key: String,
    /// `r ‖ s`, base64url.
    pub signature: String,
    /// The listen token, when the relay asks for one. Dialers send none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
}

impl Hello {
    /// The key and signature, when both are well formed.
    #[must_use]
    pub fn decode(&self) -> Option<(Key, [u8; SIGNATURE_LEN])> {
        let key = URL_SAFE_NO_PAD.decode(&self.key).ok()?.try_into().ok()?;
        let signature = URL_SAFE_NO_PAD
            .decode(&self.signature)
            .ok()?
            .try_into()
            .ok()?;
        Some((key, signature))
    }
}

/// What a listener sends after it is ready.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "t")]
pub enum FromListener {
    /// The keys allowed to dial, base64url, replacing the previous set.
    DialKeys { keys: Vec<String> },
    /// A key allowed one dial within `seconds`, for pairing.
    PairingKey { key: String, seconds: u16 },
}

impl FromListener {
    /// The same message as `wire` carries it, when it is within bounds.
    #[must_use]
    pub fn decode(&self) -> Option<Message> {
        let key = |text: &str| -> Option<Key> {
            let bytes = URL_SAFE_NO_PAD.decode(text).ok()?;
            (bytes.len() == KEY_LEN).then(|| bytes.try_into().ok())?
        };
        match self {
            Self::DialKeys { keys } if keys.len() <= MAX_DIAL_KEYS => keys
                .iter()
                .map(|text| key(text))
                .collect::<Option<Vec<_>>>()
                .map(Message::DialKeys),
            Self::DialKeys { .. } => None,
            Self::PairingKey { key: text, seconds } => {
                ((1..=MAX_PAIRING_SECS).contains(seconds)).then_some(())?;
                Some(Message::PairingKey {
                    key: key(text)?,
                    seconds: *seconds,
                })
            }
        }
    }
}

fn proof(context: &[u8], host: &str, nonce: &[u8; NONCE_LEN], extra: &[u8]) -> Vec<u8> {
    let host = &host.as_bytes()[..host.len().min(MAX_HOST)];
    [context, host, &[0], nonce, extra].concat()
}

/// The bytes a listener signs: the context, the relay's name (so a relay
/// cannot pass another relay's challenge through), and the nonce.
#[must_use]
pub fn listen_signed(host: &str, nonce: &[u8; NONCE_LEN]) -> Vec<u8> {
    proof(LISTEN_CONTEXT, host, nonce, &[])
}

/// The bytes a dialer signs to reach `route`.
#[must_use]
pub fn dial_signed(host: &str, nonce: &[u8; NONCE_LEN], route: &RouteId) -> Vec<u8> {
    proof(DIAL_CONTEXT, host, nonce, route.as_bytes())
}
