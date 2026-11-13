//! Taking the relay list only when a known signer vouched for it: the
//! maintainer's OpenPGP key, the site workflow's Ed25519 key, freshness,
//! and replay.

use crate::directory::{
    Directory, Error,
    signed::{MAINTAINER_FINGERPRINT, Signatures, Signer, maintainer, signed_by},
};
use pgp::{
    composed::{Deserializable, SignedPublicKey},
    types::KeyDetails,
};

const SIGNED: &[u8] = include_bytes!("fixtures/relays-signed.json");
const SIGNED_ASC: &[u8] = include_bytes!("fixtures/relays-signed.json.asc");
const SIGNED_SIG: &[u8] = include_bytes!("fixtures/relays-signed.json.sig");
const UNSTAMPED: &[u8] = include_bytes!("fixtures/relays-unstamped.json");
const UNSTAMPED_SIG: &[u8] = include_bytes!("fixtures/relays-unstamped.json.sig");
/// A throwaway key whose signing subkey expires on 2028-11-11.
const TEST_KEY: &str = include_str!("fixtures/openpgp-test.asc");
/// When the fixtures were signed, in Unix seconds.
const SIGNED_AT: u64 = 1_794_506_300;

fn test_key() -> SignedPublicKey {
    SignedPublicKey::from_string(TEST_KEY).unwrap().0
}

fn actions(signature: &[u8]) -> Signatures<'_> {
    Signatures {
        ed25519: Some(signature),
        ..Signatures::default()
    }
}

#[test]
fn the_compiled_in_maintainer_key_is_the_published_one() {
    let key = maintainer().expect("the maintainer key does not match its fingerprint");
    assert!(
        key.primary_key
            .fingerprint()
            .to_string()
            .eq_ignore_ascii_case(MAINTAINER_FINGERPRINT)
    );
    // It can sign: some subkey is bound for signing.
    assert!(
        key.public_subkeys
            .iter()
            .any(|subkey| subkey.signatures.iter().any(|sig| sig.key_flags().sign()))
    );
}

#[test]
fn an_openpgp_signature_holds_only_for_its_bytes_while_its_subkey_lives() {
    let key = test_key();
    assert!(signed_by(&key, SIGNED, SIGNED_ASC, SIGNED_AT));
    let mut tampered = SIGNED.to_vec();
    tampered[20] ^= 1;
    assert!(!signed_by(&key, &tampered, SIGNED_ASC, SIGNED_AT));
    // The signing subkey expired on 2028-11-11.
    assert!(!signed_by(&key, SIGNED, SIGNED_ASC, 1_900_000_000));
    // Not a signature, or not an OpenPGP one.
    assert!(!signed_by(&key, SIGNED, b"nonsense", SIGNED_AT));
    assert!(!signed_by(&key, SIGNED, SIGNED_SIG, SIGNED_AT));
    // Someone else's key does not make it the maintainer's.
    let maintainer = maintainer().unwrap();
    assert!(!signed_by(&maintainer, SIGNED, SIGNED_ASC, SIGNED_AT));
}

#[test]
fn a_list_signed_by_the_site_workflow_is_taken() {
    let signed = Directory::verify(SIGNED, actions(SIGNED_SIG), SIGNED_AT, None).unwrap();
    assert_eq!(signed.signer, Signer::Actions);
    assert_eq!(signed.serial, 5);
    assert!(signed.directory.is_empty());
    assert_eq!(
        Signer::Actions.describe(),
        "GitHub Actions (sheprd-labs/sheprd)"
    );
}

#[test]
fn a_list_no_known_signer_vouched_for_is_refused() {
    let none = Directory::verify(SIGNED, Signatures::default(), SIGNED_AT, None);
    assert!(matches!(none, Err(Error::Unsigned)));
    let mut tampered = SIGNED.to_vec();
    tampered[20] ^= 1;
    let changed = Directory::verify(&tampered, actions(SIGNED_SIG), SIGNED_AT, None);
    assert!(matches!(changed, Err(Error::Unsigned)));
    // The test key's signature is a real one, but not a known signer's.
    let stranger = Signatures {
        openpgp: Some(SIGNED_ASC),
        ..Signatures::default()
    };
    let refused = Directory::verify(SIGNED, stranger, SIGNED_AT, None);
    assert!(matches!(refused, Err(Error::Unsigned)));
    let oversized = vec![b'a'; 64 * 1024];
    let huge = Signatures {
        openpgp: Some(&oversized),
        ed25519: Some(&oversized),
    };
    assert!(matches!(
        Directory::verify(SIGNED, huge, SIGNED_AT, None),
        Err(Error::Unsigned)
    ));
}

#[test]
fn an_old_or_expired_list_cannot_be_replayed() {
    let expired = Directory::verify(SIGNED, actions(SIGNED_SIG), 4_102_444_800, None);
    assert!(matches!(expired, Err(Error::Expired { .. })));
    let stale = Directory::verify(SIGNED, actions(SIGNED_SIG), SIGNED_AT, Some(6));
    assert!(matches!(stale, Err(Error::Stale { serial: 5, kept: 6 })));
    // The same list again is fine: it is fetched every hour.
    assert!(Directory::verify(SIGNED, actions(SIGNED_SIG), SIGNED_AT, Some(5)).is_ok());
}

#[test]
fn a_signed_list_must_say_which_it_is_and_until_when() {
    let unstamped = Directory::verify(UNSTAMPED, actions(UNSTAMPED_SIG), SIGNED_AT, None);
    assert!(matches!(unstamped, Err(Error::Unstamped)));
}

#[test]
fn the_site_workflow_checks_the_same_public_key() {
    let hex = include_str!("../../keys/actions.ed25519.hex").trim();
    let written: String = crate::directory::signed::ACTIONS_KEY
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    assert_eq!(hex, written);
}

#[test]
fn the_published_list_is_stamped_and_any_maintainer_signature_holds() {
    let published = include_bytes!("../../../../website/site/relays.json");
    let directory = Directory::parse(published).unwrap();
    assert!(directory.serial.is_some() && directory.expires.is_some());
    // `just relays-sign` writes relays.json.asc beside it; when it is there
    // it must be the maintainer's, over these exact bytes.
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../website/site/relays.json.asc"
    );
    if let Ok(armored) = std::fs::read(path) {
        let signatures = Signatures {
            openpgp: Some(&armored),
            ..Signatures::default()
        };
        let serial = directory.serial.unwrap();
        let signed = Directory::verify(published, signatures, serial, None).unwrap();
        assert_eq!(signed.signer, Signer::Maintainer);
    }
}

#[test]
fn a_revoked_signing_subkey_vouches_for_nothing() {
    let signature = include_bytes!("fixtures/relays-signed.json.revoked.asc");
    let before = SignedPublicKey::from_string(include_str!("fixtures/openpgp-revoked-before.asc"))
        .unwrap()
        .0;
    let after = SignedPublicKey::from_string(include_str!("fixtures/openpgp-revoked.asc"))
        .unwrap()
        .0;
    // Signed at 1_794_506_816, then the subkey was revoked.
    let now = 1_794_507_000;
    assert!(signed_by(&before, SIGNED, signature, now));
    assert!(!signed_by(&after, SIGNED, signature, now));
}

#[test]
fn a_signature_dated_in_the_future_is_refused() {
    // Signed at SIGNED_AT: a client whose clock reads an hour earlier.
    assert!(!signed_by(
        &test_key(),
        SIGNED,
        SIGNED_ASC,
        SIGNED_AT - 3_600
    ));
}
