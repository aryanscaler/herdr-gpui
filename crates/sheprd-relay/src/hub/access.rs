//! Who may listen on a relay: anyone, by default, so a relay needs no
//! accounts and no setup; or, optionally, holders of a listen token, for a
//! relay kept to its owner's computers. Who may *dial* a computer is the
//! computer's choice (its dial keys), never the operator's.
//!
//! An operator may hand out several tokens, one per person or machine, one
//! per line of the token file (`NAME: TOKEN`, or the token alone; a line
//! starting with `#` is a comment). The file is re-read while the relay runs, and a
//! token taken out of it closes the listeners it admitted, so one person's
//! access is revoked without changing anyone else's.

use crate::{Error, Result, secret::Secret};
use sha2::{Digest, Sha256};
use std::sync::{RwLock, RwLockReadGuard};

/// The shortest listen token the relay accepts.
pub const MIN_TOKEN: usize = 16;
/// The most tokens one relay holds.
pub const MAX_TOKENS: usize = 1_024;

/// Which token admitted a listener: its SHA-256, so the relay can close
/// that listener when the token is revoked without keeping the token
/// itself beside it. `None` on an open relay.
pub type Admitted = Option<[u8; 32]>;

/// Who may listen.
#[derive(Debug, Default)]
pub enum Access {
    /// Anyone who proves a key.
    #[default]
    Open,
    /// Holders of one of the operator's tokens.
    Tokens(RwLock<Vec<Token>>),
}

/// One listen token.
#[derive(Debug)]
pub struct Token {
    digest: [u8; 32],
    secret: Secret,
}

impl Access {
    /// Holders of the tokens in `text`, one per line.
    /// Holders of the tokens in `text`, one per line. At start, a file
    /// with none is refused as a mistake; while running, emptying it
    /// revokes every token ([`Access::replace`]).
    pub fn tokens(text: &str) -> Result<Self> {
        let tokens = parse(text)?;
        if tokens.is_empty() {
            return Err(Error::WeakSecret {
                min: MIN_TOKEN,
                name: "the listen token file",
            });
        }
        Ok(Self::Tokens(RwLock::new(tokens)))
    }

    fn held(&self) -> Option<RwLockReadGuard<'_, Vec<Token>>> {
        match self {
            Self::Open => None,
            Self::Tokens(tokens) => Some(match tokens.read() {
                Ok(tokens) => tokens,
                Err(poisoned) => poisoned.into_inner(),
            }),
        }
    }

    /// Whether `token` may listen, and if so which one it was. Every token
    /// is compared, in constant time each, so the answer takes as long
    /// whichever matched.
    #[must_use]
    pub fn admit(&self, token: Option<&[u8]>) -> Option<Admitted> {
        let Some(tokens) = self.held() else {
            return Some(None);
        };
        let token = token?;
        let mut admitted = None;
        for held in tokens.iter() {
            if held.secret.matches(token) {
                admitted = Some(held.digest);
            }
        }
        admitted.map(Some)
    }

    /// Whether the token with this digest is still held.
    #[must_use]
    pub fn holds(&self, digest: &[u8; 32]) -> bool {
        self.held()
            .is_none_or(|tokens| tokens.iter().any(|token| token.digest == *digest))
    }

    /// Whether listening needs a token.
    #[must_use]
    pub fn requires_token(&self) -> bool {
        matches!(self, Self::Tokens(_))
    }

    /// Replaces the tokens with those in `text`; the digests of those kept,
    /// so listeners a dropped token admitted can be closed. A file with no
    /// tokens revokes them all. On an open relay, or when `text` does not
    /// parse (a short token, say), nothing changes, so a typo never drops
    /// someone.
    pub fn replace(&self, text: &str) -> Result<Vec<[u8; 32]>> {
        let Self::Tokens(tokens) = self else {
            return Ok(Vec::new());
        };
        let parsed = parse(text)?;
        let kept = parsed.iter().map(|token| token.digest).collect();
        match tokens.write() {
            Ok(mut tokens) => *tokens = parsed,
            Err(poisoned) => *poisoned.into_inner() = parsed,
        }
        Ok(kept)
    }
}

/// The tokens in `text`: `NAME: TOKEN` or `TOKEN` per line, `#` lines
/// comments.
/// A short token, or more than [`MAX_TOKENS`], refuses the whole file, so
/// a typo never opens the relay or silently drops someone.
fn parse(text: &str) -> Result<Vec<Token>> {
    let mut tokens = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let token = line
            .split_once(": ")
            .map_or(line, |(_, token)| token)
            .trim();
        if token.chars().count() < MIN_TOKEN {
            return Err(Error::WeakSecret {
                min: MIN_TOKEN,
                name: "a listen token",
            });
        }
        if tokens.len() >= MAX_TOKENS {
            return Err(Error::TooManyTokens { max: MAX_TOKENS });
        }
        tokens.push(Token {
            digest: Sha256::digest(token.as_bytes()).into(),
            secret: Secret::new(token.as_bytes())?,
        });
    }
    Ok(tokens)
}
