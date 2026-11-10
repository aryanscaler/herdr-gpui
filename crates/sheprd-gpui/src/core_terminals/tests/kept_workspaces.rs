//! Workspaces the app keeps for Herdr 2: listed beside Herdr 1's, brought
//! to the front without a daemon, and renamed, moved, and closed by the app.

use crate::{
    SheprdWindow,
    browser::{Location, Store},
    core_terminals::{WORKSPACE, panes::Panes},
    reorder::Beside,
    sidebar::layout_tests::{SidebarFixture, fixture_window, snapshot},
};
use gpui::{Entity, TestAppContext, VisualTestContext, prelude::*};
use sheprd_backend::TerminalId;
use std::{path::Path, sync::Arc};

pub(super) fn window(
    cx: &mut TestAppContext,
    herdr_1: bool,
) -> (Entity<SheprdWindow>, &mut VisualTestContext) {
    let (fixture, cx) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| fixture_window(window, cx));
        SidebarFixture(view)
    });
    let view = cx.update(|_, cx| fixture.read(cx).0.clone());
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            if herdr_1 {
                let mut snapshot = snapshot(2);
                snapshot.focused_workspace_id = Some("w0".into());
                view.live.snapshot = Some(Arc::new(snapshot));
            } else {
                view.live.snapshot = None;
                view.live.missing_installation = true;
            }
            view.refresh_core_session(cx);
        })
    });
    (view, cx)
}

fn front(view: &Entity<SheprdWindow>, cx: &mut VisualTestContext) -> String {
    view.read_with(cx, |view, _| view.browser_key().unwrap().1)
}

fn listed(view: &Entity<SheprdWindow>, cx: &mut VisualTestContext) -> Vec<String> {
    view.read_with(cx, |view, _| {
        view.sidebar_workspaces()
            .into_iter()
            .map(|(_, id)| id)
            .collect()
    })
}

pub(super) fn core_tab(id: &str) -> Location {
    Location::core_terminal(Panes::single(TerminalId::new(id).unwrap()), false)
}

#[gpui::test]
fn without_herdr_1_the_window_keeps_its_workspaces(cx: &mut TestAppContext) {
    let (view, cx) = window(cx, false);
    assert_eq!(listed(&view, cx), [WORKSPACE]);
    assert_eq!(front(&view, cx), WORKSPACE);
    let folder = Path::new("/src/api");
    let id = cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.open_kept_workspace(None, folder, core_tab("t1"), window, cx)
        })
    });
    let id = id.unwrap();
    assert_eq!(listed(&view, cx), [WORKSPACE, id.as_str()]);
    assert_eq!(front(&view, cx), id, "a new workspace comes to the front");
    view.read_with(cx, |view, cx| {
        assert_eq!(view.kept_workspace_at(folder).as_deref(), Some(id.as_str()));
        assert_eq!(view.kept_workspace_at(Path::new("/src/web")), None);
        let session = view.sessions(0).core.unwrap();
        assert_eq!(session.workspace(&id).unwrap().label, "api");
        let store = cx.global::<Store>();
        let scope = crate::browser::scope(&view.endpoints[0]);
        assert_eq!(store.in_workspace(&scope, &id).count(), 1);
    });
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            assert!(view.navigate(crate::NavigationTarget::Workspace(WORKSPACE), cx));
        })
    });
    assert_eq!(front(&view, cx), WORKSPACE, "no daemon needs to answer");
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.rename_kept_workspace(&id, "backend", cx);
            assert!(view.move_kept_workspace(&id, Beside::Before(WORKSPACE), cx));
        })
    });
    assert_eq!(listed(&view, cx), [id.as_str(), WORKSPACE]);
    view.read_with(cx, |view, _| {
        let session = view.sessions(0).core.unwrap();
        assert_eq!(session.workspace(&id).unwrap().label, "backend");
    });
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.navigate(crate::NavigationTarget::Workspace(&id), cx);
            view.close_kept_workspace(&id, window, cx);
        })
    });
    assert_eq!(listed(&view, cx), [WORKSPACE]);
    assert_eq!(front(&view, cx), WORKSPACE, "the front falls back");
}

#[gpui::test]
fn with_herdr_1_kept_workspaces_list_after_its_own(cx: &mut TestAppContext) {
    let (view, cx) = window(cx, true);
    assert_eq!(listed(&view, cx), ["w0", "w1"], "none kept until asked");
    view.read_with(cx, |view, _| {
        assert!(view.kept_in_front().is_none());
        assert!(!view.new_workspaces_use_core(), "the setting is off");
    });
    let id = cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.open_kept_workspace(
                Some("tools"),
                Path::new("/src/tools"),
                core_tab("t2"),
                window,
                cx,
            )
        })
    });
    let id = id.unwrap();
    assert_eq!(listed(&view, cx), ["w0", "w1", id.as_str()]);
    assert_eq!(front(&view, cx), id);
    view.read_with(cx, |view, _| {
        assert_eq!(view.herdr_workspace_in_front(), None);
        assert_eq!(
            view.herdr_tabs_in_front().count(),
            0,
            "the strip shows none of Herdr 1's tabs"
        );
        assert!(
            view.new_tabs_use_core(),
            "a tab in a kept workspace is Herdr 2's"
        );
    });
    // Choosing anything of Herdr 1's hands it the front back.
    cx.update(|_, cx| view.update(cx, |view, _| view.leave_kept_workspace()));
    view.read_with(cx, |view, _| {
        assert_eq!(view.browser_key().unwrap().1, "w0");
        assert_eq!(view.herdr_workspace_in_front(), Some("w0"));
        assert!(
            view.herdr_tabs_in_front()
                .all(|tab| tab.workspace_id == "w0")
        );
        assert!(view.herdr_tabs_in_front().next().is_some());
    });
}

