//! Two windows showing one herdr-core tab: each reads what the other saved
//! before saving its own change, so neither undoes the other's.

use crate::{
    browser::Location,
    core_terminals::{
        Host, TabPanes,
        panes::{Axis, Panes},
    },
};
use sheprd_backend::TerminalId;

fn id(name: &str) -> TerminalId {
    TerminalId::new(name).unwrap()
}

fn panes(names: &[&str]) -> Panes {
    let mut panes = Panes::single(id(names[0]));
    for pair in names.windows(2) {
        panes.split(&id(pair[0]), Axis::Row, id(pair[1])).unwrap();
    }
    panes
}

/// The tab as a window first reads it, holding `names`.
fn shown(names: &[&str]) -> TabPanes {
    let location = Location::core_terminal(panes(names), false);
    let mut shown = TabPanes::new(Host::Local, panes(names), false);
    shown.saved = Some(location);
    shown
}

#[test]
fn a_focus_change_keeps_a_split_another_window_saved() {
    let mut here = shown(&["t1", "t2"]);
    // This window moves the keyboard; another saves a third pane first.
    here.active = id("t2");
    here.unsaved = true;
    let there = Location::core_terminal(panes(&["t1", "t2", "t3"]), false);
    assert!(here.refresh(&there).is_empty());
    assert_eq!(here.panes, panes(&["t1", "t2", "t3"]));
    assert_eq!(here.active, id("t2"), "this window's focus still stands");
    assert!(here.unsaved);
    assert_eq!(here.saved.as_ref(), Some(&there));
    let Location::CoreTerminal { panes: saved, .. } = here.location() else {
        panic!("a terminal tab");
    };
    assert!(saved.contains(&id("t3")), "the next save keeps the split");
}

#[test]
fn a_pane_gone_elsewhere_takes_the_focus_and_zoom_saved_there() {
    let mut here = shown(&["t1", "t2"]);
    here.active = id("t2");
    here.set_zoom(Some(id("t2")));
    here.unsaved = true;
    let mut there = Location::core_terminal(panes(&["t1", "t3"]), false);
    if let Location::CoreTerminal { active, .. } = &mut there {
        *active = Some(id("t3"));
    }
    assert!(here.refresh(&there).is_empty());
    assert_eq!(here.active, id("t3"));
    assert!(!here.zoom.is_open(), "the zoomed pane went");
}

#[test]
fn focus_stays_where_this_windows_keyboard_is() {
    let mut here = shown(&["t1", "t2"]);
    let mut there = Location::core_terminal(panes(&["t1", "t2"]), false);
    if let Location::CoreTerminal { active, .. } = &mut there {
        *active = Some(id("t2"));
    }
    assert!(here.refresh(&there).is_empty());
    assert_eq!(here.active, id("t1"));
    assert!(!here.unsaved);
}

#[test]
fn a_name_another_window_gave_is_read() {
    let mut here = shown(&["t1", "t2"]);
    let mut there = Location::core_terminal(panes(&["t1", "t2"]), false);
    if let Location::CoreTerminal { name, names, .. } = &mut there {
        *name = Some("build".into());
        names.insert(id("t2"), "tests".into());
    }
    assert!(here.refresh(&there).is_empty());
    assert_eq!(here.name.as_deref(), Some("build"));
    assert_eq!(here.names.get(&id("t2")).map(String::as_str), Some("tests"));
}

#[test]
fn a_split_placed_here_while_another_window_saved_stays() {
    let mut here = shown(&["t1", "t2"]);
    // A split this window just placed, not saved yet.
    here.panes.split(&id("t2"), Axis::Row, id("t3")).unwrap();
    here.active = id("t3");
    here.unsaved = true;
    // Another window saved a focus change on the old layout meanwhile.
    let mut there = Location::core_terminal(panes(&["t1", "t2"]), false);
    if let Location::CoreTerminal { active, .. } = &mut there {
        *active = Some(id("t1"));
    }
    assert!(here.refresh(&there).is_empty());
    assert_eq!(here.panes, panes(&["t1", "t2", "t3"]));
    assert_eq!(here.active, id("t3"));
    assert!(here.unsaved, "the split still saves");
}

#[test]
fn layouts_both_windows_changed_merge() {
    let mut here = shown(&["t1", "t2"]);
    here.panes.split(&id("t1"), Axis::Row, id("t3")).unwrap();
    here.unsaved = true;
    // The other window closed t2 and opened t4.
    let there = Location::core_terminal(panes(&["t1", "t4"]), false);
    assert!(here.refresh(&there).is_empty());
    let mut kept: Vec<String> = here
        .panes
        .terminals()
        .iter()
        .map(ToString::to_string)
        .collect();
    kept.sort();
    assert_eq!(kept, ["t1", "t3", "t4"]);
}

#[test]
fn a_zoom_saved_on_another_pane_never_hides_the_one_in_use() {
    let mut here = shown(&["t1", "t2"]);
    let mut there = Location::core_terminal(panes(&["t1", "t2"]), false);
    if let Location::CoreTerminal { zoomed, .. } = &mut there {
        *zoomed = Some(id("t2"));
    }
    assert!(here.refresh(&there).is_empty());
    assert_eq!(here.active, id("t1"));
    assert!(!here.zoom.is_open(), "t1 in use is not hidden behind t2");
    // On the pane in use, the other window's zoom is taken.
    let mut there = Location::core_terminal(panes(&["t1", "t2"]), false);
    if let Location::CoreTerminal { zoomed, .. } = &mut there {
        *zoomed = Some(id("t1"));
    }
    assert!(here.refresh(&there).is_empty());
    assert!(here.zoom.is_open());
    assert_eq!(here.zoomed, Some(id("t1")));
}

