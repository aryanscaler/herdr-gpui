use super::*;
use crate::control::protocol::work::{Reply, remote::Request};
use crate::{
    browser::{Location, Store},
    core_terminals::{Host, panes::Panes},
    remote_work::Target,
};
use sheprd_backend::TerminalId;

#[gpui::test]
fn remote_work_uses_its_driven_tab_and_refuses_foreign_window_scope_or_terminal(
    cx: &mut gpui::TestAppContext,
) {
    let (view, cx) = cx.add_window_view(fixture_window);
    cx.update(|window, cx| view.update(cx, |view, cx| {
        let target = sheprd_client::ConnectTarget::Ssh { target: "nobody@invalid.invalid".into(), session: "work".into() };
        view.endpoints.push(crate::endpoint::Endpoint::new("ssh:work".into(), "Work host".into(), target.clone(), true));
        view.selected_endpoint = 1;
        view.live.snapshot = None;
        view.config.herdr_2_terminals = true;
        let scope = crate::browser::scope(&view.endpoints[1]);
        view.core_terminals.set_remote_hub_for_test(scope.clone(), target, crate::core_terminals::Hub::with_join(|_| Err(crate::core_terminals::Error::NotBundled)));
        let terminal = TerminalId::new("t1").unwrap();
        let id = view.open_kept_workspace(Some("tools"), std::path::Path::new("/home/me/tools"), Location::core_terminal(Panes::single(terminal.clone()), false), window, cx).unwrap();
        let tab = cx.global::<Store>().in_workspace(&scope, &id).next().unwrap().id;
        view.core_terminals.show_on_host_for_test(tab, Host::Remote(scope.clone()), terminal.clone());
        view.core_terminals.place_for_test(tab, scope.clone(), &id);
        let request = |window| Request { version: 1, window, operation: WorkOp::Mute { muted: true } };
        let caller = Target { scope: &scope, tab, terminal: &terminal, workspace: &id };
        let wrong_window = window.window_handle().window_id().as_u64().saturating_add(1);
        let response = view.serve_remote_work(caller, request(Some(wrong_window)), window, cx);
        assert_eq!(error_code(&response), Some(ErrorCode::WorkspaceNotFound));
        let local = crate::browser::scope(&view.endpoints[0]);
        let response = view.serve_remote_work(Target { scope: &local, ..caller }, request(None), window, cx);
        assert_eq!(error_code(&response), Some(ErrorCode::WorkspaceNotFound));
        let response = view.serve_remote_work(Target { terminal: &TerminalId::new("t2").unwrap(), ..caller }, request(None), window, cx);
        assert_eq!(error_code(&response), Some(ErrorCode::WorkspaceNotFound));
        let response = view.serve_remote_work(caller, request(None), window, cx);
        assert!(matches!(response, Response::Work { reply: Reply::Mute { mute } } if mute.muted && mute.host == "Work host"));
        assert!(!crate::notification_mutes::Mutes::of(cx).unwrap().kept(local.as_str(), crate::notification_mutes::Scope::Workspace, &id));
        // The request was discovered in A; moving its tab while the claim
        // waits must not turn it into a request against B.
        let moved = Store::update(cx, |store| {
            let moved = store.keep_workspace(&scope, "other", "/home/me/other", true).unwrap();
            assert!(store.move_to_workspace(tab, &moved));
            moved
        });
        view.core_terminals.place_for_test(tab, scope.clone(), &moved);
        let response = view.serve_remote_work(caller, request(None), window, cx);
        assert_eq!(error_code(&response), Some(ErrorCode::WorkspaceNotFound));
        assert!(!crate::notification_mutes::Mutes::of(cx).unwrap().kept(scope.as_str(), crate::notification_mutes::Scope::Workspace, &moved));
    }));
}

#[gpui::test]
fn remote_note_listing_does_not_return_other_hosts_notes(cx: &mut gpui::TestAppContext) {
    cx.update(|cx| {
        for endpoint in ["local", "ssh:work", "ssh:other"] {
            crate::worktree_notes::Notes::update(cx, |notes| {
                notes.set(
                    crate::worktree_notes::Checkout {
                        endpoint: endpoint.into(),
                        repo_key: "/repo/.git".into(),
                        branch: "feature".into(),
                    },
                    endpoint,
                )
            });
        }
        let Response::Work {
            reply: Reply::Notes { notes, .. },
        } = notes::list_on("ssh:work", cx)
        else {
            panic!("note reply");
        };
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].host, "ssh:work");
    });
}

#[gpui::test]
fn a_long_remote_note_listing_fits_one_reply_and_counts_the_rest(cx: &mut gpui::TestAppContext) {
    cx.update(|cx| {
        for branch in 0..100 {
            crate::worktree_notes::Notes::update(cx, |notes| {
                notes.set(
                    crate::worktree_notes::Checkout {
                        endpoint: "ssh:work".into(),
                        repo_key: "/repo/.git".into(),
                        branch: format!("feature-{branch}"),
                    },
                    &"n".repeat(400),
                )
            });
        }
        let response = notes::list_on("ssh:work", cx);
        assert!(crate::control::protocol::work::remote::encode_reply(&response).is_ok());
        let Response::Work {
            reply: Reply::Notes { notes, omitted },
        } = response
        else {
            panic!("note reply");
        };
        assert!(!notes.is_empty());
        assert!(omitted > 0);
        assert_eq!(notes.len() + omitted, 100);
    });
}
