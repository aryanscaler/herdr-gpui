//! A host with no Herdr 1 at all, and whether the connection bar speaks of
//! one: not while its herdr-core answers and runs the window's terminals.

use super::*;
use sheprd_client::{SshFailure, VersionMismatch};

/// A host whose `herdr` is not installed, as the connection bridge reports it.
fn missing_installation() -> LiveState {
    let mut state = LiveState {
        missing_installation: true,
        ..Default::default()
    };
    state.apply(ClientEvent::Disconnected {
        reason: "Could not start herdr server".into(),
        ssh: None,
    });
    state
}

#[test]
fn a_host_with_herdr_1_is_reported_whether_or_not_a_core_answers() {
    let state = LiveState::default();
    assert!(!state.herdr_1_absent());
    assert!(state.reports_herdr_1(true));
    assert!(state.reports_herdr_1(false));
}

#[test]
fn a_host_without_herdr_1_is_silent_while_its_core_answers() {
    let state = missing_installation();
    assert!(state.herdr_1_absent());
    assert!(
        state.herdr_1_unavailable(),
        "absent is one way to be unavailable"
    );
    // The window runs its terminals in herdr-core: no outage to report.
    assert!(!state.reports_herdr_1(true));
    // With the core gone too, nothing is left to run them in.
    assert!(state.reports_herdr_1(false));
}

#[test]
fn a_saved_host_whose_bridge_found_no_herdr_is_absent() {
    let mut state = LiveState::default();
    state.apply(ClientEvent::Disconnected {
        reason: "Herdr is not installed on the host".into(),
        ssh: Some(SshFailure::HerdrMissing),
    });
    assert!(state.herdr_1_absent());
    assert!(!state.reports_herdr_1(true));
    // Another failure says nothing about Herdr being there.
    state.apply(ClientEvent::Disconnected {
        reason: "unreachable".into(),
        ssh: Some(SshFailure::Unreachable),
    });
    assert!(!state.herdr_1_absent());
    assert!(state.reports_herdr_1(true));
}

#[test]
fn a_herdr_1_too_old_stays_reported_while_a_core_answers() {
    let mut state = LiveState::default();
    state.apply(ClientEvent::VersionMismatch(
        VersionMismatch::DaemonOutdated {
            server_version: Some("0.1.0".into()),
        },
    ));
    state.apply(ClientEvent::Disconnected {
        reason: "Herdr on the host is too old".into(),
        ssh: None,
    });
    // Upgrading is the user's to do, so an answer from herdr-core does not
    // stand in for it the way an absent Herdr 1 does.
    assert!(state.herdr_1_unavailable());
    assert!(!state.herdr_1_absent());
    assert!(state.reports_herdr_1(true));
    assert!(state.reports_herdr_1(false));
}

#[test]
fn an_installed_herdr_1_that_answers_is_reported_again() {
    let mut state = missing_installation();
    assert!(!state.reports_herdr_1(true));
    let welcome = serde_json::from_str(include_str!(
        "../../../../sheprd-protocol/tests/fixtures/endpoint-welcome-v1.json"
    ))
    .unwrap();
    state.apply(ClientEvent::Connected(welcome));
    assert!(!state.herdr_1_absent());
    assert!(state.reports_herdr_1(true));
}
