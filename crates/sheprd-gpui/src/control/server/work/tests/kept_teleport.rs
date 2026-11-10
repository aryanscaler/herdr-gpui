//! `sheprd work teleport` from a workspace the app keeps for Herdr 2: from
//! one of its herdr-core terminals, or from outside any pane while it is in
//! front, it opens the same dialog its menu does.

use super::*;
use crate::{
    browser::{Location, Store},
    core_terminals::panes::Panes,
    menu::Page,
};
use sheprd_backend::TerminalId;
use sheprd_vcs::git::Identity;
use std::path::Path;

const FOLDER: &str = "/src/app-feature";

#[gpui::test]
fn teleport_opens_from_a_kept_workspace(cx: &mut gpui::TestAppContext) {
    let (view, cx) = cx.add_window_view(fixture_window);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.endpoints[0].connection.target = sheprd_client::ConnectTarget::Local;
            view.live.snapshot = None;
            view.live.missing_installation = true;
            view.core_terminals.seed_checkout(
                FOLDER,
                Some(Identity {
                    branch: Some("feature".into()),
                    key: "/src/app/.git".into(),
                    label: "app".into(),
                    linked: true,
                }),
            );
            view.endpoints.push(crate::endpoint::Endpoint::new(
                "ssh:box".into(),
                "Box".into(),
                sheprd_client::ConnectTarget::Ssh {
                    target: "nobody@invalid.invalid".into(),
                    session: "default".into(),
                },
                true,
            ));
            let terminal = TerminalId::new("t1").unwrap();
            let location = Location::core_terminal(Panes::single(terminal.clone()), false);
            let id = view
                .open_kept_workspace(None, Path::new(FOLDER), location, window, cx)
                .unwrap();
            let scope = crate::browser::scope(&view.endpoints[0]);
            let tab = cx
                .global::<Store>()
                .in_workspace(&scope, &id)
                .next()
                .unwrap()
                .id;
            view.core_terminals
                .show_for_test(tab, Panes::single(terminal), false, Vec::new());
            view.core_terminals.place_for_test(tab, scope, &id);

            let from_terminal = Caller {
                terminal_id: Some("t1".into()),
                ..caller(None)
            };
            let response = flows::teleport(view, &from_terminal, "Box", window, cx);
            assert_eq!(error_code(&response), None, "{response:?}");
            assert_eq!(view.menu.page, Some(Page::Teleport));
            assert!(view.teleport.as_ref().unwrap().offers("ssh:box"));
            view.dismiss_menu(window, cx);
            view.teleport = None;

            // Outside any pane, the workspace in front is the caller's.
            let response = flows::teleport(view, &caller(None), "Box", window, cx);
            assert_eq!(error_code(&response), None, "{response:?}");
            assert_eq!(view.menu.page, Some(Page::Teleport));
            view.dismiss_menu(window, cx);
        })
    });
}
