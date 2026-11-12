#![allow(clippy::unwrap_used)]
use super::*;
use crate::{config::UndoCloseTab, window::MockPeer};
use gpui::{TestAppContext, VisualTestContext};
use sheprd_client::protocol::ClientShellSnapshot;
use std::sync::Arc;

const FIRST: &str = "w1:t1";
const SECOND: &str = "w1:t2";

/// A window over `peer` whose workspace has `tabs` tabs, with tab close
/// asking nothing and undo set to `undo`.
fn window(
    cx: &mut TestAppContext,
    tabs: usize,
    undo: UndoCloseTab,
) -> (gpui::Entity<SheprdWindow>, &mut VisualTestContext, MockPeer) {
    let peer = MockPeer::new();
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut view = crate::sidebar::layout_tests::fixture_window(window, cx);
        peer.prepare(&mut view, cx);
        let mut snapshot: ClientShellSnapshot = (**view.live.snapshot.as_ref().unwrap()).clone();
        for number in 2..=tabs {
            let mut tab = snapshot.tabs[0].clone();
            tab.tab_id = format!("w1:t{number}");
            tab.number = number;
            tab.focused = false;
            let mut pane = snapshot.panes[0].clone();
            pane.pane_id = format!("w1:p{number}");
            pane.tab_id = tab.tab_id.clone();
            pane.focused = false;
            snapshot.tabs.push(tab);
            snapshot.panes.push(pane);
        }
        view.live.snapshot = Some(Arc::new(snapshot));
        view.config.confirm_close_tab = false;
        view.config.undo_close_tab = undo;
        view
    });
    (view, cx, peer)
}

fn on(window: Duration) -> UndoCloseTab {
    UndoCloseTab {
        enabled: true,
        window,
    }
}

/// Closes `tab` as an accepted close does; whether it waits.
fn close(view: &gpui::Entity<SheprdWindow>, cx: &mut VisualTestContext, tab: &str) -> bool {
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            let snapshot = view.live.snapshot.clone().unwrap();
            let closed = ClosedHerdrTab::capture(&snapshot, tab).unwrap();
            view.defer_tab_close(&closed, cx)
        })
    })
}

fn hidden(view: &gpui::Entity<SheprdWindow>, cx: &mut VisualTestContext, tab: &str) -> bool {
    view.read_with(cx, |view, _| {
        let key = view.browser_key().unwrap();
        view.browser.hidden.hides(&key, &Pick::Herdr(tab.into()))
    })
}

#[gpui::test]
fn a_closed_tab_hides_and_reopen_brings_the_same_tab_back(cx: &mut TestAppContext) {
    let (view, cx, _peer) = window(cx, 2, on(Duration::from_secs(10)));
    assert!(close(&view, cx, FIRST));
    assert!(view.read_with(cx, |view, _| view.undo_waits(FIRST)));
    assert!(hidden(&view, cx, FIRST));
    cx.update(|window, cx| view.update(cx, |view, cx| view.reopen_closed_tab(window, cx)));
    assert!(!view.read_with(cx, |view, _| view.undo_waits(FIRST)));
    assert!(!hidden(&view, cx, FIRST));
}

#[gpui::test]
fn the_last_tab_and_undo_off_leave_the_close_to_the_caller(cx: &mut TestAppContext) {
    let (view, cx, _peer) = window(cx, 1, on(Duration::from_secs(10)));
    assert!(
        !close(&view, cx, FIRST),
        "Herdr closes its workspace with it"
    );
    assert!(!hidden(&view, cx, FIRST));
    let (view, cx, _peer) = window(cx, 2, UndoCloseTab::default());
    assert!(!close(&view, cx, SECOND));
    assert!(!hidden(&view, cx, SECOND));
    // A workspace whose other tabs all wait has no tab left to show either.
    let (view, cx, _peer) = window(cx, 2, on(Duration::from_secs(10)));
    assert!(close(&view, cx, FIRST));
    assert!(!close(&view, cx, SECOND));
}

