//! A host without a Herdr 1 that can serve the window: none installed here,
//! none a saved host's bridge found, or one too old, until one answers.

use super::*;
use sheprd_client::SshFailure;

#[test]
fn a_saved_host_is_missing_herdr_1_until_it_answers_again() {
    let mut state = LiveState::default();
    assert!(!state.herdr_1_unavailable());
    state.apply(ClientEvent::Disconnected {
        reason: "Herdr is not installed on the host".into(),
        ssh: Some(SshFailure::HerdrMissing),
    });
    assert!(state.herdr_1_unavailable());
    // Another failure says nothing about Herdr being there.
    state.apply(ClientEvent::Disconnected {
        reason: "unreachable".into(),
        ssh: Some(SshFailure::Unreachable),
    });
    assert!(!state.herdr_1_unavailable());
    state.apply(ClientEvent::Disconnected {
        reason: "Herdr is not installed on the host".into(),
        ssh: Some(SshFailure::HerdrMissing),
    });
    let welcome = serde_json::from_str(include_str!(
        "../../../../sheprd-protocol/tests/fixtures/endpoint-welcome-v1.json"
    ))
    .unwrap();
    state.apply(ClientEvent::Connected(welcome));
    assert!(
        !state.herdr_1_unavailable(),
        "installed and connected since"
    );
}

#[test]
fn a_herdr_1_too_old_to_connect_is_unavailable_until_one_connects() {
    let mut state = LiveState::default();
    state.apply(ClientEvent::VersionMismatch(
        sheprd_client::VersionMismatch::DaemonOutdated {
            server_version: Some("0.1.0".into()),
        },
    ));
    state.apply(ClientEvent::Disconnected {
        reason: "Herdr on the host is too old".into(),
        ssh: None,
    });
    assert!(state.herdr_1_unavailable());
    let welcome = serde_json::from_str(include_str!(
        "../../../../sheprd-protocol/tests/fixtures/endpoint-welcome-v1.json"
    ))
    .unwrap();
    state.apply(ClientEvent::Connected(welcome));
    assert!(!state.herdr_1_unavailable());
}
