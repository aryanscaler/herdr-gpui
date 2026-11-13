#![allow(clippy::unwrap_used)]
use super::*;

fn labels(names: &[&str]) -> HashSet<(TerminalId, String)> {
    names
        .iter()
        .map(|name| (TerminalId::new("t1").unwrap(), (*name).into()))
        .collect()
}

#[test]
fn reconnect_and_old_labels_are_never_replayed() {
    let mut seen = Seen::default();
    assert!(seen.take(labels(&["old"])).is_empty());
    assert_eq!(seen.take(labels(&["old", "new"])), labels(&["new"]));
    assert!(seen.take(labels(&["old", "new"])).is_empty());
    assert!(
        seen.take(labels(&[])).is_empty(),
        "cancelled request disappears"
    );
    seen = Seen::default();
    assert!(seen.take(labels(&["new", "during-disconnect"])).is_empty());
}

#[test]
fn exceeding_capacity_cannot_replay_when_labels_disappear() {
    let mut seen = Seen::default();
    seen.take(HashSet::new());
    let names: Vec<_> = (0..=MAX_SEEN).map(|index| index.to_string()).collect();
    let strings: Vec<_> = names.iter().map(String::as_str).collect();
    assert!(seen.take(labels(&strings)).is_empty());
    assert!(seen.take(labels(&["0"])).is_empty());
}

#[test]
fn reopening_a_host_waits_for_a_new_list_and_suppresses_queued_labels() {
    let now = Instant::now();
    let mut seen = Seen::default();
    seen.take(labels(&[]));
    seen.pause(now);
    assert!(!seen.fresh(None));
    assert!(!seen.fresh(Some(now)));
    assert!(seen.fresh(Some(now + std::time::Duration::from_millis(1))));
    assert!(seen.take(labels(&["queued-while-hidden"])).is_empty());
    assert_eq!(
        seen.take(labels(&["queued-while-hidden", "new"])),
        labels(&["new"])
    );
}
