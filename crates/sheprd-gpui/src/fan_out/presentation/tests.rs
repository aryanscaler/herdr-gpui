use super::*;
use crate::{core_terminals::panes::Panes, sidebar::layout_tests::fixture_window};
use sheprd_backend::TerminalId;
use sheprd_client::ConnectTarget;

#[gpui::test]
fn a_remote_lane_is_kept_once_and_closed_on_its_host_while_local_stays_selected(
    cx: &mut gpui::TestAppContext,
) {
    let (view, cx) = cx.add_window_view(fixture_window);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.endpoints.push(crate::endpoint::Endpoint::new(
                "ssh:fanout".into(),
                "Remote".into(),
                ConnectTarget::Ssh {
                    target: "fanout-test.invalid".into(),
                    session: "work".into(),
                },
                true,
            ));
            let selected = view.selected_endpoint;
            let host = crate::teleport::host_for(&view.endpoints[1].connection.target).unwrap();
            let checkout = crate::fan_out::job::Checkout {
                endpoint_id: "ssh:fanout".into(),
                host: host.clone(),
                worktree: sheprd_remote::work::Worktree {
                    path: "/work/lane".into(),
                    opened: sheprd_remote::work::Opened::Herdr2,
                },
                terminal: Some("t-owned".into()),
                recoverable: true,
            };
            let scope = Scope::endpoint("ssh:fanout");
            let location =
                Location::core_terminal(Panes::single(TerminalId::new("t-owned").unwrap()), false);
            for _ in 0..2 {
                view.show_fan_out_terminal(
                    "ssh:fanout",
                    &host,
                    "lane",
                    "/work/lane",
                    location.clone(),
                    cx,
                );
            }
            assert_eq!(view.selected_endpoint, selected);
            let id = view
                .kept_workspace_in(&scope, Path::new("/work/lane"), cx)
                .unwrap();
            assert_eq!(cx.global::<Store>().in_workspace(&scope, &id).count(), 1);
            assert!(
                view.sessions(selected)
                    .core
                    .is_none_or(|session| session.workspace(&id).is_none())
            );
            let target = view.endpoints[1].connection.target.clone();
            view.endpoints[1].connection.target = ConnectTarget::Ssh {
                target: "replacement.invalid".into(),
                session: "work".into(),
            };
            view.close_fan_out_folder(&checkout, window, cx);
            assert!(
                view.kept_workspace_in(&scope, Path::new("/work/lane"), cx)
                    .is_some()
            );
            view.endpoints[1].connection.target = target;
            view.close_fan_out_folder(&checkout, window, cx);
            assert_eq!(view.selected_endpoint, selected);
            assert!(
                view.kept_workspace_in(&scope, Path::new("/work/lane"), cx)
                    .is_none()
            );
        })
    });
}