#[gpui::test]
fn a_hidden_tab_focused_again_is_listed_and_kept(cx: &mut TestAppContext) {
    let (view, cx, _peer) = window(cx, 2, on(Duration::ZERO));
    assert!(close(&view, cx, SECOND));
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            // A poll sees the window moved off it, then the user goes back.
            view.browser.undo.pending[0].due = Instant::now() + Duration::from_secs(60);
            view.poll_undo_close(cx);
            let pick = Pick::Herdr(SECOND.into());
            let group = view
                .ensure_layout()
                .map(|layout| layout.owner(&pick))
                .unwrap();
            view.ensure_layout().unwrap().show(group, pick);
            view.browser.undo.pending[0].due = Instant::now();
            view.poll_undo_close(cx);
        })
    });
    assert!(!hidden(&view, cx, SECOND));
    assert!(!view.read_with(cx, |view, _| view.undo_waits(SECOND)));
}

#[gpui::test]
fn a_tab_alone_in_its_group_closes_at_once(cx: &mut TestAppContext) {
    let (view, cx, _peer) = window(cx, 2, on(Duration::from_secs(10)));
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            let first = view.ensure_layout().unwrap().active();
            view.split_group(first, window, cx);
            let second = view.ensure_layout().unwrap().active();
            assert_ne!(first, second, "the split opens a group");
            view.move_tab_to_group(Pick::Herdr(SECOND.into()), second, window, cx);
            assert!(view.group_shows(&Pick::Herdr(SECOND.into())));
        })
    });
    // Nothing would show in its place, so it must not wait on screen.
    assert!(!close(&view, cx, SECOND));
    assert!(!hidden(&view, cx, SECOND));
    assert!(!view.read_with(cx, |view, _| view.undo_waits(SECOND)));
}

#[gpui::test]
fn another_hosts_waiting_tab_does_not_count_as_closed_here(cx: &mut TestAppContext) {
    let (view, cx, _peer) = window(cx, 2, on(Duration::from_secs(10)));
    assert!(close(&view, cx, FIRST));
    cx.update(|_, cx| {
        view.update(cx, |view, _| {
            // The same tab id, waiting on another host instead.
            view.browser.undo.pending[0].scope = Scope::endpoint("elsewhere");
            view.browser.hidden = Default::default();
        })
    });
    assert!(!view.read_with(cx, |view, _| view.undo_waits(FIRST)));
    // This host's FIRST is still open, so SECOND is not its last tab.
    assert!(close(&view, cx, SECOND));
    // And the other host's close is kept beside this one.
    assert_eq!(
        view.read_with(cx, |view, _| view.browser.undo.pending.len()),
        2
    );
}

#[gpui::test]
fn a_daemon_restart_drops_the_close_and_shows_the_tab_again(cx: &mut TestAppContext) {
    let (view, cx, _peer) = window(cx, 2, on(Duration::from_secs(10)));
    assert!(close(&view, cx, FIRST));
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            // The daemon restarts and lists a tab under the same id.
            view.browser.undo.pending[0].boot = "an older boot".into();
            view.poll_undo_close(cx);
        })
    });
    assert!(!view.read_with(cx, |view, _| view.undo_waits(FIRST)));
    assert!(!hidden(&view, cx, FIRST), "the new boot's tab is shown");
}

#[gpui::test]
fn a_tab_focused_again_before_the_first_poll_is_listed_and_kept(cx: &mut TestAppContext) {
    let (view, cx, _peer) = window(cx, 2, on(Duration::ZERO));
    assert!(close(&view, cx, SECOND));
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            // Another client focuses it before this window ever polls.
            let mut snapshot: ClientShellSnapshot =
                (**view.live.snapshot.as_ref().unwrap()).clone();
            snapshot.focused_tab_id = Some(SECOND.into());
            for tab in &mut snapshot.tabs {
                tab.focused = tab.tab_id == SECOND;
            }
            for pane in &mut snapshot.panes {
                pane.focused = pane.tab_id == SECOND;
            }
            view.live.snapshot = Some(Arc::new(snapshot));
            view.terminal_focus_moved(SECOND, false, cx);
            assert!(view.group_shows(&Pick::Herdr(SECOND.into())));
            view.poll_undo_close(cx);
        })
    });
    assert!(!hidden(&view, cx, SECOND));
    assert!(!view.read_with(cx, |view, _| view.undo_waits(SECOND)));
}
