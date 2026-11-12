//! The rescroll dispatch's herdr-core arm: a target naming the shown tab's
//! pane scrolls that view's own history to the bottom. No sidebar agent row
//! names a core terminal yet; the seam stays total for the row sites that
//! adopt the resolver.

use super::{
    accessibility::look,
    history::requests,
    worker::{Scripted, size},
};
use crate::{
    browser::TabId,
    core_terminals::{CoreTerminalView, panes::Panes, worker},
    navigation::RescrollTarget,
    sidebar::layout_tests::fixture_window,
    terminal_painter::TerminalPainter,
};
use gpui::{AppContext as _, TestAppContext};
use sheprd_backend::{History, Role as AttachRole, TerminalId};
use std::{cell::RefCell, rc::Rc};

#[gpui::test]
fn a_core_terminal_target_scrolls_its_view_to_the_bottom(cx: &mut TestAppContext) {
    let backend = Scripted::default();
    let terminal = TerminalId::new("t1").unwrap();
    let link = worker::spawn(
        backend.clone(),
        terminal.clone(),
        size(80, 24),
        AttachRole::Active,
    )
    .unwrap();
    let painter = Rc::new(RefCell::new(TerminalPainter::default()));
    let tab = TabId::test(1);
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut view = fixture_window(window, cx);
        let core =
            cx.new(|cx| CoreTerminalView::new(terminal.clone(), link, look(), painter, false, cx));
        view.core_terminals
            .show_for_test(tab, Panes::single(terminal.clone()), false, Vec::new());
        view.core_terminals
            .views
            .insert((tab, terminal.clone()), core);
        view.core_terminals.shown = Some((tab, terminal.clone()));
        view
    });
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            assert!(view.core_pane_shown(tab, &terminal));
            view.scroll_to_bottom(
                RescrollTarget::CoreTerminal {
                    tab,
                    terminal: terminal.clone(),
                },
                cx,
            );
        })
    });
    assert_eq!(requests(&backend, 1), vec![History::ScrollTo { offset: 0 }]);
}
