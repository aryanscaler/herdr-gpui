//! QUIC's identity kept across restarts, so its pin can be published in
//! the signed relay list, and the relay's own name with a path prefix.

use crate::{options::public_host, quic::tls::Identity};

fn scratch(name: &str) -> std::path::PathBuf {
    let directory = std::env::temp_dir().join(format!(
        "sheprd-relay-identity-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(&directory).unwrap();
    directory
}

#[test]
fn a_kept_identity_has_the_same_pin_after_a_restart() {
    let directory = scratch("kept");
    let first = Identity::kept(&directory).unwrap();
    let second = Identity::kept(&directory).unwrap();
    assert_eq!(first.pin, second.pin);
    assert_ne!(first.pin, Identity::generate().unwrap().pin);
    // The key is this user's alone.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = std::fs::metadata(directory.join("quic-key.der"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o077, 0, "{mode:o}");
    }
    assert!(second.server_crypto().is_ok());
    std::fs::remove_dir_all(&directory).unwrap();
}

#[test]
fn an_unreadable_identity_is_an_error_not_a_new_pin() {
    let directory = scratch("unreadable");
    // A directory where the key file should be.
    std::fs::create_dir(directory.join("quic-key.der")).unwrap();
    assert!(matches!(
        Identity::kept(&directory),
        Err(crate::quic::Error::Identity { .. })
    ));
    std::fs::remove_dir_all(&directory).unwrap();
}

#[test]
fn the_public_name_may_carry_a_path_prefix() {
    assert_eq!(
        public_host("Relay.Example.org:443/sheprd/").as_deref(),
        Some("relay.example.org/sheprd")
    );
    assert_eq!(
        public_host("relay.example.org").as_deref(),
        Some("relay.example.org")
    );
    assert_eq!(public_host("https://relay.example.org"), None);
    assert_eq!(public_host("user@relay.example.org"), None);
}
