//! Smart dispatch with this computer keeping its workspaces for Herdr 2:
//! it counts as a host to send work to, a kept workspace's dialog sends work
//! elsewhere, and a finished dispatch here lands in a kept workspace. The
//! scripts are exercised in `sheprd_remote::teleport`; here their answers
//! are handed to the window.

use super::*;
use crate::{
    browser::Location,
    core_terminals::panes::Panes,
    dispatch::{Dispatched, Job, Repository, Then},
    sidebar::layout_tests::{SidebarFixture, fixture_window},
    teleport::{Created, Naming},
};
use gpui::{AppContext, Entity, TestAppContext, VisualTestContext};
use sheprd_backend::TerminalId;
use sheprd_vcs::git::Identity;
use std::path::Path;

const KEY: &str = "/src/app/.git";
const LINKED: &str = "/src/app-feature";
const NEW: &str = "/src/app-lane";

/// A window on this computer with no Herdr 1, keeping a workspace at
/// `LINKED`, a checkout of `app`, beside an SSH host "Box" that runs
/// Herdr 1 when `herdr_1` is set, else only herdr-core.
fn window(
    cx: &mut TestAppContext,
    herdr_1: bool,
) -> (Entity<SheprdWindow>, &mut VisualTestContext, String) {
    let (fixture, cx) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| fixture_window(window, cx));
        SidebarFixture(view)
    });
    let view = cx.update(|_, cx| fixture.read(cx).0.clone());
    let id = cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            let mut remote = crate::endpoint::Endpoint::new(
                "ssh:box".into(),
                "Box".into(),
                sheprd_client::ConnectTarget::Ssh {
                    target: "nobody@box.invalid".into(),
                    session: "default".into(),
                },
                true,
            );
            remote.live = view.live.clone();
            if herdr_1 {
                remote.live.status = crate::state::ConnectionStatus::Connected;
            } else {
                remote.live.snapshot = None;
            }
            view.endpoints.push(remote);
            view.endpoints[0].connection.target = sheprd_client::ConnectTarget::Local;
            view.live.snapshot = None;
            view.live.missing_installation = true;
            view.core_terminals.seed_checkout_for_test(
                LINKED,
                Some(Identity {
                    branch: Some("feature".into()),
                    key: KEY.into(),
                    label: "app".into(),
                    linked: true,
                }),
            );
            let location =
                Location::core_terminal(Panes::single(TerminalId::new("t1").unwrap()), false);
            view.open_kept_workspace(None, Path::new(LINKED), location, window, cx)
        })
    });
    (view, cx, id.unwrap())
}

#[gpui::test]
fn this_computer_takes_work_while_it_keeps_its_workspaces(cx: &mut TestAppContext) {
    let (view, cx, _) = window(cx, false);
    cx.update(|_, cx| {
        let view = view.read(cx);
        let local = view.endpoints[0].id.clone();
        assert!(view.dispatch_keeps(&local));
        assert!(!view.dispatch_keeps("ssh:box"), "no remote herdr-core yet");
        let candidates = view.dispatch_candidates("app", cx);
        let here = candidates.iter().find(|c| c.endpoint_id == local).unwrap();
        assert!(here.online, "online through Herdr 2 alone");
        assert_eq!(here.repository, Repository::Present);
        let other = candidates.iter().find(|c| c.endpoint_id == "ssh:box");
        assert!(!other.unwrap().online, "herdr-core alone takes no dispatch");
        // Discovery reads the kept workspaces, never Herdr 1's snapshot.
        let destination = view.dispatch_destination(&local).unwrap();
        let repositories = destination.repositories.unwrap();
        assert_eq!(repositories.len(), 1);
        assert_eq!(repositories[0].key, KEY);
        assert_eq!(repositories[0].label, "app");
    });
}

#[gpui::test]
fn a_kept_workspace_sends_its_new_worktree_to_another_host(cx: &mut TestAppContext) {
    let (view, cx, id) = window(cx, true);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.open_workspace_menu(&id, gpui::Point::default(), window, cx);
            view.open_workspace_dialog(WorkspaceAction::NewWorktree, window, cx);
            let picker = view.menu.dispatch.as_mut().unwrap();
            assert!(picker.online("ssh:box").is_some());
            assert_eq!(view.dispatch_choice(WorkspaceAction::NewWorktree), None);
            assert!(view.menu.dispatch.as_mut().unwrap().choose("ssh:box"));
            assert_eq!(
                view.dispatch_choice(WorkspaceAction::NewWorktree),
                Some(crate::dispatch::Target::Host("ssh:box".into()))
            );
        })
    });
}

