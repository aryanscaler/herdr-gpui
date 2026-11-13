use super::*;
use crate::pull_request::{Origin, View};

#[gpui::test]
fn shown_remote_checkout_is_host_scoped_and_never_a_local_git_input(cx: &mut TestAppContext) {
    let (view, cx) = window(cx, false);
    view.update(cx, |view, _| {
        let folder = "/work/same-path";
        let tab = crate::browser::TabId::test(99);
        let terminal = TerminalId::new("t1").unwrap();
        view.core_terminals.set_remote_hub_for_test(
            scope(),
            view.endpoints[1].connection.target.clone(),
            crate::core_terminals::Hub::with_join(|_| {
                Err(crate::core_terminals::Error::NotBundled)
            }),
        );
        view.core_terminals.tabs.insert(
            tab,
            crate::core_terminals::TabPanes::new(
                Host::Remote(scope()),
                Panes::single(terminal.clone()),
                false,
            ),
        );
        view.core_terminals.shown = Some((tab, terminal.clone()));
        let identity = sheprd_vcs::git::Identity {
            branch: Some("remote".into()),
            key: "/remote/.git".into(),
            label: "repo".into(),
            linked: true,
        };
        view.core_terminals.seed_checkout_for_test(
            folder,
            Some(sheprd_vcs::git::Identity {
                branch: Some("local".into()),
                ..identity.clone()
            }),
        );
        let remote = view.core_terminals.remote.get_mut(&scope()).unwrap();
        remote.checkouts.seed(folder, Some(identity));
        remote.infos.insert(sheprd_backend::TerminalInfo {
            agent: sheprd_client::protocol::AgentStatus::Unknown,
            cwd: Some(folder.into()),
            foreground: None,
            id: terminal.clone(),
            labels: Default::default(),
            name: None,
            reporter: None,
            state: sheprd_backend::RunState::Running,
            title: None,
        });
        let input = view.core_git_input().unwrap();
        assert_eq!(input.branch, "remote");
        assert_eq!(input.repo_key.as_deref(), Some("/remote/.git"));
        assert!(view.core_folder(tab, &terminal).is_none());
        assert!(view.git_input().is_none());
        assert!(
            view.sessions(1).core.is_none(),
            "shown pane needs no kept workspace"
        );
        view.menu
            .github_hosts
            .insert(REMOTE.into(), crate::github::Auth::connected_fixture());
        let tokens = view.pr_tokens().unwrap();
        let origin = view.pr_origin().unwrap();
        let now = std::time::Instant::now();
        view.menu
            .pr_cache
            .scope((0, 0, String::new()), tokens.clone(), &tokens.key(), origin);
        view.menu
            .pr_cache
            .seed(input.clone(), crate::pull_request::fixture().unwrap(), now);
        view.menu.pr_cache.scope(
            (0, 0, String::new()),
            tokens.clone(),
            &tokens.key(),
            Origin::Local,
        );
        view.update_workspace_pr();
        assert!(
            view.menu.pr_cache.get(&input).is_some(),
            "shown-only SSH input is prefetched in its own scope"
        );
        view.selected_endpoint = 0;
        assert!(
            view.core_git_input().is_none(),
            "shown input cannot leak across selection"
        );
        view.selected_endpoint = 1;
        view.endpoints[1].connection.target = ConnectTarget::Ssh {
            target: "replacement".into(),
            session: "work".into(),
        };
        assert!(
            view.core_git_input().is_none(),
            "edited host cannot reuse old checkout metadata"
        );
        view.endpoints[1].connection.target = ConnectTarget::Wsl {
            distro: "Ubuntu".into(),
            session: "work".into(),
        };
        assert!(
            view.pr_origin().is_none(),
            "WSL must not fall back to a local PR origin"
        );
        assert!(view.endpoint_pr_view(1).is_none());
    });
}

#[gpui::test]
fn ssh_only_prefetch_and_menu_use_the_hosts_account_without_local_git(cx: &mut TestAppContext) {
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
            view.core_terminals
                .remote
                .get_mut(&scope())
                .unwrap()
                .checkouts
                .seed(
                    folder,
                    Some(sheprd_vcs::git::Identity {
                        branch: Some("feature".into()),
                        key: "/remote/.git".into(),
                        label: "api".into(),
                        linked: true,
                    }),
                );
            let location =
                Location::core_terminal(Panes::single(TerminalId::new("t1").unwrap()), false);
            let id = view
                .open_kept_workspace(None, Path::new(folder), location, window, cx)
                .unwrap();
            let mut auth = crate::github::Auth::connected_fixture();
            auth.profile.as_mut().unwrap().login = "remote-user".into();
            view.menu.github_hosts.insert(REMOTE.into(), auth);
            view.menu.github = Default::default();
            assert!(view.live.snapshot.is_none());
            assert!(!view.live.status.is_connected());
            assert!(view.git_input().is_none());
            let input = view
                .kept_checkout_inputs()
                .into_iter()
                .find(|(known, _)| known == &id)
                .unwrap()
                .1;
            let tokens = view.pr_tokens().unwrap();
            let expected = View {
                origin: Origin::Ssh("box".into()),
                login: tokens.key(),
            };
            assert_eq!(view.endpoint_pr_view(1), Some(expected.clone()));
            let now = std::time::Instant::now();
            view.menu.pr_cache.scope(
                (0, 0, String::new()),
                tokens.clone(),
                &tokens.key(),
                expected.origin.clone(),
            );
            view.menu
                .pr_cache
                .seed(input.clone(), crate::pull_request::fixture().unwrap(), now);
            // Start from a different scope: the tick must select SSH even though
            // there is no H1 snapshot and the main account is signed out.
            view.menu.pr_cache.scope(
                (0, 0, String::new()),
                tokens.clone(),
                &tokens.key(),
                Origin::Local,
            );
            view.update_workspace_pr();
            assert!(view.menu.pr_cache.get(&input).is_some());
            for wrong in [
                View {
                    origin: Origin::Local,
                    ..expected.clone()
                },
                View {
                    origin: Origin::Ssh("other".into()),
                    ..expected.clone()
                },
                View {
                    login: "another-account".into(),
                    ..expected.clone()
                },
            ] {
                assert!(view.menu.pr_cache.peek_input(&wrong, &input).is_none());
            }
            view.open_kept_workspace_menu(&id, Default::default(), window, cx);
            assert!(view.menu.pr.value.is_some());
            view.menu
                .github_hosts
                .get_mut(REMOTE)
                .unwrap()
                .profile
                .as_mut()
                .unwrap()
                .login = "replacement-account".into();
            view.menu.pr_cache.seed_rate_limit(
                "replacement-account",
                now + std::time::Duration::from_secs(60),
            );
            view.update_workspace_pr();
            assert!(
                view.menu.pr.value.is_none(),
                "an open menu cannot keep another account's PR"
            );
            view.endpoints[1].generation += 1;
            view.update_workspace_pr();
            assert!(
                view.menu.pr.value.is_none(),
                "a replaced endpoint invalidates the open menu"
            );
            view.menu.github_hosts.clear();
            view.update_workspace_pr();
            assert!(
                view.menu.pr_cache.get(&input).is_none(),
                "sign-out drops the old account's answer"
            );
        });
    });
}
