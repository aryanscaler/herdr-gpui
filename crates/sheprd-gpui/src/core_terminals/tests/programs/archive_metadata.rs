use super::*;

#[gpui::test]
fn archive_success_survives_missing_metadata_and_a_pruned_terminal_view(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().to_str().unwrap();
    let identity = sheprd_vcs::git::Identity {
        branch: Some("feature".into()),
        key: "/fixture/repo/.git".into(),
        label: "app".into(),
        linked: true,
    };
    let (view, cx, tab) = window_with_split(cx, &["archive", "shell"]);
    let terminal = id("archive");
    let backend = Scripted::default();
    backend.push(vec![SurfaceUpdate::Ended(Exit { code: Some(0) })]);
    let (workspace, pane) = cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            let scope = view.local_kept_scope().unwrap();
            let workspace = Store::update(cx, |store| {
                store.keep_workspace(&scope, "archive", folder, false)
            })
            .unwrap();
            view.core_terminals
                .seed_checkout_for_test(folder, Some(identity.clone()));
            view.refresh_core_session(cx);
            view.follow_archive_for_test(workspace.clone(), terminal.clone());
            let link =
                worker::spawn(backend, terminal.clone(), size(80, 24), Role::Active).unwrap();
            let painter = Rc::new(RefCell::new(TerminalPainter::default()));
            let pane = cx.new(|cx| {
                CoreTerminalView::new(terminal.clone(), link, look(), painter, false, cx)
            });
            view.core_terminals
                .views
                .insert((tab, terminal.clone()), pane.clone());
            (workspace, pane)
        })
    });
    until(|| {
        cx.update(|window, cx| {
            pane.update(cx, |pane, cx| {
                pane.poll(window, cx);
                pane.ended().then_some(())
            })
        })
    });
    view.update(cx, |view, cx| {
        // A transient host read loses metadata, not the kept workspace.
        view.core_terminals.seed_checkout_for_test(folder, None);
        view.refresh_core_session(cx);
        view.poll_kept_archives(cx);
        assert!(view.follows_kept_archive(&workspace));
        assert!(view.kept_worktree_done.is_empty());
        // The normal pane lifecycle can prune this view after its exit.
        view.core_terminals.views.remove(&(tab, terminal.clone()));
        view.poll_kept_archives(cx);
        assert!(view.follows_kept_archive(&workspace));
        view.core_terminals
            .seed_checkout_for_test(folder, Some(identity));
        view.refresh_core_session(cx);
        view.poll_kept_archives(cx);
        assert!(
            !view.follows_kept_archive(&workspace),
            "the saved success resumes deletion after recovery"
        );
    });
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert_eq!(view.kept_worktree_done.len(), 1, "removal is attempted exactly once; its fresh Git verification refuses this non-repository fixture");
    });
    assert!(dir.path().exists());
}
