//! Notification mutes of Herdr 2 tabs, from their menu, and what a muted tab
//! keeps quiet, beside its marks in the strip.

use super::commands::{id, window_with_split};
use crate::{
    browser::{Location, Store},
    core_terminals::{
        Host,
        menu::{Action, Target},
        mutes::MuteRow,
        panes::Panes,
    },
    icons::AgentIcon,
    notification_mutes::{Mutes, Scope},
    tab_layout::Mark,
};
use gpui::{TestAppContext, point, px};
use sheprd_backend::{Process, RunState, TerminalInfo};
use sheprd_client::protocol::AgentStatus;

fn claude(name: &str, agent: AgentStatus) -> TerminalInfo {
    TerminalInfo {
        agent,
        cwd: None,
        foreground: Some(Process {
            argv: vec!["claude".into()],
            name: "claude".into(),
            pid: 42,
        }),
        id: id(name),
        labels: Default::default(),
        name: None,
        reporter: None,
        state: RunState::Running,
        title: None,
    }
}

#[gpui::test]
fn muting_a_tab_from_its_menu_silences_it_and_its_panes(cx: &mut TestAppContext) {
    let (view, cx, tab) = window_with_split(cx, &["t1", "t2"]);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            assert!(!view.core_tab_silenced(tab, cx));
            view.open_core_menu(Target::Tab(tab), point(px(10.), px(10.)), window, cx);
            view.activate_core_menu(Action::Mute(MuteRow::Mute), window, cx);
            assert!(view.menu.page.is_none(), "the menu closes");
            assert!(view.core_tab_silenced(tab, cx));
            assert!(view.core_terminal_silenced(&Host::Local, &id("t2"), cx));
            assert!(
                !view.core_terminal_silenced(&Host::Local, &id("t9"), cx),
                "a terminal no muted tab holds still rings"
            );
            let other = Host::Remote(crate::browser::Scope::endpoint("ssh:box"));
            assert!(
                !view.core_terminal_silenced(&other, &id("t1"), cx),
                "another host's t1 is another terminal"
            );
            assert_eq!(view.core_mute_row(tab, cx), MuteRow::Unmute);
            let stored = Store::update(cx, |store| store.get(tab).cloned()).unwrap();
            assert!(view.core_face(&stored, cx).unwrap().muted);
            view.apply_core_mute_row(tab, MuteRow::Unmute, cx);
            assert!(!view.core_tab_silenced(tab, cx));
            assert_eq!(view.core_mute_row(tab, cx), MuteRow::Mute);
        })
    });
}

#[gpui::test]
fn a_muted_device_silences_its_tabs_and_the_menu_lifts_it(cx: &mut TestAppContext) {
    let (view, cx, tab) = window_with_split(cx, &["t1"]);
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            Mutes::update(cx, |mutes| mutes.set_host(Host::Local.endpoint(), true));
            assert!(view.core_tab_silenced(tab, cx));
            assert!(view.core_terminal_silenced(&Host::Local, &id("t1"), cx));
            assert_eq!(view.core_mute_row(tab, cx), MuteRow::UnmuteDevice);
            view.apply_core_mute_row(tab, MuteRow::UnmuteDevice, cx);
            assert!(!view.core_tab_silenced(tab, cx));
        })
    });
}

#[gpui::test]
fn a_muted_kept_workspace_silences_its_tabs_until_lifted(cx: &mut TestAppContext) {
    let (view, cx, _) = window_with_split(cx, &["t1"]);
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            let (scope, _) = view.browser_key().unwrap();
            let (workspace, tab) = Store::update(cx, |store| {
                let workspace = store.keep_workspace(&scope, "kept", "", false).unwrap();
                let location = Location::core_terminal(Panes::single(id("t5")), false);
                let tab = store
                    .open(scope.clone(), &workspace, Some(location), None)
                    .unwrap();
                (workspace, tab)
            });
            assert!(crate::core_terminals::mutes::set_kept(
                cx,
                scope.as_str(),
                Scope::Workspace,
                &workspace,
                true,
            ));
            assert!(view.core_tab_silenced(tab, cx));
            assert!(view.core_terminal_silenced(&Host::Local, &id("t5"), cx));
            assert_eq!(view.core_mute_row(tab, cx), MuteRow::UnmuteWorkspace);
            view.apply_core_mute_row(tab, MuteRow::UnmuteWorkspace, cx);
            assert!(!view.core_tab_silenced(tab, cx));
            assert_eq!(view.core_mute_row(tab, cx), MuteRow::Mute);
        })
    });
}

