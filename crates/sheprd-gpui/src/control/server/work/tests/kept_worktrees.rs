use super::*;
use crate::{browser::Location, core_terminals::panes::Panes};
use sheprd_backend::TerminalId;

#[gpui::test]
fn scripts_and_listing_accept_an_app_kept_checkout_without_herdr(cx: &mut gpui::TestAppContext) {
    let (view, cx) = cx.add_window_view(fixture_window);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.live.snapshot = None;
            view.live.missing_installation = true;
            let folder = if cfg!(windows) {
                "C:/src/tools"
            } else {
                "/src/tools"
            };
            view.core_terminals.seed_checkout_for_test(
                folder,
                Some(sheprd_vcs::git::Identity {
                    branch: Some("feature".into()),
                    key: format!("{folder}/.git"),
                    label: "tools".into(),
                    linked: true,
                }),
            );
            let terminal = TerminalId::new("t1").unwrap();
            let location = Location::core_terminal(Panes::single(terminal.clone()), false);
            let id = view
                .open_kept_workspace(None, std::path::Path::new(folder), location, window, cx)
                .unwrap();
            let tab = view.core_terminals.tab_of(&terminal).unwrap();
            let scope = crate::browser::scope(&view.endpoints[0]);
            view.core_terminals.place_for_test(tab, scope, &id);
            let caller = Caller {
                terminal_id: Some("t1".into()),
                ..caller(None)
            };
            let response = worktrees::list(view, &caller);
            let Response::Work {
                reply: crate::control::protocol::work::Reply::Worktrees { worktrees },
            } = response
            else {
                panic!("kept listing")
            };
            assert!(
                worktrees
                    .iter()
                    .any(|tree| tree.workspace == id && tree.path == folder && tree.linked)
            );
            let response = flows::script(
                view,
                &caller,
                crate::control::protocol::work::Script::Run,
                cx,
            );
            assert!(matches!(response, Response::Done { .. }), "{response:?}");
            assert!(view.worktree_script.is_some());
        })
    });
}

#[gpui::test]
fn a_worktree_dialog_refuses_a_herdr_workspace_instead_of_routing_elsewhere(
    cx: &mut gpui::TestAppContext,
) {
    let (view, cx) = cx.add_window_view(fixture_window);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            connected(view);
            let response = worktrees::dialog(
                view,
                &caller(Some("w3")),
                worktrees::Dialog::Delete,
                window,
                cx,
            );
            assert_eq!(error_code(&response), Some(ErrorCode::Unsupported));
            assert!(view.menu.page.is_none());
        })
    });
}
