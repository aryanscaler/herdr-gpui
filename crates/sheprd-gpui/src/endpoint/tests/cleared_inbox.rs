//! Notices still in a connection's inbox when Clear All Notifications runs.

use super::*;
use crate::notifications::tests::notification;
use sheprd_client::protocol::ServerMessage;

/// A connected endpoint with `titles` waiting in its inbox, not yet polled.
fn endpoint_with(titles: &[&str]) -> Endpoint {
    let endpoint = Endpoint::new(LOCAL.into(), "Local".into(), ConnectTarget::Local, true);
    deliver(&endpoint, titles);
    endpoint
}

fn deliver(endpoint: &Endpoint, titles: &[&str]) {
    let mut state = endpoint.connection.inbox.lock().unwrap();
    state.status = ConnectionStatus::Connected;
    for title in titles {
        state.apply(ClientEvent::Message(ServerMessage::SemanticNotification(
            notification(title),
        )));
    }
}

#[test]
fn a_notice_received_before_a_clear_never_reaches_the_toasts() {
    let mut endpoint = endpoint_with(&["before"]);
    endpoint.toasts.clear(Instant::now());
    endpoint.poll(Instant::now());
    assert!(endpoint.toasts.entries.is_empty());
    assert_eq!(endpoint.toasts.history.alerts().count(), 0);
}

/// Two windows hear the same event; one shows it before the clear, the
/// other has not polled yet. One cutoff drops it from both.
#[test]
fn a_duplicate_not_yet_polled_by_another_window_stays_cleared() {
    let mut first = endpoint_with(&["shared"]);
    let mut second = endpoint_with(&["shared"]);
    first.poll(Instant::now());
    assert_eq!(first.toasts.entries.len(), 1);
    let cutoff = Instant::now();
    for endpoint in [&mut first, &mut second] {
        endpoint.toasts.clear(cutoff);
    }
    for endpoint in [&mut first, &mut second] {
        endpoint.poll(Instant::now());
        assert!(endpoint.toasts.entries.is_empty());
    }
}
