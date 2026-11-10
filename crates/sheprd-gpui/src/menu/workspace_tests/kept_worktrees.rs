//! Worktrees of a workspace the app keeps for Herdr 2: the menu offers them,
//! the dialog's rows ask Git for the right checkout, and finished Git work
//! lands in a kept workspace or closes one. Git itself is exercised in
//! `sheprd_remote::work`; here its answers are handed to the window.

use super::*;
use crate::{
    browser::Location,
    core_terminals::panes::Panes,
    menu::{
        KeptDone, Page,
        workspace::{Ask, KeptLanding},
        worktree_source::Pending,
    },
    repo_items::Branch,
    sidebar::layout_tests::{SidebarFixture, fixture_window},
};
use gpui::{AppContext, Entity, TestAppContext, VisualTestContext};
use sheprd_backend::TerminalId;
use sheprd_vcs::git::Identity;
use std::path::Path;

const LINKED: &str = "/src/app-feature";
const MAIN: &str = "/src/app";
const PLAIN: &str = "/src/notes";
const NEW: &str = "/src/app-lane";

/// A window with no Herdr 1, keeping a workspace at `folder`, which Git
/// says is a checkout of `app` when `linked` is given.
fn window<'a>(
    cx: &'a mut TestAppContext,
    folder: &str,
    linked: Option<bool>,
) -> (Entity<SheprdWindow>, &'a mut VisualTestContext, String) {
    let (fixture, cx) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| fixture_window(window, cx));
        SidebarFixture(view)
    });
    let view = cx.update(|_, cx| fixture.read(cx).0.clone());
    let id = cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.live.snapshot = None;
            view.live.missing_installation = true;
            view.core_terminals.seed_checkout_for_test(
                folder,
                linked.map(|linked| Identity {
                    branch: Some(if linked { "feature" } else { "main" }.into()),
                    key: Path::new(MAIN).join(".git").display().to_string(),
                    label: "app".into(),
                    linked,
                }),
            );
            let location =
                Location::core_terminal(Panes::single(TerminalId::new("t1").unwrap()), false);
            view.open_kept_workspace(None, Path::new(folder), location, window, cx)
        })
    });
    (view, cx, id.unwrap())
}

fn rows(view: &Entity<SheprdWindow>, cx: &mut VisualTestContext, id: &str) -> Vec<WorkspaceAction> {
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.open_workspace_menu(id, gpui::Point::default(), window, cx);
            view.workspace_items()
                .into_iter()
                .filter_map(|(action, _)| match action {
                    WorkspaceMenuAction::Dialog(action) => Some(action),
                    _ => None,
                })
                .collect()
        })
    })
}

#[gpui::test]
fn a_kept_linked_checkout_offers_new_and_deleted_worktrees(cx: &mut TestAppContext) {
    let (view, cx, id) = window(cx, LINKED, Some(true));
    let rows = rows(&view, cx, &id);
    assert_eq!(rows.first(), Some(&WorkspaceAction::NewWorktree));
    assert_eq!(rows.last(), Some(&WorkspaceAction::DeleteWorktree));
}

#[gpui::test]
fn a_kept_main_checkout_is_never_offered_for_deletion(cx: &mut TestAppContext) {
    let (view, cx, id) = window(cx, MAIN, Some(false));
    let rows = rows(&view, cx, &id);
    assert!(rows.contains(&WorkspaceAction::NewWorktree));
    assert!(!rows.contains(&WorkspaceAction::DeleteWorktree));
}

#[gpui::test]
fn a_kept_folder_outside_git_offers_no_worktrees(cx: &mut TestAppContext) {
    let (view, cx, id) = window(cx, PLAIN, None);
    let rows = rows(&view, cx, &id);
    assert!(!rows.contains(&WorkspaceAction::NewWorktree));
    assert!(!rows.contains(&WorkspaceAction::DeleteWorktree));
}

