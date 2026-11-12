use crate::{
    browser::{Location, Scope},
    core_terminals::{Hub, panes::Panes},
    sidebar::layout_tests::fixture_window,
};
use sheprd_backend::TerminalId;
use sheprd_client::ConnectTarget;

#[gpui::test]
fn a_remote_branch_input_keeps_its_posix_checkout_on_every_client_platform(
    cx: &mut gpui::TestAppContext,
) {
    let (view, cx) = cx.add_window_view(fixture_window);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            let target = ConnectTarget::Wsl {
                distro: "Ubuntu".into(),
                session: "work".into(),
            };
            let scope = Scope::endpoint("wsl:Ubuntu");
            view.endpoints.push(crate::endpoint::Endpoint::new(
                "wsl:Ubuntu".into(),
                "Ubuntu".into(),
                target.clone(),
                true,
            ));
            view.selected_endpoint = 1;
            view.live.snapshot = None;
            view.core_terminals.set_remote_hub_for_test(
                scope.clone(),
                target,
                Hub::with_join(|_| Err(crate::core_terminals::Error::NotBundled)),
            );
            view.core_terminals.seed_remote_checkout_for_test(
                &scope,
                "/same/path",
                Some(sheprd_vcs::git::Identity {
                    branch: Some("feature".into()),
                    key: "/same/path/.git".into(),
                    label: "app".into(),
                    linked: true,
                }),
            );
            let location =
                Location::core_terminal(Panes::single(TerminalId::new("t1").unwrap()), false);
            let id = view
                .open_kept_workspace(
                    None,
                    std::path::Path::new("/same/path"),
                    location,
                    window,
                    cx,
                )
                .unwrap();
            view.open_workspace_menu(&id, Default::default(), window, cx);
            let input = view.branch_repository_input().unwrap();
            assert_eq!(input.checkout.as_deref(), Some("/same/path"));
            assert_eq!(input.repo_key.as_deref(), Some("/same/path/.git"));
            assert_eq!(input.branch, "feature");
        })
    });
}
