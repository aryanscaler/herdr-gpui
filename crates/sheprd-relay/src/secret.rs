//! A secret the operator gave: the dashboard token, the listen token, or
//! the metrics token.

use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;
use std::fmt;

/// Holds a secret and checks guesses against it in constant time. `Debug`
/// never shows it.
#[derive(Clone)]
pub struct Secret {
    /// A key drawn at start, so the comparison below costs the same whatever
    /// the guess and the secret's length.
    key: [u8; 64],
    mac: Vec<u8>,
}

impl Secret {
    pub fn new(secret: &[u8]) -> crate::Result<Self> {
        let mut key = [0; 64];
        getrandom::fill(&mut key).map_err(crate::Error::Random)?;
        let mac = keyed(&key, secret).finalize().into_bytes().to_vec();
        Ok(Self { key, mac })
    }

    /// Whether `guess` is the secret.
    #[must_use]
    pub fn matches(&self, guess: &[u8]) -> bool {
        keyed(&self.key, guess).verify_slice(&self.mac).is_ok()
    }
}

/// HMAC-SHA-256 under a key of one block, which needs no fallible setup.
fn keyed(key: &[u8; 64], bytes: &[u8]) -> Hmac<Sha256> {
    let mut mac = <Hmac<Sha256> as KeyInit>::new(&(*key).into());
    mac.update(bytes);
    mac
}

impl fmt::Debug for Secret {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Secret(…)")
    }
}
