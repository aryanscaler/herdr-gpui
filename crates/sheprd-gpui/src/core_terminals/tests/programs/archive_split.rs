use super::*;

#[gpui::test]
fn a_split_archives_exit_is_consumed_before_its_pane_is_removed(cx: &mut TestAppContext) {
    let (view, cx, tab) = window_with_split(cx, &["archive", "shell"]);
    let terminal = id("archive");
    let backend = Scripted::default();
    let workspace = cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            let scope = view.local_kept_scope().unwrap();
            let workspace = Store::update(cx, |store| {
                store.keep_workspace(&scope, "archive", "/nonexistent/archive-checkout", false)
            })
            .unwrap();
            view.follow_archive_for_test(workspace.clone(), terminal.clone());
            let link = worker::spawn(
                backend.clone(),
                terminal.clone(),
                size(80, 24),
                Role::Active,
            )
            .unwrap();
            let painter = Rc::new(RefCell::new(TerminalPainter::default()));
            let pane = cx.new(|cx| {
                CoreTerminalView::new(terminal.clone(), link, look(), painter, false, cx)
            });
            view.core_terminals
                .views
                .insert((tab, terminal.clone()), pane);
            view.poll_kept_scripts(cx);
            assert!(view.follows_kept_archive(&workspace));
            workspace
        })
    });
    backend.push(vec![SurfaceUpdate::Ended(Exit { code: Some(1) })]);
    until(|| {
        cx.update(|window, cx| {
            view.update(cx, |view, cx| {
                // The real tick polls archives first, then polls and prunes panes.
                view.poll_kept_scripts(cx);
                view.poll_core_panes(window, cx);
                (!view
                    .core_terminals
                    .views
                    .contains_key(&(tab, terminal.clone())))
                .then_some(())
            })
        })
    });
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.poll_kept_scripts(cx);
            assert!(!view.follows_kept_archive(&workspace));
            assert_eq!(
                view.local_error.as_deref(),
                Some(crate::i18n::fl!("worktree-scripts-archive-failed").as_str()),
                "the exit must be handled as failure, not mistaken for cancellation"
            );
            assert!(view.kept_worktree_done.is_empty());
        })
    });
}
