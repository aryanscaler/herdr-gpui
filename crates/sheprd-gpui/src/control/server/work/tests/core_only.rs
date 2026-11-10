//! Work commands from a herdr-core terminal: one in a workspace the app
//! keeps for Herdr 2 finds it, and what needs Herdr 1's state says so; one
//! in a tab of no workspace the window keeps refuses as unsupported, whether
//! Herdr runs beside it or not.

use super::*;
use crate::{
    browser::{Location, Store},
    core_terminals::panes::Panes,
};
use sheprd_backend::TerminalId;

#[gpui::test]
fn a_core_only_workspace_beside_herdr_is_unsupported(cx: &mut gpui::TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut view = fixture_window(window, cx);
        connected(&mut view);
        view
    });
    view.update(cx, |view, cx| {
        let scope = crate::browser::scope(&view.endpoints[0]);
        let id = TerminalId::new("t1").unwrap();
        let location = Location::core_terminal(Panes::single(id.clone()), false);
        // A workspace the app keeps, which Herdr does not list.
        let tab = Store::update(cx, |store| {
            store.open(scope.clone(), "kept-1", Some(location), None)
        })
        .unwrap();
        view.core_terminals
            .show_for_test(tab, Panes::single(id), false, Vec::new());
        view.core_terminals.place_for_test(tab, scope, "kept-1");
    });
    view.read_with(cx, |view, _| {
        let from_core = Caller {
            terminal_id: Some("t1".into()),
            ..caller(None)
        };
        let Err(response) = find(view, &from_core) else {
            panic!("a core-only workspace has no Herdr state");
        };
        assert_eq!(error_code(&response), Some(ErrorCode::Unsupported));
        // A Herdr workspace that is gone is still not found.
        let Err(response) = find(view, &caller(Some("w99"))) else {
            panic!("w99 is not shown");
        };
        assert_eq!(error_code(&response), Some(ErrorCode::WorkspaceNotFound));
    });
}

#[gpui::test]
fn a_kept_workspace_is_found_from_its_terminal(cx: &mut gpui::TestAppContext) {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut view = fixture_window(window, cx);
        connected(&mut view);
        view
    });
    let id = cx
        .update(|window, cx| {
            view.update(cx, |view, cx| {
                let terminal = TerminalId::new("t1").unwrap();
                let location = Location::core_terminal(Panes::single(terminal.clone()), false);
                let id = view.open_kept_workspace(
                    Some("tools"),
                    std::path::Path::new("/src/tools"),
                    location,
                    window,
                    cx,
                )?;
                let tab = view.core_terminals.tab_of(&terminal)?;
                let scope = crate::browser::scope(&view.endpoints[0]);
                view.core_terminals.place_for_test(tab, scope, &id);
                Some(id)
            })
        })
        .unwrap();
    let from_core = Caller {
        terminal_id: Some("t1".into()),
        ..caller(None)
    };
    view.update(cx, |view, cx| {
        let Ok(found) = find(view, &from_core) else {
            panic!("the tab's workspace is found");
        };
        assert_eq!(found.workspace.id, id);
        assert_eq!(found.workspace.label, "tools");
        assert!(found.herdr.is_none(), "no Herdr 1 state");
        // A dialog opens on the selected host, Herdr 1 connected or not.
        assert_eq!(selected_workspace(view, &from_core).ok(), Some(id.clone()));
        // What needs Herdr 1's state says so rather than "not found".
        let response = mute::status(view, &from_core, cx);
        assert_eq!(error_code(&response), Some(ErrorCode::Unsupported));
    });
}

#[gpui::test]
fn restore_from_a_core_terminal_opens_its_kept_checkouts_checkpoints(
    cx: &mut gpui::TestAppContext,
) {
    // Checkpoints are kept on Linux and macOS checkouts only.
    if !cfg!(any(target_os = "linux", target_os = "macos")) {
        return;
    }
    let (view, cx) = cx.add_window_view(fixture_window);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            connected(view);
            view.core_terminals.seed_checkout_for_test(
                "/src/tools",
                Some(sheprd_vcs::git::Identity {
                    branch: Some("feature".into()),
                    key: "/src/tools/.git".into(),
                    label: "tools".into(),
                    linked: false,
                }),
            );
            let terminal = TerminalId::new("t1").unwrap();
            let location = Location::core_terminal(Panes::single(terminal.clone()), false);
            let id = view
                .open_kept_workspace(
                    None,
                    std::path::Path::new("/src/tools"),
                    location,
                    window,
                    cx,
                )
                .unwrap();
            let tab = view.core_terminals.tab_of(&terminal).unwrap();
            let scope = crate::browser::scope(&view.endpoints[0]);
            view.core_terminals.place_for_test(tab, scope, &id);
            let from_core = Caller {
                terminal_id: Some("t1".into()),
                ..caller(None)
            };
            let opened = flows::restore(view, &from_core, None, window, cx);
            assert!(matches!(opened, Response::Done { .. }), "{opened:?}");
            let dialog = view.checkpoints.view.as_ref().unwrap();
            assert_eq!(dialog.checkout.branch, "feature");
            assert_eq!(dialog.checkout.repo_key, "/src/tools/.git");
            assert_eq!(dialog.workspace_id, id);
        })
    });
}
