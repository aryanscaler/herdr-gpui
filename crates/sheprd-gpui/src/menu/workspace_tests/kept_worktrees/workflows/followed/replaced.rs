use super::*;

#[gpui::test]
fn an_archive_is_cancelled_when_its_checkout_changes_branch(cx: &mut TestAppContext) {
    let (view, cx, id) = window(cx, LINKED, Some(true));
    view.update(cx, |view, cx| {
        start_archive(view, &id, LINKED, cx);
        assert!(view.follows_kept_archive(&id));
        view.core_terminals.seed_checkout_for_test(
            LINKED,
            Some(Identity {
                branch: Some("replacement".into()),
                key: Path::new(MAIN).join(".git").display().to_string(),
                label: "app".into(),
                linked: true,
            }),
        );
        view.refresh_core_session(cx);
        view.poll_kept_archives(cx);
        assert!(!view.follows_kept_archive(&id));
        assert!(view.is_kept_workspace(&id));
        assert!(view.kept_worktree_done.is_empty());
    });
}
