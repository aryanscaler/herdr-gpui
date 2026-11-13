//! Routes, sessions, and the proof a listener gives.
//!
//! A route is named by the SHA-256 of a P-256 public key, so only the holder
//! of the private key can listen on it. The key is the computer's relay
//! identity, separate from its end-to-end keys: the relay learns nothing
//! that would let it open the stream.

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use p256::ecdsa::{Signature, VerifyingKey, signature::Verifier};
use sha2::{Digest, Sha256};
use std::fmt;

/// A listener's address on the relay: SHA-256 of its public key.
#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub struct RouteId([u8; 32]);

impl RouteId {
    /// The route `key` (a SEC1 P-256 public key) listens on.
    #[must_use]
    pub fn of(key: &VerifyingKey) -> Self {
        Self(Sha256::digest(key.to_sec1_point(true).as_bytes()).into())
    }

    #[must_use]
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Reads a route from its URL form, 43 base64url characters.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        if text.len() != 43 {
            return None;
        }
        let bytes = URL_SAFE_NO_PAD.decode(text).ok()?;
        bytes.try_into().ok().map(Self)
    }

    /// The first eight characters, for the dashboard: enough to tell routes
    /// apart, too few to dial one.
    #[must_use]
    pub fn short(&self) -> String {
        let mut text = self.to_string();
        text.truncate(8);
        text
    }
}

impl fmt::Display for RouteId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&URL_SAFE_NO_PAD.encode(self.0))
    }
}

/// Route ids stay out of logs; `Debug` shows only the short form.
impl fmt::Debug for RouteId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "RouteId({}…)", self.short())
    }
}

/// One dial waiting for its listener, named by 128 random bits that only the
/// listener learns.
#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub struct SessionId([u8; 16]);

impl SessionId {
    /// A fresh id.
    pub fn random() -> crate::Result<Self> {
        let mut bytes = [0; 16];
        getrandom::fill(&mut bytes).map_err(crate::Error::Random)?;
        Ok(Self(bytes))
    }

    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        if text.len() != 22 {
            return None;
        }
        let bytes = URL_SAFE_NO_PAD.decode(text).ok()?;
        bytes.try_into().ok().map(Self)
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&URL_SAFE_NO_PAD.encode(self.0))
    }
}

impl fmt::Debug for SessionId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SessionId(…)")
    }
}

/// Why a proof was refused.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Refusal {
    Key,
    Signature,
}

/// The route `key` (SEC1 P-256) listens on, if `signature` (`r ‖ s`) holds
/// over `message`. Both transports' proofs end here.
pub fn prove(key: &[u8], signature: &[u8], message: &[u8]) -> Result<RouteId, Refusal> {
    let key = VerifyingKey::from_sec1_bytes(key).map_err(|_| Refusal::Key)?;
    let signature = Signature::from_slice(signature).map_err(|_| Refusal::Signature)?;
    key.verify(message, &signature)
        .map_err(|_| Refusal::Signature)?;
    Ok(RouteId::of(&key))
}
