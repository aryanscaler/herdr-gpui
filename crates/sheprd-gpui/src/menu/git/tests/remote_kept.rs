use super::*;
use crate::browser::{Location, Scope};
use sheprd_client::ConnectTarget;

#[gpui::test]
fn remote_kept_checkouts_never_reach_local_git_workers(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(crate::sidebar::layout_tests::fixture_window);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            let target = ConnectTarget::Ssh {
                target: "box".into(),
                session: "work".into(),
            };
            view.endpoints.push(crate::endpoint::Endpoint::new(
                "ssh:box".into(),
                "Box".into(),
                target.clone(),
                true,
            ));
            view.selected_endpoint = 1;
            view.live = Default::default();
            let scope = Scope::endpoint("ssh:box");
            view.core_terminals.set_remote_hub_for_test(
                scope.clone(),
                target,
                crate::core_terminals::Hub::with_join(|_| {
                    Err(crate::core_terminals::Error::NotBundled)
                }),
            );
            let folder = "/work/same-path";
            let identity = sheprd_vcs::git::Identity {
                branch: Some("feature".into()),
                key: "/work/same-path/.git".into(),
                label: "repo".into(),
                linked: true,
            };
            view.core_terminals
                .seed_checkout_for_test(folder, Some(identity.clone()));
            view.core_terminals
                .seed_remote_checkout_for_test(&scope, folder, Some(identity));
            let location = Location::core_terminal(
                sheprd_session::panes::Panes::single(
                    sheprd_backend::TerminalId::new("t1").unwrap(),
                ),
                false,
            );
            let id = view
                .open_kept_workspace(None, std::path::Path::new(folder), location, window, cx)
                .unwrap();
            assert_eq!(view.kept_in_front(), Some(id.as_str()));
            assert!(
                !view.kept_checkout_inputs().is_empty(),
                "remote PR metadata exists"
            );
            assert!(
                view.git_input().is_none(),
                "remote focused checkout cannot run local Git actions"
            );
            assert!(
                view.listed_git_inputs().is_empty(),
                "remote sidebar cannot run local Git probes"
            );
            view.update_git();
            assert!(view.git.tracked().is_none());
        });
    });
}