#[test]
fn a_name_given_here_stays_when_another_window_saves() {
    let mut here = shown(&["t1", "t2"]);
    here.name = Some("build".into());
    here.unsaved = true;
    let mut there = Location::core_terminal(panes(&["t1", "t2"]), false);
    if let Location::CoreTerminal { active, .. } = &mut there {
        *active = Some(id("t2"));
    }
    assert!(here.refresh(&there).is_empty());
    assert_eq!(here.name.as_deref(), Some("build"));
}

#[test]
fn panes_both_windows_closed_leave_the_keyboard_on_one_still_there() {
    let mut here = shown(&["a", "b", "c"]);
    // This window closes b and moves to a.
    here.panes.remove(&id("b"));
    here.active = id("a");
    here.unsaved = true;
    // Another closes a and saves with b in use.
    let mut there = Location::core_terminal(panes(&["b", "c"]), false);
    if let Location::CoreTerminal { active, .. } = &mut there {
        *active = Some(id("b"));
    }
    assert!(here.refresh(&there).is_empty());
    assert_eq!(here.panes, Panes::single(id("c")));
    assert_eq!(here.active, id("c"));
}

#[test]
fn a_pane_a_full_tab_cannot_take_comes_back_for_a_tab_of_its_own() {
    let names: Vec<String> = (0..15).map(|index| format!("p{index}")).collect();
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    let mut here = shown(&names);
    // Both windows split the fifteen-pane tab at once.
    here.panes.split(&id("p14"), Axis::Row, id("mine")).unwrap();
    here.unsaved = true;
    let mut theirs = panes(&names);
    theirs.split(&id("p14"), Axis::Row, id("theirs")).unwrap();
    let left = here.refresh(&Location::core_terminal(theirs, false));
    assert_eq!(left.overflow, [id("theirs")]);
    assert!(here.panes.contains(&id("mine")));
    assert_eq!(here.panes.count(), 16);
}

#[test]
fn closing_the_pane_in_use_elsewhere_leaves_the_keyboard_on_one_still_there() {
    let mut here = shown(&["A", "B", "C"]);
    // This window closes C and keeps A in use.
    here.panes.remove(&id("C"));
    here.unsaved = true;
    // Another closes A and saves with C in use.
    let mut there = Location::core_terminal(panes(&["B", "C"]), false);
    if let Location::CoreTerminal { active, .. } = &mut there {
        *active = Some(id("C"));
    }
    assert!(here.refresh(&there).is_empty());
    assert_eq!(here.panes, Panes::single(id("B")));
    assert_eq!(here.active, id("B"));
}

#[test]
fn names_given_to_different_panes_in_two_windows_both_stand() {
    let mut here = shown(&["t1", "t2"]);
    here.names.insert(id("t1"), "api".into());
    here.unsaved = true;
    let mut there = Location::core_terminal(panes(&["t1", "t2"]), false);
    if let Location::CoreTerminal { names, .. } = &mut there {
        names.insert(id("t2"), "tests".into());
    }
    assert!(here.refresh(&there).is_empty());
    assert_eq!(here.names.get(&id("t1")).map(String::as_str), Some("api"));
    assert_eq!(here.names.get(&id("t2")).map(String::as_str), Some("tests"));
}

#[test]
fn each_window_closing_the_others_last_pane_empties_the_tab() {
    let mut here = shown(&["a", "b"]);
    // This window closes a; the other closes b.
    here.panes.remove(&id("a"));
    here.unsaved = true;
    let left = here.refresh(&Location::core_terminal(panes(&["a"]), false));
    assert!(left.emptied, "b must not come back");
}

/// Another window closes `t1`, which holds this window's keyboard.
fn closed_elsewhere(
    view: &gpui::Entity<crate::SheprdWindow>,
    cx: &mut gpui::VisualTestContext,
    tab: crate::browser::TabId,
) {
    cx.update(|window, cx| {
        let location = Location::core_terminal(Panes::single(id("t2")), false);
        crate::browser::Store::update(cx, |store| store.visited(tab, Some(location), None));
        view.update(cx, |view, cx| view.sync_core_tabs(window, cx));
    });
}

#[gpui::test]
fn a_pane_closed_elsewhere_hands_the_keyboard_to_the_one_left(cx: &mut gpui::TestAppContext) {
    let (view, cx, tab) = super::commands::window_with_split(cx, &["t1", "t2"]);
    closed_elsewhere(&view, cx, tab);
    let focused = cx.update(|window, cx| {
        let view = view.read(cx);
        view.core_terminals.views[&(tab, id("t2"))]
            .read(cx)
            .focus_handle()
            .is_focused(window)
    });
    assert!(focused);
}

#[gpui::test]
fn a_control_focused_elsewhere_keeps_the_keyboard(cx: &mut gpui::TestAppContext) {
    let (view, cx, tab) = super::commands::window_with_split(cx, &["t1", "t2"]);
    let control = cx.update(|window, cx| {
        let control = cx.focus_handle();
        window.focus(&control, cx);
        control
    });
    closed_elsewhere(&view, cx, tab);
    let kept = cx.update(|window, _| control.is_focused(window));
    assert!(kept);
}
