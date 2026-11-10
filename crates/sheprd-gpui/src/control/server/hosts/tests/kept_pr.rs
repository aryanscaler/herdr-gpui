//! `host pr` from a Herdr 2 pane: a workspace the app keeps is looked up by
//! the checkout Git names for its folder, as the window prefetches it.

use super::super::pr::status;
use super::*;
use crate::{browser::Location, core_terminals::panes::Panes, pull_request::fixture};
use sheprd_backend::TerminalId;
use sheprd_vcs::git::Identity;
use std::path::Path;

// Pull requests are only looked up for an absolute checkout, which a path
// without a drive is not on Windows.
#[cfg(not(windows))]
const FOLDER: &str = "/src/app-feature";
#[cfg(not(windows))]
const KEY: &str = "/src/app/.git";
#[cfg(windows)]
const FOLDER: &str = r"C:\src\app-feature";
#[cfg(windows)]
const KEY: &str = r"C:\src\app\.git";

#[gpui::test]
fn a_kept_workspaces_pull_request_is_found_from_its_terminal(cx: &mut TestAppContext) {
    let (view, cx) = window(cx);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.menu.github = crate::github::Auth::connected_fixture();
            view.live.local_daemon_peer = true;
            view.core_terminals.seed_checkout_for_test(
                FOLDER,
                Some(Identity {
                    branch: Some("feature".into()),
                    key: KEY.into(),
                    label: "app".into(),
                    linked: true,
                }),
            );
            let terminal = TerminalId::new("t1").unwrap();
            let location = Location::core_terminal(Panes::single(terminal.clone()), false);
            let id = view
                .open_kept_workspace(None, Path::new(FOLDER), location, window, cx)
                .unwrap();
            let tab = view.core_terminals.tab_of(&terminal).unwrap();
            let scope = crate::browser::scope(&view.endpoints[0]);
            view.core_terminals.place_for_test(tab, scope, &id);
            let from_core = Caller {
                terminal_id: Some("t1".into()),
                ..caller(None)
            };
            let input = crate::pull_request::key_input(KEY, Some("feature")).unwrap();
            view.menu
                .pr_cache
                .seed(input, fixture().unwrap(), std::time::Instant::now());
            let Response::Host {
                reply: Reply::Pr { pr: Some(pr), .. },
            } = status(view, &from_core)
            else {
                panic!("expected the kept checkout's pull request");
            };
            assert_eq!(pr.number, 8);
        })
    });
}
