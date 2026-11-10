//! Workspaces kept behind the one in front, as fan-out opens its Herdr 2
//! lanes: always this computer's, and never by switching the shown host.

use super::*;

#[gpui::test]
fn a_lane_opens_and_closes_on_this_computer_while_another_host_is_shown(cx: &mut TestAppContext) {
    let (view, cx) = window(cx, true);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.config.herdr_2_terminals = true;
            let remote = crate::endpoint::Endpoint::new(
                "ssh:test".into(),
                "Remote".into(),
                sheprd_client::ConnectTarget::Ssh {
                    target: "unused".into(),
                    session: "default".into(),
                },
                true,
            );
            view.endpoints.push(remote);
            view.endpoints[0].live = std::mem::take(&mut view.live);
            view.selected_endpoint = 1;
            let location =
                Location::core_terminal(Panes::single(TerminalId::new("t5").unwrap()), false);
            let lane = Path::new("/src/app-lane");
            let id = view
                .open_kept_workspace_behind("lane", lane, location, cx)
                .expect("kept on this computer");
            assert_eq!(view.selected_endpoint, 1, "the shown host stays shown");
            let scope = view.local_kept_scope().unwrap();
            assert_eq!(view.kept_workspace_in(&scope, lane, cx), Some(id.clone()));
            let tabs = cx.global::<Store>().in_workspace(&scope, &id).count();
            assert_eq!(tabs, 1, "its tab shows the agent's terminal");

            assert!(view.close_local_kept_folder(lane, window, cx));
            assert_eq!(view.selected_endpoint, 1);
            assert_eq!(view.kept_workspace_in(&scope, lane, cx), None);
            assert!(!view.close_local_kept_folder(lane, window, cx));
        })
    });
}
