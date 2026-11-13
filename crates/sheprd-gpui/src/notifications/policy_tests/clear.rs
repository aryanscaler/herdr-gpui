//! Clear All Notifications on the notices and alerts of every host.

use super::*;

/// Two hosts, neither with the alert's tab in front, so each keeps its alert.
fn hosts() -> [Endpoint; 2] {
    let mut endpoints = endpoints();
    let snapshot = Arc::make_mut(endpoints[1].live.snapshot.as_mut().unwrap());
    snapshot.focused_tab_id = None;
    snapshot.focused_workspace_id = None;
    endpoints
}

fn alert_on_both(endpoints: &mut [Endpoint], now: Instant) {
    for endpoint in endpoints.iter_mut() {
        receive(endpoint, wire(Kind::NeedsAttention), now);
    }
    tick(endpoints, 1, config(0), false, now);
}

fn kept(endpoint: &Endpoint) -> Vec<u64> {
    endpoint
        .toasts
        .history
        .alerts()
        .map(|alert| alert.id)
        .collect()
}

#[test]
fn clearing_drops_shown_and_waiting_toasts_and_alerts_on_every_host() {
    let now = Instant::now();
    let mut endpoints = hosts();
    alert_on_both(&mut endpoints, now);
    // One shows; the other host's waits its turn. A third, just arrived, is
    // not ticked yet.
    assert_eq!(visible(&endpoints).len(), 1);
    let mut third = wire(Kind::Custom);
    third.pane_id = None;
    receive(&mut endpoints[1], third, now);
    assert_eq!(
        endpoints
            .iter()
            .map(|e| e.toasts.entries.len())
            .sum::<usize>(),
        3
    );
    assert!(endpoints.iter().all(|e| kept(e).len() == 1));
    let revisions: Vec<u64> = endpoints
        .iter()
        .map(|e| e.toasts.history.revision())
        .collect();
    for endpoint in &mut endpoints {
        endpoint.toasts.clear(now);
    }
    assert!(endpoints.iter().all(|e| e.toasts.entries.is_empty()));
    assert!(endpoints.iter().all(|e| kept(e).is_empty()));
    // The page sees a change, though nothing new was kept.
    for (endpoint, before) in endpoints.iter().zip(revisions) {
        assert!(endpoint.toasts.history.revision() > before);
    }
    // Nothing comes back on the next tick.
    tick(
        &mut endpoints,
        1,
        config(0),
        false,
        now + Duration::from_secs(1),
    );
    assert!(visible(&endpoints).is_empty());
}

#[test]
fn an_alert_after_a_clear_is_kept_under_a_new_id() {
    let now = Instant::now();
    let mut endpoints = hosts();
    alert_on_both(&mut endpoints, now);
    let old = kept(&endpoints[0])[0];
    let before = endpoints[0].toasts.history.revision();
    endpoints[0].toasts.clear(now);
    let cleared = endpoints[0].toasts.history.revision();
    let later = now + Duration::from_secs(10);
    receive(&mut endpoints[0], wire(Kind::NeedsAttention), later);
    tick(&mut endpoints, 1, config(0), false, later);
    let ids = kept(&endpoints[0]);
    assert_eq!(ids.len(), 1);
    assert_ne!(ids[0], old, "an old id must not open the new alert");
    assert!(endpoints[0].toasts.history.get(old).is_none());
    assert!(endpoints[0].toasts.history.get(ids[0]).is_some());
    let after = endpoints[0].toasts.history.revision();
    assert!(before < cleared && cleared < after);
}

#[test]
fn clearing_an_empty_history_is_no_news() {
    let mut endpoints = hosts();
    let before = endpoints[0].toasts.history.revision();
    endpoints[0].toasts.clear(Instant::now());
    assert_eq!(endpoints[0].toasts.history.revision(), before);
}

#[test]
fn only_notices_that_arrived_after_the_clear_are_received() {
    let now = Instant::now();
    let mut endpoints = hosts();
    endpoints[0].toasts.clear(now);
    // Still in the inbox at the clear, then drained.
    receive(&mut endpoints[0], wire(Kind::NeedsAttention), now);
    assert!(endpoints[0].toasts.entries.is_empty());
    let mut local = Notice::local_feedback(tests::notification("feedback"), now);
    local.promote(now);
    endpoints[0].toasts.receive([local]);
    assert_eq!(
        endpoints[0].toasts.entries.len(),
        1,
        "feedback is not a daemon notice"
    );
    receive(
        &mut endpoints[0],
        wire(Kind::NeedsAttention),
        now + Duration::from_nanos(1),
    );
    assert_eq!(endpoints[0].toasts.entries.len(), 2);
}

#[test]
fn a_clear_never_moves_an_enable_cutoff_back() {
    let now = Instant::now();
    let mut endpoints = hosts();
    let later = now + Duration::from_secs(5);
    endpoints[0].toasts.enabled_since = Some(later);
    endpoints[0].toasts.clear(now);
    assert_eq!(endpoints[0].toasts.enabled_since, Some(later));
}
