//! Go To lists the focused workspace's Herdr 2 tabs and panes, as the
//! sidebar does.

use super::*;
use crate::{
    browser::{Location, Store, TabId},
    core_terminals::panes::{Axis, Panes},
};
use sheprd_backend::{RunState, TerminalId, TerminalInfo};
use sheprd_client::protocol::AgentStatus;

fn id(name: &str) -> TerminalId {
    TerminalId::new(name).unwrap()
}

fn info(name: &str, agent: AgentStatus) -> TerminalInfo {
    TerminalInfo {
        agent,
        cwd: None,
        foreground: None,
        id: id(name),
        labels: Default::default(),
        name: Some(name.to_uppercase()),
        reporter: None,
        state: RunState::Running,
        title: None,
    }
}

fn open(view: &mut SheprdWindow, panes: &Panes, observe: bool, cx: &mut App) -> TabId {
    let (scope, workspace) = view.browser_key().unwrap();
    let location = Location::core_terminal(panes.clone(), observe);
    Store::update(cx, |store| {
        store.open(scope, &workspace, Some(location), None)
    })
    .unwrap()
}

#[gpui::test]
fn herdr_2_panes_are_go_to_rows_with_their_agents_status(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(fixture_window);
    let entries = cx.update(|_, cx| {
        Store::default().install(cx);
        view.update(cx, |view, cx| {
            let mut panes = Panes::single(id("t1"));
            panes.split(&id("t1"), Axis::Row, id("t2")).unwrap();
            open(view, &Panes::single(id("t1")), true, cx);
            let tab = open(view, &panes, false, cx);
            view.core_terminals.show_for_test(
                tab,
                panes,
                false,
                vec![
                    info("t1", AgentStatus::Unknown),
                    info("t2", AgentStatus::Working),
                ],
            );
            view.core_palette_entries(cx)
        })
    });
    let labels: Vec<_> = entries
        .iter()
        .map(|entry| entry.label.to_string())
        .collect();
    assert_eq!(
        labels,
        ["T1", "T2"],
        "one tab is no choice, and a mirror lists nothing of its own"
    );
    assert_eq!(entries[0].badge, fl!("palette-go-to-terminal"));
    assert_eq!(entries[0].status, None);
    assert_eq!(entries[1].status, Some(AgentStatus::Working));
    assert!(matches!(
        &entries[1].action,
        Action::Core { terminal: Some(terminal), .. } if *terminal == id("t2")
    ));
    assert!(Filter::Navigation.accepts(&entries[1].action));
}

#[gpui::test]
fn with_a_choice_of_tabs_each_heads_its_panes(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(fixture_window);
    let entries = cx.update(|_, cx| {
        Store::default().install(cx);
        view.update(cx, |view, cx| {
            open(view, &Panes::single(id("t1")), false, cx);
            open(view, &Panes::single(id("t2")), false, cx);
            view.core_palette_entries(cx)
        })
    });
    let shape: Vec<_> = entries
        .iter()
        .map(|entry| {
            let tab = matches!(entry.action, Action::Core { terminal: None, .. });
            (tab, entry.parent)
        })
        .collect();
    assert_eq!(
        shape,
        [
            (true, None),
            (false, Some(0)),
            (true, None),
            (false, Some(2))
        ]
    );
}

#[gpui::test]
fn herdr_2_panes_nest_under_their_tab_after_herdr_rows(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(fixture_window);
    let entries = cx.update(|window, cx| {
        Store::default().install(cx);
        view.update(cx, |view, cx| {
            open(view, &Panes::single(id("t1")), false, cx);
            open(view, &Panes::single(id("t2")), false, cx);
            view.open_palette(Filter::All, window, cx);
            view.menu.palette.as_ref().unwrap().base.to_vec()
        })
    });
    let first = entries
        .iter()
        .position(|entry| matches!(entry.action, Action::Core { .. }))
        .unwrap();
    assert!(first > 0, "the fixture's Herdr rows come first");
    for (index, entry) in entries.iter().enumerate() {
        if let Action::Core {
            tab,
            terminal: Some(_),
        } = entry.action
        {
            let parent = &entries[entry.parent.unwrap()];
            assert!(
                matches!(parent.action, Action::Core { tab: of, terminal: None } if of == tab),
                "row {index} nests under its own tab"
            );
        }
    }
}

#[gpui::test]
fn an_open_go_to_follows_a_herdr_2_agents_status(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(fixture_window);
    let status = |view: &Entity<SheprdWindow>, cx: &mut App| {
        let palette = view.read(cx).menu.palette.as_ref().unwrap();
        (palette.base.iter())
            .find(|entry| {
                matches!(
                    entry.action,
                    Action::Core {
                        terminal: Some(_),
                        ..
                    }
                )
            })
            .and_then(|entry| entry.status)
    };
    cx.update(|window, cx| {
        Store::default().install(cx);
        view.update(cx, |view, cx| {
            let panes = Panes::single(id("t1"));
            let tab = open(view, &panes, false, cx);
            let infos = vec![info("t1", AgentStatus::Working)];
            view.core_terminals.show_for_test(tab, panes, false, infos);
            view.open_palette(Filter::All, window, cx);
        });
        assert_eq!(status(&view, cx), Some(AgentStatus::Working));
        view.update(cx, |view, cx| {
            let tab = view.listed_core_panes(cx)[0].0;
            let panes = Panes::single(id("t1"));
            let infos = vec![info("t1", AgentStatus::Blocked)];
            view.core_terminals.show_for_test(tab, panes, false, infos);
            view.refresh_palette(window, cx);
        });
        assert_eq!(status(&view, cx), Some(AgentStatus::Blocked));
    });
}