#[gpui::test]
fn the_new_worktree_dialog_reads_the_kept_folder(cx: &mut TestAppContext) {
    let (view, cx, id) = window(cx, LINKED, Some(true));
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.open_workspace_dialog_for(
                &id,
                WorkspaceAction::NewWorktree,
                gpui::Point::default(),
                window,
                cx,
            );
            assert_eq!(view.kept_checkout().as_deref(), Some(LINKED));
            assert!(view.menu.worktree.is_some());
            assert!(view.menu.dispatch.is_none(), "no other host makes them");
            // Read through Git's names, with no daemon to ask.
            let input = view
                .menu_target_input(&crate::pull_request::Origin::Local)
                .unwrap();
            assert_eq!(input.checkout.as_deref(), Some(LINKED));
            assert_eq!(input.branch, "feature");
        })
    });
}

#[test]
fn picked_rows_ask_git_for_their_checkout() {
    let branch = Pending::Branch(Branch {
        name: "fix/login".into(),
    });
    assert_eq!(
        Ask::of(&branch, "HEAD".into()).unwrap(),
        Ask::Create {
            branch: "fix/login".into(),
            base: "HEAD".into(),
            label: None,
        }
    );
    let open = Pending::Checkout(NEW.into());
    assert_eq!(
        Ask::of(&open, "HEAD".into()).unwrap(),
        Ask::Open { path: NEW.into() }
    );
    let bad = Pending::Branch(Branch { name: "-x".into() });
    assert!(Ask::of(&bad, "HEAD".into()).is_err());
}

#[gpui::test]
fn a_made_checkout_lands_in_a_kept_workspace(cx: &mut TestAppContext) {
    let (view, cx, id) = window(cx, LINKED, Some(true));
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.open_workspace_dialog_for(
                &id,
                WorkspaceAction::NewWorktree,
                gpui::Point::default(),
                window,
                cx,
            );
            view.menu.creation = Some("kept-1".into());
            view.kept_worktree_done.push(KeptDone::Checkout {
                request: "kept-1".into(),
                landing: Box::new(KeptLanding::shell(Some("lane"))),
                result: Ok((
                    Ask::Create {
                        branch: "lane".into(),
                        base: "HEAD".into(),
                        label: None,
                    },
                    NEW.into(),
                )),
            });
            view.poll_kept_worktree(window, cx);
        })
    });
    view.read_with(cx, |view, _| {
        assert!(view.menu.page.is_none(), "the dialog closed");
        assert!(view.menu.creation.is_none());
        let kept = view.kept_workspace_at(Path::new(NEW)).unwrap();
        assert_eq!(view.kept_in_front(), Some(kept.as_str()));
        let sessions = view.sessions(0);
        assert_eq!(sessions.workspace(&kept).unwrap().label, "lane");
    });
}

#[gpui::test]
fn a_refused_checkout_keeps_the_dialog_open(cx: &mut TestAppContext) {
    let (view, cx, id) = window(cx, LINKED, Some(true));
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.open_workspace_dialog_for(
                &id,
                WorkspaceAction::NewWorktree,
                gpui::Point::default(),
                window,
                cx,
            );
            view.menu.creation = Some("kept-2".into());
            view.kept_worktree_done.push(KeptDone::Checkout {
                request: "kept-2".into(),
                landing: Box::new(KeptLanding::shell(None)),
                result: Err(crate::Error::StaleWorkspace),
            });
            view.poll_kept_worktree(window, cx);
        })
    });
    view.read_with(cx, |view, _| {
        assert_eq!(
            view.menu.page,
            Some(Page::Dialog(WorkspaceAction::NewWorktree))
        );
        assert!(view.menu.error.is_some());
        assert!(view.kept_workspace_at(Path::new(NEW)).is_none());
    });
}

#[gpui::test]
fn deleting_a_kept_checkout_names_its_folder_and_closes_it(cx: &mut TestAppContext) {
    let (view, cx, id) = window(cx, LINKED, Some(true));
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.open_workspace_dialog_for(
                &id,
                WorkspaceAction::DeleteWorktree,
                gpui::Point::default(),
                window,
                cx,
            );
            let deletion = view.menu.deletion.as_mut().unwrap();
            assert_eq!(deletion.path.as_deref(), Some(LINKED));
            deletion.pending = Some("kept-3".into());
            view.kept_worktree_done.push(KeptDone::Removed {
                scope: crate::browser::scope(&view.endpoints[0]),
                request: "kept-3".into(),
                workspace: id.clone(),
                result: Ok(()),
            });
            view.poll_kept_worktree(window, cx);
        })
    });
    view.read_with(cx, |view, _| {
        assert!(view.menu.page.is_none());
        assert!(!view.is_kept_workspace(&id), "its workspace closed");
    });
}

