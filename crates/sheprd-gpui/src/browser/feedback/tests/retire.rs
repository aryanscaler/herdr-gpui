use super::*;

fn remote(scope: &str, pane: &str) -> FeedbackKey {
    FeedbackKey {
        scope: Scope::endpoint(scope),
        pane_id: pane.into(),
    }
}

#[test]
fn retiring_a_host_drops_only_its_notes_and_waiters() {
    let mut feedback = Feedback::default();
    let old = remote("ssh:box", "core:t1");
    let other = remote("ssh:other", "core:t1");
    for target in [&old, &other] {
        feedback.keep(
            Batch {
                target: target.clone(),
                text: "note".into(),
            },
            None,
        );
    }
    feedback.set_waiting(vec![old.clone(), other.clone()]);
    feedback.retire(&Scope::endpoint("ssh:box"));
    assert!(!feedback.has(&old));
    assert!(feedback.take(&old).is_none());
    assert!(feedback.has(&other));
    assert_eq!(feedback.waiting(), [other]);
}
