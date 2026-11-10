//! Moving a herdr-core pane or tab to another workspace of its host: a
//! Herdr 1 workspace, or a new one the app keeps.

use super::kept_workspaces::window;
use crate::{
    SheprdWindow,
    browser::{Location, Store, TabId},
    core_terminals::{
        moves::Destination,
        panes::{Axis, Panes},
    },
};
use gpui::{Entity, VisualTestContext};
use sheprd_backend::TerminalId;
use std::path::Path;

fn id(name: &str) -> TerminalId {
    TerminalId::new(name).unwrap()
}

/// A kept workspace in front whose one tab holds `names`, split.
fn kept_tab(view: &Entity<SheprdWindow>, cx: &mut VisualTestContext, names: &[&str]) -> TabId {
    let mut panes = Panes::single(id(names[0]));
    for pair in names.windows(2) {
        panes.split(&id(pair[0]), Axis::Row, id(pair[1])).unwrap();
    }
    let location = Location::core_terminal(panes, false);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            let workspace = view
                .open_kept_workspace(None, Path::new("/src/api"), location, window, cx)
                .unwrap();
            let scope = crate::browser::scope(&view.endpoints[0]);
            let tab = cx
                .global::<Store>()
                .in_workspace(&scope, &workspace)
                .next()
                .unwrap()
                .id;
            view.sync_core_views(cx);
            tab
        })
    })
}

fn workspace_of(view: &Entity<SheprdWindow>, cx: &mut VisualTestContext, tab: TabId) -> String {
    view.read_with(cx, |_, cx| {
        cx.global::<Store>().get(tab).unwrap().workspace_id.clone()
    })
}

#[gpui::test]
fn a_pane_moves_to_a_herdr_1_workspace_in_a_new_tab(cx: &mut gpui::TestAppContext) {
    let (view, cx) = window(cx, true);
    let tab = kept_tab(&view, cx, &["t1", "t2"]);
    let moves = cx.update(|_, cx| view.read(cx).core_moves(tab, &id("t2"), cx));
    let destinations: Vec<_> = moves.into_iter().map(|row| row.destination).collect();
    assert!(destinations.contains(&Destination::NewTab));
    assert!(destinations.contains(&Destination::NewWorkspace));
    assert!(destinations.contains(&Destination::Workspace("w1".into())));
    let front = view.read_with(cx, |view, _| view.browser_key().unwrap().1);
    assert!(
        !destinations.contains(&Destination::Workspace(front)),
        "the pane's own workspace takes a new tab, not a move there"
    );
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.move_core_pane(
                tab,
                id("t2"),
                Destination::Workspace("w1".into()),
                window,
                cx,
            )
        })
    });
    view.read_with(cx, |view, cx| {
        let store = cx.global::<Store>();
        let moved = store
            .tabs()
            .find(|other| other.workspace_id == "w1")
            .expect("a new tab in w1");
        assert!(matches!(
            &moved.location,
            Some(Location::CoreTerminal { panes, .. }) if panes.terminals() == [&id("t2")]
        ));
        let left = &view.core_terminals.tabs[&tab];
        assert!(!left.panes.contains(&id("t2")) && left.panes.contains(&id("t1")));
    });
}

#[gpui::test]
fn a_lone_pane_moves_to_a_new_workspace_and_its_old_tab_goes(cx: &mut gpui::TestAppContext) {
    let (view, cx) = window(cx, true);
    let tab = kept_tab(&view, cx, &["t1"]);
    let before = view.read_with(cx, |view, _| view.browser_key().unwrap().1);
    let moves = cx.update(|_, cx| view.read(cx).core_moves(tab, &id("t1"), cx));
    assert!(
        !moves
            .iter()
            .any(|row| row.destination == Destination::NewTab),
        "a lone pane is its tab already"
    );
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.move_core_pane(tab, id("t1"), Destination::NewWorkspace, window, cx)
        })
    });
    view.read_with(cx, |view, cx| {
        let (scope, front) = view.browser_key().unwrap();
        assert_ne!(front, before, "the new workspace comes to the front");
        assert!(view.is_kept_workspace(&front));
        let store = cx.global::<Store>();
        assert!(store.get(tab).is_none(), "the emptied tab closed");
        let moved = store.in_workspace(&scope, &front).next().unwrap();
        assert!(matches!(
            &moved.location,
            Some(Location::CoreTerminal { panes, .. }) if panes.contains(&id("t1"))
        ));
    });
    let ended = cx.update(|_, cx| Store::update(cx, Store::take_ending));
    assert!(
        ended.is_empty(),
        "the terminal keeps running in its new tab"
    );
}

#[gpui::test]
fn a_tab_moves_whole_to_another_workspace(cx: &mut gpui::TestAppContext) {
    let (view, cx) = window(cx, true);
    let tab = kept_tab(&view, cx, &["t1", "t2"]);
    let moves = cx.update(|_, cx| view.read(cx).core_tab_moves(tab));
    assert!(
        moves
            .iter()
            .any(|row| row.destination == Destination::Workspace("w0".into()))
    );
    let actions = cx.update(|_, cx| {
        view.read(cx)
            .core_menu_actions(&crate::core_terminals::menu::Target::Tab(tab), cx)
    });
    assert!(actions.contains(&crate::core_terminals::menu::Action::MoveToWorkspace));
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.move_core_tab(tab, Destination::Workspace("w0".into()), window, cx)
        })
    });
    assert_eq!(workspace_of(&view, cx, tab), "w0");
    view.read_with(cx, |view, _| {
        let shown = &view.core_terminals.tabs[&tab];
        assert_eq!(shown.panes.count(), 2, "its layout comes along");
    });
    let ended = cx.update(|_, cx| Store::update(cx, Store::take_ending));
    assert!(ended.is_empty());
    // A destination gone since the menu opened moves nothing.
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.move_core_tab(tab, Destination::Workspace("gone".into()), window, cx)
        })
    });
    assert_eq!(workspace_of(&view, cx, tab), "w0");
}

#[gpui::test]
fn a_mirror_tab_stays_where_it_is(cx: &mut gpui::TestAppContext) {
    let (view, cx) = window(cx, true);
    let tab = kept_tab(&view, cx, &["t1"]);
    cx.update(|_, cx| {
        view.update(cx, |view, _| {
            view.core_terminals.tabs.get_mut(&tab).unwrap().observe = true;
        })
    });
    let moves = cx.update(|_, cx| view.read(cx).core_tab_moves(tab));
    assert!(moves.is_empty());
}

#[gpui::test]
fn a_tab_another_window_moved_reads_its_new_workspace(cx: &mut gpui::TestAppContext) {
    let (view, cx) = window(cx, true);
    let tab = kept_tab(&view, cx, &["t1"]);
    let before = view.read_with(cx, |view, _| {
        view.core_terminals.tab_workspace(tab).cloned().unwrap()
    });
    // Another window moved it: only the shared store says so.
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            assert!(Store::update(cx, |store| store.move_to_workspace(tab, "w1")));
            view.sync_core_views(cx);
        })
    });
    let after = view.read_with(cx, |view, _| {
        view.core_terminals.tab_workspace(tab).cloned().unwrap()
    });
    assert_eq!(after, (before.0, "w1".to_owned()));
}