#[test]
fn kept_work_names_its_branch() {
    let named = Then::Worktree {
        base: "HEAD".into(),
        naming: Naming {
            branch: Some("lane".into()),
            label: Some("Lane".into()),
        },
    };
    assert_eq!(
        named.kept(),
        Then::KeptWorktree {
            base: "HEAD".into(),
            branch: "lane".into(),
            label: Some("Lane".into()),
        }
    );
    // Git needs a name where Herdr 1's daemon would have picked one.
    let unnamed = Then::Worktree {
        base: "HEAD".into(),
        naming: Naming::default(),
    };
    let Then::KeptWorktree { branch, .. } = unnamed.kept() else {
        panic!("a kept worktree");
    };
    assert!(crate::worktree::validate_branch(&branch).is_ok());
    assert_eq!(
        Then::Workspace { label: None }.kept(),
        Then::KeptWorkspace { label: None }
    );
}

#[gpui::test]
fn a_dispatch_here_lands_in_a_kept_workspace(cx: &mut TestAppContext) {
    let (view, cx, _) = window(cx, false);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            let endpoint_id = view.endpoints[0].id.clone();
            view.dispatch_job = Some(Job::finished(
                "This Mac",
                "app",
                Ok(Dispatched::Kept {
                    endpoint_id,
                    path: NEW.into(),
                    label: Some("Lane".into()),
                    checkout: Some(crate::teleport::NewCheckout {
                        repo_key: "/src/app/.git".into(),
                        path: NEW.into(),
                        root: Some("/src/app".into()),
                    }),
                }),
            ));
            view.poll_dispatch(window, cx);
        })
    });
    view.read_with(cx, |view, _| {
        assert!(view.dispatch_job.is_none());
        let kept = view.kept_workspace_at(Path::new(NEW)).unwrap();
        assert_eq!(view.kept_in_front(), Some(kept.as_str()));
        assert_eq!(view.sessions(0).workspace(&kept).unwrap().label, "Lane");
        // Its setup waits to run in a tab of the workspace just kept.
        let setup = view.queued_kept_setup().unwrap();
        assert_eq!(setup.repo_key, "/src/app/.git");
        assert_eq!(setup.checkout.as_ref().unwrap().path, NEW);
        assert!(
            view.teleport_follow.is_none(),
            "no Herdr 1 workspace to follow"
        );
    });
}

#[gpui::test]
fn a_herdr_1_dispatch_still_follows_its_workspace(cx: &mut TestAppContext) {
    let (view, cx, _) = window(cx, true);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.dispatch_job = Some(Job::finished(
                "Box",
                "app",
                Ok(Dispatched::Herdr1(Created {
                    endpoint_id: "ssh:box".into(),
                    workspace_id: "w9".into(),
                    checkout: None,
                })),
            ));
            view.poll_dispatch(window, cx);
        })
    });
    view.read_with(cx, |view, _| {
        assert!(view.teleport_follow.is_some());
        assert!(view.kept_workspace_at(Path::new(NEW)).is_none());
    });
}

#[gpui::test]
fn herdr_1_callers_never_get_kept_workspace_ids(cx: &mut TestAppContext) {
    let (view, cx, _) = window(cx, false);
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            // Herdr 1 runs here too, with the repository open, while new
            // workspaces are kept for Herdr 2.
            view.config.herdr_2_terminals = true;
            view.live.missing_installation = false;
            view.live.status = crate::state::ConnectionStatus::Connected;
            view.live.snapshot = Some(std::sync::Arc::new(sidebar::layout_tests::snapshot(7)));
            view.refresh_core_session(cx);
            let local = view.endpoints[0].id.clone();
            assert!(view.dispatch_keeps(&local));
            let kept = view
                .dispatch_destination(&local)
                .unwrap()
                .repositories
                .unwrap();
            assert!(
                kept.iter()
                    .all(|repo| crate::browser::is_kept_id(&repo.workspace_id))
            );
            let herdr_1 = view
                .herdr_1_destination(&local)
                .unwrap()
                .repositories
                .unwrap();
            assert!(!herdr_1.is_empty(), "Herdr 1's repositories stay reachable");
            assert!(
                herdr_1
                    .iter()
                    .all(|repo| !crate::browser::is_kept_id(&repo.workspace_id))
            );
        })
    });
}
