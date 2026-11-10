//! Pane commands while a kept workspace is in front and the keyboard is
//! elsewhere, as after a sidebar step: they act on the pane the workspace
//! shows, and never reach Herdr 1's focused pane behind it.

use super::kept_workspaces::{core_tab, window};
use crate::{browser::Store, controls::Command, core_terminals::acts_on_panes};
use std::path::Path;

#[gpui::test]
fn a_pane_command_goes_to_the_kept_workspaces_shown_pane(cx: &mut gpui::TestAppContext) {
    let (view, cx) = window(cx, true);
    let tab = cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            let mut location = core_tab("t1");
            if let crate::browser::Location::CoreTerminal { panes, .. } = &mut location {
                panes
                    .split(
                        &sheprd_backend::TerminalId::new("t1").unwrap(),
                        crate::core_terminals::panes::Axis::Row,
                        sheprd_backend::TerminalId::new("t2").unwrap(),
                    )
                    .unwrap();
            }
            let workspace = view
                .open_kept_workspace(None, Path::new("/src/api"), location, window, cx)
                .unwrap();
            view.sync_core_views(cx);
            let scope = crate::browser::scope(&view.endpoints[0]);
            let tab = cx
                .global::<Store>()
                .in_workspace(&scope, &workspace)
                .next()
                .unwrap()
                .id;
            // The keyboard is on the window, as after a sidebar step.
            window.focus(&view.focus, cx);
            tab
        })
    });
    let shown = cx.update(|window, cx| view.read(cx).core_command_pane(window, cx));
    assert_eq!(shown.map(|(shown, _)| shown), Some(tab));
    cx.update(|window, cx| view.update(cx, |view, cx| view.command(Command::Zoom, window, cx)));
    view.read_with(cx, |view, _| {
        assert!(view.core_terminals.tabs[&tab].zoom.is_open());
    });
}

#[gpui::test]
fn without_a_herdr_2_pane_a_pane_command_stops_at_the_kept_workspace(
    cx: &mut gpui::TestAppContext,
) {
    let (view, cx) = window(cx, true);
    // Herdr 1 in front: a clear its daemon cannot run says so.
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.live.supports_pane_clear = false;
            view.command(Command::ClearPane, window, cx);
            assert!(view.local_error.take().is_some());
        })
    });
    // A kept workspace with nothing shown in front: Herdr 1 never hears it.
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            let scope = crate::browser::scope(&view.endpoints[0]);
            let id = Store::update(cx, |store| store.keep_workspace(&scope, "empty", "", false))
                .unwrap();
            view.refresh_core_session(cx);
            assert!(view.focus_kept_workspace(&id, cx));
        })
    });
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            assert!(view.core_command_pane(window, cx).is_none());
            view.command(Command::ClearPane, window, cx);
            assert!(view.local_error.is_none());
        })
    });
}

#[test]
fn closing_and_window_commands_are_not_pane_commands() {
    assert!(acts_on_panes(Command::SplitRight));
    assert!(acts_on_panes(Command::Zoom));
    assert!(acts_on_panes(Command::ClearPane));
    assert!(acts_on_panes(Command::FocusLeft));
    assert!(!acts_on_panes(Command::ClosePane));
    assert!(!acts_on_panes(Command::CloseTab));
    assert!(!acts_on_panes(Command::NewBrowserTab));
}
