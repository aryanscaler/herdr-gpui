//! A page requested from a herdr-core terminal goes first to the window
//! holding its tab, even when another window in front shows its workspace.

use super::*;
use crate::{
    browser::{Location, Store},
    core_terminals::panes::Panes,
    sidebar::layout_tests::fixture_window,
};
use gpui::{TestAppContext, WindowHandle};
use sheprd_backend::TerminalId;

/// Gives `handle`'s window a herdr-core tab in w3 holding `terminal`.
fn hold(handle: WindowHandle<crate::SheprdWindow>, terminal: &str, cx: &mut TestAppContext) {
    handle
        .update(cx, |view, _, cx| {
            let scope = crate::browser::scope(&view.endpoints[0]);
            let id = TerminalId::new(terminal).unwrap();
            let location = Location::core_terminal(Panes::single(id.clone()), false);
            let tab = Store::update(cx, |store| {
                store.open(scope.clone(), "w3", Some(location), None)
            })
            .unwrap();
            view.core_terminals
                .show_for_test(tab, Panes::single(id), false, Vec::new());
            view.core_terminals.place_for_test(tab, scope, "w3");
        })
        .unwrap();
}

#[gpui::test]
fn the_window_holding_the_terminal_is_asked_first(cx: &mut TestAppContext) {
    let front = cx.add_window(fixture_window);
    let back = cx.add_window(fixture_window);
    hold(front, "t1", cx);
    hold(back, "t2", cx);
    cx.update(|cx| {
        use crate::window::main_window::MainWindow;
        let ids = |windows: &[MainWindow]| {
            windows
                .iter()
                .map(|handle| handle.window_id())
                .collect::<Vec<_>>()
        };
        let mut windows = vec![MainWindow::Root(front), MainWindow::Root(back)];
        holders_first(&mut windows, Some("t2"), cx);
        assert_eq!(
            ids(&windows),
            ids(&[MainWindow::Root(back), MainWindow::Root(front)])
        );
        // No terminal, or one no window holds, keeps the frontmost order.
        let mut windows = vec![MainWindow::Root(front), MainWindow::Root(back)];
        holders_first(&mut windows, None, cx);
        assert_eq!(
            ids(&windows),
            ids(&[MainWindow::Root(front), MainWindow::Root(back)])
        );
        holders_first(&mut windows, Some("t9"), cx);
        assert_eq!(
            ids(&windows),
            ids(&[MainWindow::Root(front), MainWindow::Root(back)])
        );
    });
}

#[gpui::test]
fn a_core_terminals_notes_are_waited_for_under_its_tabs_daemon(cx: &mut TestAppContext) {
    let window = cx.add_window(fixture_window);
    hold(window, "t1", cx);
    let held = window
        .read_with(cx, |view, _| crate::browser::scope(&view.endpoints[0]))
        .unwrap();
    cx.update(|cx| {
        // Whatever socket the terminal inherited, its pages' notes are kept
        // under the daemon whose tab holds it.
        assert_eq!(core_scope("core:t1", cx), Some(held));
        // A terminal no window holds, or a Herdr pane, keeps the caller's.
        assert_eq!(core_scope("core:t9", cx), None);
        assert_eq!(core_scope("w0:p1", cx), None);
    });
}
