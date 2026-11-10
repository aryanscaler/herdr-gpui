//! Workspaces the app keeps for Herdr 2 on a saved SSH, cloud, or WSL host:
//! kept under that host's scope, listed with its sessions, and their new
//! terminals given that host's folders and daemon.

use crate::{
    SheprdWindow,
    browser::{Location, Scope, Store},
    core_terminals::{Host, Purpose, panes::Panes},
    sidebar::layout_tests::fixture_window,
};
use gpui::{Entity, TestAppContext, VisualTestContext};
use sheprd_backend::{Spawn, TerminalId};
use sheprd_client::ConnectTarget;
use std::{path::Path, sync::Arc};

const REMOTE: &str = "ssh:box";

fn scope() -> Scope {
    Scope::endpoint(REMOTE)
}

fn env<'a>(spawn: &'a Spawn, name: &str) -> Option<&'a str> {
    spawn
        .env
        .iter()
        .find(|(known, _)| known == name)
        .map(|(_, value)| value.as_str())
}

/// A window showing a saved SSH host with Herdr 2 chosen for new terminals.
/// With `herdr_1`, the host's Herdr shows the fixture's workspaces, w0's
/// first pane in `/home/me/app`; this computer keeps its own.
fn window(
    cx: &mut TestAppContext,
    herdr_1: bool,
) -> (Entity<SheprdWindow>, &mut VisualTestContext) {
    cx.add_window_view(|window, cx| {
        let mut view = fixture_window(window, cx);
        view.config.herdr_2_terminals = true;
        let mut remote = crate::endpoint::Endpoint::new(
            REMOTE.into(),
            "Box".into(),
            ConnectTarget::Ssh {
                target: "box".into(),
                session: "work".into(),
            },
            true,
        );
        if herdr_1 {
            let mut live = view.live.clone();
            let snapshot = Arc::make_mut(live.snapshot.as_mut().unwrap());
            snapshot.panes.push(
                serde_json::from_value(serde_json::json!({
                    "pane_id": "p0", "workspace_id": "w0", "tab_id": "t0", "label": null,
                    "cwd": "/home/me/app", "focused": true,
                    "right_click_passthrough": false
                }))
                .unwrap(),
            );
            remote.live = live;
        }
        view.endpoints.push(remote);
        view.endpoints[0].live = std::mem::take(&mut view.live);
        view.selected_endpoint = 1;
        view.live = view.endpoints[1].live.clone();
        view
    })
}

fn spawn_for(view: &Entity<SheprdWindow>, cx: &mut VisualTestContext, workspace: &str) -> Spawn {
    let mut spawn = Spawn::default();
    view.update(cx, |view, cx| {
        let purpose = Purpose::Tab {
            scope: scope(),
            workspace: workspace.into(),
        };
        view.give_core_context(&purpose, &Host::Remote(scope()), &mut spawn, cx)
    });
    spawn
}

#[gpui::test]
fn a_saved_host_keeps_herdr_2_workspaces_of_its_own(cx: &mut TestAppContext) {
    let (view, cx) = window(cx, false);
    view.read_with(cx, |view, _| {
        assert!(view.new_workspaces_use_core(), "Herdr 2 is chosen");
    });
    let folder = Path::new("/home/me/api");
    let id = cx
        .update(|window, cx| {
            view.update(cx, |view, cx| {
                let location =
                    Location::core_terminal(Panes::single(TerminalId::new("t1").unwrap()), false);
                view.open_kept_workspace(None, folder, location, window, cx)
            })
        })
        .expect("a saved host keeps workspaces");
    view.read_with(cx, |view, cx| {
        assert_eq!(view.kept_in_front(), Some(id.as_str()));
        assert_eq!(view.kept_workspace_at(folder).as_deref(), Some(id.as_str()));
        // Kept under the host's scope and listed with its sessions only.
        let session = view.sessions(1).core.unwrap();
        assert_eq!(session.workspace(&id).unwrap().cwd, "/home/me/api");
        assert!(
            view.sessions(0)
                .core
                .is_none_or(|local| local.workspace(&id).is_none())
        );
        let store = cx.global::<Store>();
        assert_eq!(store.in_workspace(&scope(), &id).count(), 1);
        assert!(store.kept_workspace(&scope(), &id).is_some());
    });
    // A new tab there starts in the workspace's folder on that host, and
    // names no Herdr workspace: none holds it.
    let spawn = spawn_for(&view, cx, &id);
    assert_eq!(spawn.cwd.as_deref(), Some(folder));
    assert_eq!(env(&spawn, "HERDR_WORKSPACE_ID"), Some(""));
    assert_eq!(env(&spawn, "HERDR_SESSION"), None);
    // Closing it ends its terminal on that host's core, once it answers.
    cx.update(|window, cx| view.update(cx, |view, cx| view.close_kept_workspace(&id, window, cx)));
    view.read_with(cx, |view, _| {
        assert!(view.kept_in_front().is_none());
        assert!(view.sessions(1).core.is_none(), "nothing kept there now");
        let unended = view.core_terminals.unended.lock().unwrap().clone();
        assert!(unended.contains(&(Host::Remote(scope()), TerminalId::new("t1").unwrap())));
    });
}

