//! Teleport from and to the workspaces the app keeps for Herdr 2 on this
//! computer, which no Herdr 1 snapshot lists.

use super::*;
use crate::core_terminals::panes::Panes;
use gpui::{Entity, TestAppContext, VisualTestContext};
use sheprd_backend::TerminalId;
use sheprd_vcs::git::Identity;
use std::path::Path;

const FOLDER: &str = "/src/app-feature";
const KEY: &str = "/src/app/.git";

/// This computer with no Herdr 1, keeping a workspace in a linked worktree
/// of `app` on `feature`, and an SSH host beside it. Answers the workspace.
fn window(cx: &mut TestAppContext) -> (Entity<SheprdWindow>, &mut VisualTestContext, String) {
    let (view, cx) = cx.add_window_view(crate::sidebar::layout_tests::fixture_window);
    let id = cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.endpoints[0].connection.target = sheprd_client::ConnectTarget::Local;
            view.live.snapshot = None;
            view.live.missing_installation = true;
            view.core_terminals.seed_checkout(
                FOLDER,
                Some(Identity {
                    branch: Some("feature".into()),
                    key: KEY.into(),
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
            let location = Location::core_terminal(Panes::single(terminal), false);
            view.open_kept_workspace(None, Path::new(FOLDER), location, window, cx)
        })
    });
    (view, cx, id.unwrap())
}

#[gpui::test]
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn a_kept_worktree_teleports_from_its_folder(cx: &mut TestAppContext) {
    let (view, cx, id) = window(cx);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.open_kept_workspace_menu(&id, Default::default(), window, cx);
            assert_eq!(teleport_items(view), 1);
            view.activate_workspace_menu(WorkspaceMenuAction::Teleport, window, cx);
            assert_eq!(view.menu.page, Some(Page::Teleport));
            let teleport = view.teleport.as_ref().unwrap();
            let source = teleport.source_for_test();
            assert_eq!(
                source.keeping,
                Keeping::App {
                    checkout: FOLDER.into()
                }
            );
            assert_eq!(
                (source.repo_key.as_str(), source.branch.as_deref()),
                (KEY, Some("feature"))
            );
            assert_eq!(source.core_tabs.len(), 1, "its Herdr 2 tab moves");
            assert!(teleport.offers("ssh:box"));
        })
    });
}

/// This computer, seen from elsewhere, takes the work into a workspace the
/// app keeps, finds its kept repositories, and the checkout work left.
#[gpui::test]
fn this_computer_lands_work_in_a_kept_workspace(cx: &mut TestAppContext) {
    let (view, cx, id) = window(cx);
    view.update(cx, |view, _| {
        let local = view.endpoints[0].id.clone();
        let host = Host::new(&sheprd_client::ConnectTarget::Local).unwrap();
        let fresh = view.teleport_host(0, host.clone());
        assert_eq!(fresh.lands_in, Keeper::App);
        assert_eq!(
            fresh.repositories.as_deref(),
            Some(
                &[Repository {
                    key: KEY.into(),
                    label: "app".into(),
                    workspace_id: id.clone(),
                }][..]
            )
        );
        assert!(fresh.retired.is_empty());

        view.teleport_marks.add(Mark {
            endpoint: local,
            repo_key: KEY.into(),
            branch: "feature".into(),
            destination: crate::teleport::MarkDestination {
                endpoint: "ssh:box".into(),
                label: "Box".into(),
                repo_key: "/home/me/app/.git".into(),
                workspace_id: "w2".into(),
            },
        });
        let back = view.teleport_host(0, host.clone());
        assert_eq!(
            back.retired,
            [Retired {
                repo_key: KEY.into(),
                branch: "feature".into(),
                workspace_id: id.clone(),
                keeping: Keeping::App {
                    checkout: FOLDER.into()
                },
            }]
        );

        // With Herdr 1 installed and not chosen for new tabs, new work goes
        // there, which knows none of the app's workspaces; the checkout work
        // left is still one to come back to, through its mark.
        view.live.missing_installation = false;
        let herdr_1 = view.teleport_host(0, host);
        assert_eq!(herdr_1.lands_in, Keeper::Herdr1);
        assert_eq!(herdr_1.retired.len(), 1);
        assert_eq!(herdr_1.repositories, None);
    });
}
