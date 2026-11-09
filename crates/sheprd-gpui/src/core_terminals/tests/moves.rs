//! Moving a herdr-core pane to a new tab or into another tab keeps its
//! terminal running and its name.

use super::commands::{flash, id, run, window_with_split};
use crate::{
    browser::{Location, Store, TabId},
    controls::Command,
    core_terminals::{menu::Target, moves::Destination, panes::Panes},
    i18n::fl,
};
use gpui::{Entity, TestAppContext, VisualTestContext};

fn open_other(view: &Entity<crate::SheprdWindow>, cx: &mut VisualTestContext, name: &str) -> TabId {
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            let (scope, workspace) = view.browser_key().unwrap();
            let location = Location::core_terminal(Panes::single(id(name)), false);
            Store::update(cx, |store| {
                store.open(scope, &workspace, Some(location), None)
            })
            .unwrap()
        })
    })
}

fn panes_of(cx: &mut VisualTestContext, tab: TabId) -> Option<Vec<String>> {
    cx.update(|_, cx| match &cx.global::<Store>().get(tab)?.location {
        Some(Location::CoreTerminal { panes, .. }) => {
            Some(panes.terminals().iter().map(ToString::to_string).collect())
        }
        _ => None,
    })
}

fn ending(cx: &mut VisualTestContext) -> bool {
    cx.update(|_, cx| cx.global::<Store>().has_ending())
}

#[gpui::test]
fn a_pane_moves_to_a_new_tab_with_its_name(cx: &mut TestAppContext) {
    let (view, cx, tab) = window_with_split(cx, &["t1", "t2"]);
    let pane = Target::Pane {
        tab,
        terminal: id("t2"),
    };
    let moves = cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.set_core_name(&pane, Some("tests".into()), cx);
            let moves = view.core_moves(tab, &id("t2"), cx);
            view.move_core_pane(tab, id("t2"), Destination::NewTab, window, cx);
            moves
        })
    });
    assert_eq!(moves[0].destination, Destination::NewTab);
    assert_eq!(panes_of(cx, tab).unwrap(), ["t1"]);
    let moved = cx.update(|_, cx| {
        cx.global::<Store>()
            .tabs()
            .find(|other| other.id != tab)
            .map(|other| other.id)
            .unwrap()
    });
    assert_eq!(panes_of(cx, moved).unwrap(), ["t2"]);
    let label = cx.update(|_, cx| view.read(cx).core_pane_label(moved, &id("t2"), cx).0);
    assert_eq!(label, "tests");
    assert!(!ending(cx), "the terminal runs on");
}

#[gpui::test]
fn a_tabs_only_pane_joins_another_tab_and_its_tab_goes(cx: &mut TestAppContext) {
    let (view, cx, tab) = window_with_split(cx, &["t1"]);
    let other = open_other(&view, cx, "t9");
    let moves = cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            let moves = view.core_moves(tab, &id("t1"), cx);
            view.move_core_pane(tab, id("t1"), Destination::Tab(other), window, cx);
            moves
        })
    });
    // Alone in its tab, it has no new tab to go to.
    assert_eq!(moves.len(), 1);
    assert_eq!(moves[0].destination, Destination::Tab(other));
    assert_eq!(panes_of(cx, other).unwrap(), ["t9", "t1"]);
    assert!(panes_of(cx, tab).is_none(), "the emptied tab is gone");
    let kept = cx.update(|_, cx| Store::update(cx, Store::take_ending));
    assert!(kept.is_empty(), "the moved terminal is not ended");
}

#[gpui::test]
fn a_lone_pane_with_nowhere_to_go_says_so(cx: &mut TestAppContext) {
    let (view, cx, _) = window_with_split(cx, &["t1"]);
    run(&view, cx, Command::MovePane);
    assert_eq!(
        flash(&view, cx),
        Some(fl!("core-terminals-nowhere-to-move"))
    );
}

#[gpui::test]
fn move_pane_opens_the_picker(cx: &mut TestAppContext) {
    let (view, cx, _) = window_with_split(cx, &["t1", "t2"]);
    run(&view, cx, Command::MovePane);
    let page = cx.update(|_, cx| view.read(cx).menu.page);
    assert_eq!(page, Some(crate::menu::Page::CoreMove));
}

#[gpui::test]
fn a_move_with_no_room_for_a_tab_keeps_the_panes_name(cx: &mut TestAppContext) {
    let (view, cx, tab) = window_with_split(cx, &["t1", "t2"]);
    let pane = Target::Pane {
        tab,
        terminal: id("t2"),
    };
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.set_core_name(&pane, Some("api".into()), cx);
            let (scope, workspace) = view.browser_key().unwrap();
            // Fill the store, so the new tab cannot open.
            while Store::update(cx, |store| {
                store.open(scope.clone(), &workspace, None, None)
            })
            .is_some()
            {}
            view.move_core_pane(tab, id("t2"), Destination::NewTab, window, cx);
        })
    });
    let name = cx.update(|_, cx| {
        view.read(cx).core_terminals.tabs[&tab]
            .names
            .get(&id("t2"))
            .cloned()
    });
    assert_eq!(name.as_deref(), Some("api"));
    assert_eq!(panes_of(cx, tab), Some(vec!["t1".into(), "t2".into()]));
}

#[gpui::test]
fn the_move_picker_opens_where_asked_without_dimming(cx: &mut TestAppContext) {
    let (view, cx, _) = window_with_split(cx, &["t1", "t2"]);
    run(&view, cx, Command::MovePane);
    let (dims, anchored) = cx.update(|_, cx| {
        let view = view.read(cx);
        (
            view.menu_dims(),
            view.menu.page.is_some_and(|page| page.pointer_anchored()),
        )
    });
    assert!(!dims && anchored);
}

#[gpui::test]
fn a_move_reads_what_another_window_saved_first(cx: &mut TestAppContext) {
    let (view, cx, tab) = window_with_split(cx, &["a", "b"]);
    // Another window moved b out, leaving a alone in the tab.
    cx.update(|_, cx| {
        let location = Location::core_terminal(Panes::single(id("a")), false);
        Store::update(cx, |store| store.visited(tab, Some(location), None));
    });
    let tabs = |cx: &mut VisualTestContext| cx.update(|_, cx| cx.global::<Store>().tabs().count());
    let before = tabs(cx);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.move_core_pane(tab, id("a"), Destination::NewTab, window, cx)
        })
    });
    // Alone, a has no new tab to go to: it stays, and only once.
    assert_eq!(tabs(cx), before);
    assert_eq!(panes_of(cx, tab), Some(vec!["a".into()]));
}