#[gpui::test]
fn a_new_tab_starts_in_its_kept_workspaces_folder(cx: &mut TestAppContext) {
    let (view, cx) = window(cx, true);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.open_kept_workspace(None, Path::new("/src/tools"), core_tab("t3"), window, cx);
            view.new_core_terminal(window, cx);
            let (purpose, _, spawn, _) = view.core_terminals.wanted.back().unwrap();
            let (purpose, spawn) = (purpose.clone(), spawn.clone());
            assert_eq!(spawn.cwd.as_deref(), Some(Path::new("/src/tools")));
            let crate::core_terminals::Purpose::Tab { scope, workspace } = purpose else {
                panic!("a tab");
            };
            // Closing the workspace while its shell starts leaves the shell
            // nowhere to go: it is ended once it answers.
            assert!(!view.kept_workspace_gone(&scope, &workspace, cx));
            view.close_kept_workspace(&workspace, window, cx);
            assert!(view.kept_workspace_gone(&scope, &workspace, cx));
            assert!(!view.kept_workspace_gone(&scope, "w0", cx), "Herdr 1's");
        })
    });
}

#[gpui::test]
fn workspaces_made_in_a_row_each_start_a_shell(cx: &mut TestAppContext) {
    let (view, cx) = window(cx, false);
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.new_kept_workspace(Some("api"), Some("/src/api".into()), cx);
            view.new_kept_workspace(Some("web"), Some("/src/web".into()), cx);
            // No core answers in tests, so both wait their turn in order
            // rather than the second replacing the first.
            let folders: Vec<_> = view
                .core_terminals
                .wanted
                .iter()
                .map(|(_, _, spawn, _)| spawn.cwd.clone().unwrap())
                .collect();
            assert_eq!(folders, [Path::new("/src/api"), Path::new("/src/web")]);
        })
    });
}

#[gpui::test]
fn a_workspace_whose_first_tab_cannot_open_is_not_kept(cx: &mut TestAppContext) {
    let (view, cx) = window(cx, false);
    cx.update(|_, cx| {
        let scope = crate::browser::scope(&view.read(cx).endpoints[0]);
        Store::update(cx, |store| {
            while store.open(scope.clone(), WORKSPACE, None, None).is_some() {}
        });
    });
    let id = cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.open_kept_workspace(None, Path::new("/src/api"), core_tab("t1"), window, cx)
        })
    });
    assert_eq!(id, None, "no tab could show it");
    assert_eq!(listed(&view, cx), [WORKSPACE]);
    assert_eq!(front(&view, cx), WORKSPACE);
}

#[gpui::test]
fn closing_the_default_workspace_lets_its_shell_go(cx: &mut TestAppContext) {
    let (view, cx) = window(cx, false);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            assert!(view.default_workspace_kept(cx), "its shell is shown");
            view.new_kept_workspace(Some("api"), None, cx);
            view.close_kept_workspace(WORKSPACE, window, cx);
            // The shell standing in for Herdr 1 is ended on the next poll,
            // and none starts while the workspace is not kept.
            assert!(!view.default_workspace_kept(cx));
        })
    });
}

#[gpui::test]
fn a_local_project_opens_in_herdr_2_while_another_host_is_shown(cx: &mut TestAppContext) {
    let (view, cx) = window(cx, true);
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.config.herdr_2_terminals = true;
            let remote = crate::endpoint::Endpoint::new(
                "ssh:test".into(),
                "Remote".into(),
                sheprd_client::ConnectTarget::Ssh {
                    target: "unused".into(),
                    session: "default".into(),
                },
                true,
            );
            view.endpoints.push(remote);
            view.endpoints[0].live = std::mem::take(&mut view.live);
            view.selected_endpoint = 1;
            // Herdr 2 is chosen, and nothing says the host lacks a core.
            assert!(view.new_workspaces_use_core(), "the shown host keeps one");
            assert!(view.local_workspaces_use_core(), "a project is local");
            view.open_local_kept_folder("api", "/src/api".into(), cx);
            assert_eq!(view.selected_endpoint, 0, "this computer is shown");
            let id = view.kept_workspace_at(Path::new("/src/api")).unwrap();
            assert_eq!(view.kept_in_front(), Some(id.as_str()));
            // Opening it again finds the same workspace.
            view.open_local_kept_folder("api", "/src/api".into(), cx);
            assert_eq!(view.kept_workspace_at(Path::new("/src/api")), Some(id));
        })
    });
}

mod behind;
mod run_terminal;
