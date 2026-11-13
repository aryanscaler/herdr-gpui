use super::*;

/// Notes a waiter already holds for the old machine are dropped with it,
/// never put back where a waiter on the new machine could take them.
#[gpui::test]
fn a_replaced_hosts_held_notes_are_never_restored(cx: &mut gpui::TestAppContext) {
    cx.update(|cx| {
        let now = Instant::now();
        let mut held = pending("ssh:box", now);
        let mut feedback = Feedback::default();
        keep(&mut feedback, &held.target, "for the old machine");
        held.notes = feedback.take_lease(&held.target);
        assert!(held.notes.is_some());
        let target = held.target.clone();
        let other = pending("ssh:other", now);
        keep(&mut feedback, &other.target, "elsewhere");
        cx.set_global(feedback);
        cx.set_global(PendingReplies(vec![held, other]));

        retire(&Scope::endpoint("ssh:box"), cx);

        let replies = &cx.global::<PendingReplies>().0;
        assert_eq!(replies.len(), 1);
        assert_eq!(replies[0].target.scope, Scope::endpoint("ssh:other"));
        let feedback = cx.global::<Feedback>();
        assert!(!feedback.has(&target));
        assert!(feedback.has(&replies[0].target));
    });
}
