//! The dashboard's login: one access token, a session cookie the relay
//! keeps a record of, and the login form's field.
//!
//! A cookie is 32 random bytes; the relay keeps only their SHA-256 and the
//! session's expiry, in memory and bounded. Signing out forgets it, so a
//! copied cookie stops working then, not 12 hours later, and a restart
//! signs everyone out.
//!
//! The token, not a password, is what keeps guessing hopeless: at least 24
//! characters, and `deploy/relay` generates 32 random bytes. The lockout in
//! `lockout` then stops an address after three wrong tries.

use crate::secret::Secret;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use percent_encoding::percent_decode_str;
use sha2::{Digest, Sha256};
use std::{collections::HashMap, fmt, sync::Mutex, time::Duration};

/// The cookie's name.
pub const COOKIE: &str = "sheprd_relay";
/// How long a login lasts.
pub const SESSION: Duration = Duration::from_secs(12 * 60 * 60);
/// The shortest token the relay accepts. `openssl rand -base64 32` or
/// cloud-init's own gives 43 or more.
pub const MIN_TOKEN: usize = 24;
/// The longest field the login form may carry.
const MAX_FIELD: usize = 256;
/// Sessions kept at once; signing in past this ends the oldest.
pub const MAX_SESSIONS: usize = 32;

/// Who may read the dashboard: whoever holds the token, and the sessions
/// signed in with it.
pub struct Account {
    /// Each session cookie's SHA-256, and when it expires.
    sessions: Mutex<HashMap<[u8; 32], u64>>,
    token: Secret,
}

impl fmt::Debug for Account {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Account(…)")
    }
}

impl Account {
    /// An account for `token`, refusing a short one.
    pub fn new(token: &str) -> crate::Result<Self> {
        if token.chars().count() < MIN_TOKEN {
            return Err(crate::Error::WeakSecret {
                min: MIN_TOKEN,
                name: "the dashboard token",
            });
        }
        Ok(Self {
            sessions: Mutex::default(),
            token: Secret::new(token.as_bytes())?,
        })
    }

    fn sessions(&self) -> std::sync::MutexGuard<'_, HashMap<[u8; 32], u64>> {
        match self.sessions.lock() {
            Ok(sessions) => sessions,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    /// Whether `token` is the dashboard's, in constant time.
    #[must_use]
    pub fn check(&self, token: &str) -> bool {
        self.token.matches(token.as_bytes())
    }

    /// A new session cookie good until `now_secs` plus [`SESSION`].
    pub fn issue(&self, now_secs: u64) -> crate::Result<String> {
        let mut value = [0; 32];
        getrandom::fill(&mut value).map_err(crate::Error::Random)?;
        let value = URL_SAFE_NO_PAD.encode(value);
        let expires = now_secs.saturating_add(SESSION.as_secs());
        let mut sessions = self.sessions();
        sessions.retain(|_, until| *until > now_secs);
        if sessions.len() >= MAX_SESSIONS
            && let Some(oldest) = sessions
                .iter()
                .min_by_key(|(_, until)| **until)
                .map(|(key, _)| *key)
        {
            sessions.remove(&oldest);
        }
        sessions.insert(digest(&value), expires);
        Ok(value)
    }

    /// When the session `value` names expires, if it is still signed in.
    #[must_use]
    pub fn expiry(&self, value: &str, now_secs: u64) -> Option<u64> {
        self.sessions()
            .get(&digest(value))
            .copied()
            .filter(|expires| *expires > now_secs)
    }

    /// Whether `value` names a session still signed in.
    #[must_use]
    pub fn valid(&self, value: &str, now_secs: u64) -> bool {
        self.expiry(value, now_secs).is_some()
    }

    /// Signs the session `value` names out.
    pub fn revoke(&self, value: &str) {
        self.sessions().remove(&digest(value));
    }
}

/// What the relay keeps of a cookie: its SHA-256, compared by lookup of a
/// hash, so neither the cookie nor timing reveals another session's.
fn digest(value: &str) -> [u8; 32] {
    Sha256::digest(value.as_bytes()).into()
}

/// The session cookie's value in a `Cookie` header, if present.
#[must_use]
pub fn cookie(header: &str) -> Option<&str> {
    header.split(';').find_map(|pair| {
        let (name, value) = pair.trim().split_once('=')?;
        (name == COOKIE).then_some(value)
    })
}

/// The `Set-Cookie` value that stores `value`, or clears it when `None`.
#[must_use]
pub fn set_cookie(value: Option<&str>) -> String {
    let attributes = "Path=/dashboard; HttpOnly; Secure; SameSite=Strict";
    match value {
        Some(value) => format!(
            "{COOKIE}={value}; Max-Age={}; {attributes}",
            SESSION.as_secs()
        ),
        None => format!("{COOKIE}=; Max-Age=0; {attributes}"),
    }
}

/// The token from a urlencoded login form.
#[must_use]
pub fn login_form(body: &[u8]) -> Option<String> {
    let body = std::str::from_utf8(body).ok()?;
    let mut token = None;
    for pair in body.split('&') {
        let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
        if value.len() > MAX_FIELD * 3 {
            return None;
        }
        if name == "token" {
            let value = value.replace('+', " ");
            token = Some(
                percent_decode_str(&value)
                    .decode_utf8()
                    .ok()?
                    .trim()
                    .to_owned(),
            );
        }
    }
    token
}