#[gpui::test]
fn closing_a_kept_workspace_forgets_its_mutes(cx: &mut TestAppContext) {
    let (view, cx, _) = window_with_split(cx, &["t1"]);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            let (scope, _) = view.browser_key().unwrap();
            let (workspace, tab) = Store::update(cx, |store| {
                let workspace = store.keep_workspace(&scope, "kept", "", false).unwrap();
                let tab = store.open(scope.clone(), &workspace, None, None).unwrap();
                (workspace, tab)
            });
            let set = |cx: &mut gpui::App, kind, id: &str| {
                crate::core_terminals::mutes::set_kept(cx, scope.as_str(), kind, id, true)
            };
            assert!(set(cx, Scope::Workspace, &workspace));
            assert!(set(cx, Scope::Tab, &tab.to_string()));
            view.close_kept_workspace(&workspace, window, cx);
            assert!(Mutes::of(cx).unwrap().is_empty());
        })
    });
}

#[gpui::test]
fn the_strip_shows_a_core_tabs_agent_and_zoom(cx: &mut TestAppContext) {
    let (view, cx, tab) = window_with_split(cx, &["t1", "t2"]);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.core_terminals
                .infos
                .list_for_test(vec![claude("t2", AgentStatus::Working)]);
            let stored = Store::update(cx, |store| store.get(tab).cloned()).unwrap();
            let face = view.core_face(&stored, cx).unwrap();
            assert!(matches!(
                face.mark,
                Some(Mark::Herdr {
                    status: AgentStatus::Working,
                    agent: Some(AgentIcon::Claude),
                })
            ));
            assert!(!face.zoomed && !face.muted);
            view.open_core_menu(
                Target::Pane {
                    tab,
                    terminal: id("t2"),
                },
                point(px(10.), px(10.)),
                window,
                cx,
            );
            view.activate_core_menu(Action::Zoom, window, cx);
            assert!(view.core_face(&stored, cx).unwrap().zoomed);
        })
    });
}

#[gpui::test]
fn a_tab_without_an_agent_keeps_its_terminal_icon(cx: &mut TestAppContext) {
    let (view, cx, tab) = window_with_split(cx, &["t1"]);
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            let stored = Store::update(cx, |store| store.get(tab).cloned()).unwrap();
            let face = view.core_face(&stored, cx).unwrap();
            assert!(face.mark.is_none());
            assert_eq!(face.lead(), crate::tab_layout::Lead::Icon);
        })
    });
}

#[gpui::test]
fn the_stand_in_shell_follows_its_workspaces_mute(cx: &mut TestAppContext) {
    let (view, cx, _) = window_with_split(cx, &["t1"]);
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            assert!(!view.core_fallback_silenced(cx));
            let scope = crate::browser::scope(&view.endpoints[view.selected_endpoint]);
            Store::update(cx, |store| {
                store.keep_default(&scope, crate::core_terminals::WORKSPACE, "Herdr 2")
            });
            assert!(crate::core_terminals::mutes::set_kept(
                cx,
                scope.as_str(),
                Scope::Workspace,
                crate::core_terminals::WORKSPACE,
                true,
            ));
            assert!(view.core_fallback_silenced(cx));
            Mutes::update(cx, |mutes| {
                mutes.set_kept(
                    scope.as_str(),
                    Scope::Workspace,
                    crate::core_terminals::WORKSPACE,
                    false,
                    std::time::SystemTime::now(),
                    |_, _, _| true,
                )
            });
            assert!(!view.core_fallback_silenced(cx));
            Mutes::update(cx, |mutes| mutes.set_host(Host::Local.endpoint(), true));
            assert!(view.core_fallback_silenced(cx));
        })
    });
}