#[gpui::test]
fn a_kept_checkout_with_changes_asks_to_discard_them(cx: &mut TestAppContext) {
    let (view, cx, id) = window(cx, LINKED, Some(true));
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.open_workspace_dialog_for(
                &id,
                WorkspaceAction::DeleteWorktree,
                gpui::Point::default(),
                window,
                cx,
            );
            view.menu.deletion = Some(Deletion {
                pending: Some("kept-4".into()),
                path: Some(LINKED.into()),
                ..Deletion::new(None, false)
            });
            view.kept_worktree_done.push(KeptDone::Removed {
                scope: crate::browser::scope(&view.endpoints[0]),
                request: "kept-4".into(),
                workspace: id.clone(),
                result: Err(sheprd_remote::work::Error::Dirty.into()),
            });
            view.poll_kept_worktree(window, cx);
        })
    });
    view.read_with(cx, |view, _| {
        assert!(view.menu.deletion.as_ref().unwrap().force);
        assert!(view.menu.error.is_some());
        assert!(view.is_kept_workspace(&id), "nothing was removed");
    });
}

#[gpui::test]
fn work_finishing_together_all_lands(cx: &mut TestAppContext) {
    let (view, cx, _) = window(cx, LINKED, Some(true));
    let other = "/src/app-other";
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            for (request, path) in [("kept-5", NEW), ("kept-6", other)] {
                view.kept_worktree_done.push(KeptDone::Checkout {
                    request: request.into(),
                    landing: Box::new(KeptLanding::shell(None)),
                    result: Ok((Ask::Open { path: path.into() }, path.into())),
                });
            }
            view.poll_kept_worktree(window, cx);
        })
    });
    view.read_with(cx, |view, _| {
        assert!(view.kept_workspace_at(Path::new(NEW)).is_some());
        assert!(view.kept_workspace_at(Path::new(other)).is_some());
        assert!(view.kept_worktree_done.is_empty());
    });
}

#[gpui::test]
fn another_hosts_kept_workspace_offers_no_local_git(cx: &mut TestAppContext) {
    let (view, cx, id) = window(cx, LINKED, Some(true));
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.open_workspace_menu(&id, gpui::Point::default(), window, cx);
            // Shown as if kept on an SSH host: its folder is that host's,
            // which Git on this computer must never touch.
            view.endpoints.push(crate::endpoint::Endpoint::new(
                "ssh:box".into(),
                "Box".into(),
                sheprd_client::ConnectTarget::Ssh {
                    target: "box".into(),
                    session: "work".into(),
                },
                true,
            ));
            view.selected_endpoint = view.endpoints.len() - 1;
            assert!(view.kept_checkout().is_none());
            let rows: Vec<_> = view
                .workspace_items()
                .into_iter()
                .map(|(action, _)| action)
                .collect();
            assert!(!rows.contains(&WorkspaceMenuAction::Dialog(WorkspaceAction::NewWorktree)));
            assert!(!rows.contains(&WorkspaceMenuAction::Dialog(
                WorkspaceAction::DeleteWorktree
            )));
        })
    });
}

#[gpui::test]
fn a_kept_dialog_outlives_the_tick_without_herdr_1(cx: &mut TestAppContext) {
    let (view, cx, id) = window(cx, LINKED, Some(true));
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.open_workspace_dialog_for(
                &id,
                WorkspaceAction::DeleteWorktree,
                gpui::Point::default(),
                window,
                cx,
            );
            // No Herdr 1 daemon or boot to wait on: the window's tick must
            // leave the confirmation for the user.
            view.update_workspace_dialog(window, cx);
            assert!(view.menu.deletion.is_some());
            assert_eq!(
                view.menu.page,
                Some(Page::Dialog(WorkspaceAction::DeleteWorktree))
            );
        })
    });
}

mod workflows;