#[gpui::test]
fn a_tab_in_a_saved_hosts_herdr_workspace_starts_there_naming_none(cx: &mut TestAppContext) {
    let (view, cx) = window(cx, true);
    let spawn = spawn_for(&view, cx, "w0");
    // Where the workspace started on that host.
    assert_eq!(spawn.cwd.as_deref(), Some(Path::new("/home/me/app")));
    // Its daemon's socket is under that host's home, unknown here, and an
    // inherited one cannot be cleared, so no Herdr workspace is named.
    assert_eq!(env(&spawn, "HERDR_WORKSPACE_ID"), Some(""));
    assert_eq!(env(&spawn, "HERDR_SESSION"), None);
    assert_eq!(env(&spawn, "HERDR_SOCKET_PATH"), None);
    // A workspace that host's Herdr does not list has no folder to give.
    let spawn = spawn_for(&view, cx, "w99");
    assert_eq!(spawn.cwd, None);
}

#[gpui::test]
fn without_herdr_2_chosen_a_saved_hosts_herdr_keeps_new_workspaces(cx: &mut TestAppContext) {
    let (view, cx) = window(cx, true);
    view.update(cx, |view, _| {
        view.config.herdr_2_terminals = false;
        assert!(!view.new_workspaces_use_core());
    });
}

#[gpui::test]
fn a_saved_host_without_herdr_1_keeps_a_workspace_with_a_shell(cx: &mut TestAppContext) {
    let (view, cx) = window(cx, false);
    view.update(cx, |view, cx| {
        view.config.herdr_2_terminals = false;
        // The bridge found no Herdr 1 there; the core is never dialled here.
        view.live.ssh_failure = Some(sheprd_client::SshFailure::HerdrMissing);
        let target = view.endpoints[1].connection.target.clone();
        view.core_terminals.set_remote_hub_for_test(
            scope(),
            target,
            crate::core_terminals::Hub::with_join(|_| {
                Err(crate::core_terminals::Error::Bridge(
                    sheprd_client::core_bridge::Error::Missing.into(),
                ))
            }),
        );
        view.refresh_core_session(cx);
        assert_eq!(view.kept_in_front(), Some(crate::core_terminals::WORKSPACE));
        assert!(view.new_workspaces_use_core(), "no Herdr 1 to make them");
        let store = cx.global::<Store>();
        assert!(
            store
                .kept_workspace(&scope(), crate::core_terminals::WORKSPACE)
                .is_some()
        );
        // Its first shell waits for that host's core.
        assert!(view.core_terminals.wanted.iter().any(|(purpose, host, ..)| {
            *host == Host::Remote(scope())
                && matches!(purpose, Purpose::Tab { workspace, .. } if workspace == crate::core_terminals::WORKSPACE)
        }));
        // This computer keeps none for it.
        assert!(
            view.sessions(0)
                .core
                .is_none_or(|local| local.workspaces.is_empty())
        );
    });
}
