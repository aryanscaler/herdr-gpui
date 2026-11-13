//! Tests, and the fixtures they share: signing keys and proofs.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod directory;
mod endpoint;
mod public_hosts;
mod route;
#[cfg(feature = "server")]
mod server;
#[cfg(feature = "signed-directory")]
mod signed_directory;
mod socket;
mod wire;

use crate::{socket::Hello, wire::Key};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use p256::ecdsa::{Signature, SigningKey, signature::Signer};

pub(crate) fn signing_key(seed: u8) -> SigningKey {
    SigningKey::from_slice(&[seed; 32]).unwrap()
}

/// `key`'s public half as the wire carries it.
pub(crate) fn public(key: &SigningKey) -> Key {
    key.verifying_key()
        .to_sec1_point(true)
        .as_bytes()
        .try_into()
        .unwrap()
}

/// `key`'s signature over `message`, `r ‖ s`.
pub(crate) fn sign(key: &SigningKey, message: &[u8]) -> [u8; 64] {
    let signature: Signature = key.sign(message);
    signature.to_bytes().into()
}

/// A WebSocket hello: `key` signing `message`.
pub(crate) fn hello(key: &SigningKey, message: &[u8]) -> Hello {
    Hello {
        key: URL_SAFE_NO_PAD.encode(public(key)),
        signature: URL_SAFE_NO_PAD.encode(sign(key, message)),
        token: None,
    }
}
