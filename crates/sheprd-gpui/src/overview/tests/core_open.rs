//! Opening a Herdr 2 agent's row brings its workspace to the front first.

use crate::{
    SheprdWindow,
    browser::{Location, Store},
    core_terminals::{WORKSPACE, panes::Panes},
    sidebar::layout_tests::{SidebarFixture, fixture_window},
};
use gpui::{Entity, TestAppContext, VisualTestContext, prelude::*};
use sheprd_backend::TerminalId;
use std::path::Path;

fn window(cx: &mut TestAppContext) -> (Entity<SheprdWindow>, &mut VisualTestContext) {
    let (fixture, cx) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| fixture_window(window, cx));
        SidebarFixture(view)
    });
    let view = cx.update(|_, cx| fixture.read(cx).0.clone());
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            // With no Herdr 1, the window's workspaces are the app's own.
            view.live.snapshot = None;
            view.live.missing_installation = true;
            view.refresh_core_session(cx);
        })
    });
    (view, cx)
}

#[gpui::test]
fn an_agent_in_another_workspace_brings_it_to_the_front(cx: &mut TestAppContext) {
    let (view, cx) = window(cx);
    let terminal = TerminalId::new("t1").unwrap();
    let location = Location::core_terminal(Panes::single(terminal.clone()), false);
    let workspace = cx
        .update(|window, cx| {
            view.update(cx, |view, cx| {
                view.open_kept_workspace(None, Path::new("/src/api"), location, window, cx)
            })
        })
        .unwrap();
    // The user went back to the first workspace; the agent's is behind.
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            assert!(view.focus_kept_workspace(WORKSPACE, cx));
        })
    });
    let tab = view.read_with(cx, |view, cx| {
        let scope = crate::browser::scope(&view.endpoints[0]);
        cx.global::<Store>()
            .in_workspace(&scope, &workspace)
            .next()
            .unwrap()
            .id
    });
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.open_core_overview_row(tab, &terminal, window, cx);
        })
    });
    view.read_with(cx, |view, _| {
        assert_eq!(view.kept_in_front(), Some(workspace.as_str()));
    });
}
