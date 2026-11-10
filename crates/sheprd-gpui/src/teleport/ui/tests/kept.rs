//! A move that lands where the app keeps the workspaces: the app keeps a new
//! one for the checkout, or takes back the one the work left, and the window
//! follows to it with no Herdr 1 to list it.

use super::*;
use crate::{
    browser::{Location, Scope, Store},
    core_terminals::panes::Panes,
};
use core::prelude::v1::test;
use sheprd_backend::TerminalId;

fn panes(id: &str) -> Panes {
    Panes::single(TerminalId::new(id).unwrap())
}

fn outcome(workspace: Landed) -> Outcome {
    Outcome {
        endpoint_id: "local".into(),
        workspace,
        repo_key: "/src/app/.git".into(),
        branch: "feature".into(),
        warnings: Vec::new(),
        core_tabs: vec![panes("t5"), panes("t6")],
    }
}

fn tabs(cx: &App, scope: &Scope, workspace: &str) -> Vec<Panes> {
    cx.global::<Store>()
        .in_workspace(scope, workspace)
        .filter_map(|tab| match &tab.location {
            Some(Location::CoreTerminal { panes, .. }) => Some(panes.clone()),
            _ => None,
        })
        .collect()
}

#[gpui::test]
fn a_new_checkout_gets_a_kept_workspace_with_its_tabs(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(crate::sidebar::layout_tests::fixture_window);
    let scope = cx.update(|_, cx| view.read(cx).endpoint_scope("local"));
    let landed = |label: Option<&str>| {
        outcome(Landed::New {
            checkout: "/home/me/.herdr/worktrees/app/feature".into(),
            label: label.map(str::to_owned),
        })
    };
    cx.update(|window, cx| {
        let named = view
            .update(cx, |view, cx| {
                view.settle_teleport(&landed(None), &scope, window, cx)
            })
            .unwrap();
        let store = cx.global::<Store>();
        let kept = store.kept_workspace(&scope, &named).unwrap();
        assert_eq!(kept.cwd, "/home/me/.herdr/worktrees/app/feature");
        assert_eq!(kept.label, "feature");
        assert!(!kept.custom, "named after its folder");
        assert_eq!(tabs(cx, &scope, &named), [panes("t5"), panes("t6")]);

        // The user's own name for the source goes with it.
        let custom = view
            .update(cx, |view, cx| {
                view.settle_teleport(&landed(Some("Teleport work")), &scope, window, cx)
            })
            .unwrap();
        assert_ne!(custom, named);
        let kept = cx
            .global::<Store>()
            .kept_workspace(&scope, &custom)
            .unwrap();
        assert_eq!(kept.label, "Teleport work");
        assert!(kept.custom);
    });
}

/// Coming back to a kept checkout, its idle shell gives way to the work's
/// tabs, as a Herdr 1 checkout's tabs do in the move itself.
#[gpui::test]
fn a_reclaimed_kept_workspace_trades_its_tabs_for_the_works(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(crate::sidebar::layout_tests::fixture_window);
    let scope = cx.update(|_, cx| view.read(cx).endpoint_scope("local"));
    cx.update(|window, cx| {
        let id = Store::update(cx, |store| {
            let id = store
                .keep_workspace(&scope, "feature", "/src/app-feature", false)
                .unwrap();
            let idle = Location::core_terminal(panes("t1"), false);
            store.open(scope.clone(), &id, Some(idle), None).unwrap();
            id
        });
        let settled = view.update(cx, |view, cx| {
            view.settle_teleport(&outcome(Landed::Workspace(id.clone())), &scope, window, cx)
        });
        assert_eq!(settled.as_deref(), Some(id.as_str()));
        assert_eq!(tabs(cx, &scope, &id), [panes("t5"), panes("t6")]);
    });
}

/// The window follows to a kept workspace with no Herdr 1 snapshot to list
/// it, and stops following once it is in front.
#[gpui::test]
fn the_window_follows_to_a_kept_workspace(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(crate::sidebar::layout_tests::fixture_window);
    view.update(cx, |view, cx| {
        view.live.snapshot = None;
        let endpoint = view.endpoints[0].id.clone();
        let scope = view.endpoint_scope(&endpoint);
        let id = Store::update(cx, |store| {
            store.keep_workspace(&scope, "feature", "/src/app-feature", false)
        })
        .unwrap();
        view.refresh_core_session(cx);
        view.teleport_follow = Some(Follow::new(endpoint, id.clone()));
        view.follow_teleport(cx);
        assert_eq!(view.kept_in_front(), Some(id.as_str()));
        assert!(view.teleport_follow.is_none());
    });
}

/// A move into a new kept workspace is refused before it starts while the
/// app keeps the most workspaces it can: once it ended, its programs would
/// have no tab.
#[gpui::test]
fn a_full_workspace_list_refuses_the_move_before_it_starts(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(crate::sidebar::layout_tests::fixture_window);
    cx.update(|window, cx| {
        let scope = Scope::endpoint("full");
        Store::update(cx, |store| {
            while store.kept_room() > 0 {
                store.keep_workspace(&scope, "w", "/w", false).unwrap();
            }
        });
        view.update(cx, |view, cx| {
            let candidate = Candidate {
                lands_in: Keeper::App,
                ..candidate()
            };
            let mut teleport = Teleport::start(source(), "feature".into(), Vec::new());
            teleport.stage = Stage::Ready(candidate, Box::new(review(GitHubAccess::Direct)));
            view.teleport = Some(teleport);
            view.submit_teleport(window, cx);
            assert!(
                matches!(view.teleport.as_ref().unwrap().stage, Stage::Ready(..)),
                "nothing started"
            );
        });
    });
}

/// A kept source keeps one idle shell, which needs a tab of its own: a move
/// that would leave no room for it is refused before it starts.
#[gpui::test]
fn a_kept_sources_idle_shell_needs_room_for_its_tab(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(crate::sidebar::layout_tests::fixture_window);
    cx.update(|window, cx| {
        let scope = Scope::endpoint("full");
        Store::update(cx, |store| {
            while store.room() > 1 {
                store.open(scope.clone(), "w", None, None).unwrap();
            }
        });
        view.update(cx, |view, cx| {
            let source = Source {
                keeping: Keeping::App {
                    checkout: "/src/app-feature".into(),
                },
                ..source()
            };
            let mut teleport = Teleport::start(source, "feature".into(), Vec::new());
            // One tab moves into Herdr 2, which fits; the shell does not.
            teleport.stage = Stage::Ready(candidate(), Box::new(review(GitHubAccess::Direct)));
            view.teleport = Some(teleport);
            view.submit_teleport(window, cx);
            assert!(
                matches!(view.teleport.as_ref().unwrap().stage, Stage::Ready(..)),
                "nothing started"
            );
        });
    });
}
