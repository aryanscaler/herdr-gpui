use super::*;

#[gpui::test]
fn remote_git_metadata_never_uses_the_same_local_path(cx: &mut TestAppContext) {
    let (view, cx) = window(cx, false);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            let folder = "/home/me/api";
            let target = view.endpoints[1].connection.target.clone();
            view.core_terminals.set_remote_hub_for_test(
                scope(),
                target,
                crate::core_terminals::Hub::with_join(|_| {
                    Err(crate::core_terminals::Error::NotBundled)
                }),
            );
            let local = sheprd_vcs::git::Identity {
                branch: Some("local".into()),
                key: "/local/.git".into(),
                label: "local".into(),
                linked: false,
            };
            view.core_terminals
                .seed_checkout_for_test(folder, Some(local));
            let remote = sheprd_vcs::git::Identity {
                branch: Some("remote".into()),
                key: "/remote/.git".into(),
                label: "remote".into(),
                linked: true,
            };
            view.core_terminals
                .remote
                .get_mut(&scope())
                .unwrap()
                .checkouts
                .seed(folder, Some(remote));
            let location =
                Location::core_terminal(Panes::single(TerminalId::new("t1").unwrap()), false);
            let id = view
                .open_kept_workspace(None, Path::new(folder), location, window, cx)
                .unwrap();
            let session = view.sessions(1).core.unwrap();
            let workspace = session.workspace(&id).unwrap();
            assert_eq!(workspace.branch.as_deref(), Some("remote"));
            let tree = workspace.worktree.as_ref().unwrap();
            assert_eq!(tree.key, "/remote/.git");
            assert!(tree.linked);
            let input = crate::pull_request::kept_input(workspace, true).unwrap();
            assert_eq!(input.checkout.as_deref(), Some(folder));
            assert_eq!(input.repo_key.as_deref(), Some("/remote/.git"));
        })
    });
}

#[gpui::test]
fn an_edited_destination_cannot_land_work_on_its_replacement(cx: &mut TestAppContext) {
    let (view, cx) = window(cx, false);
    view.update(cx, |view, cx| {
        let old = view.endpoints[1].connection.target.clone();
        view.endpoints[1].connection.target = ConnectTarget::Wsl {
            distro: "Ubuntu".into(),
            session: "work".into(),
        };
        assert!(!view.select_worktree_host(REMOTE, &old, cx));
        assert!(!view.select_worktree_host("removed", &old, cx));
        assert_eq!(view.selected_endpoint, 1);
        assert!(
            view.sessions(0)
                .core
                .is_none_or(|session| session.workspaces.is_empty())
        );
    });
}
