//! Pane and tab commands while a herdr-core pane holds the keyboard: they act
//! on its tab, or say they cannot, and never reach the Herdr pane behind it.

use super::{
    accessibility::look,
    worker::{Scripted, size},
};
use crate::{
    SheprdWindow,
    browser::{Location, Store, TabId},
    controls::Command,
    core_terminals::{
        CoreTerminalView, Host, TabPanes,
        panes::{Axis, Panes},
        worker,
    },
    i18n::fl,
    terminal_painter::TerminalPainter,
};
use gpui::{AppContext as _, Entity, TestAppContext, VisualTestContext};
use sheprd_backend::{Role, TerminalId};
use std::{cell::RefCell, rc::Rc};

pub(super) fn id(name: &str) -> TerminalId {
    TerminalId::new(name).unwrap()
}

/// A window whose workspace has one herdr-core tab split into `names`, the
/// first pane holding the keyboard.
pub(super) fn window_with_split<'a>(
    cx: &'a mut TestAppContext,
    names: &[&str],
) -> (Entity<SheprdWindow>, &'a mut VisualTestContext, TabId) {
    let (view, cx) = cx.add_window_view(crate::palette::fixture_window);
    let tab = cx.update(|window, cx| {
        Store::default().install(cx);
        view.update(cx, |view, cx| {
            let mut panes = Panes::single(id(names[0]));
            for pair in names.windows(2) {
                panes.split(&id(pair[0]), Axis::Row, id(pair[1])).unwrap();
            }
            let (scope, workspace) = view.browser_key().unwrap();
            let location = Location::core_terminal(panes.clone(), false);
            let tab = Store::update(cx, |store| {
                store.open(scope, &workspace, Some(location.clone()), None)
            })
            .unwrap();
            // Saved as a restored tab is, so syncing keeps what the test
            // changes rather than reading the tab again.
            let mut shown = TabPanes::new(Host::Local, panes, false);
            shown.saved = Some(location);
            view.core_terminals.tabs.insert(tab, shown);
            for name in names {
                let link = worker::spawn(Scripted::default(), id(name), size(80, 24), Role::Active)
                    .unwrap();
                let painter = Rc::new(RefCell::new(TerminalPainter::default()));
                let pane =
                    cx.new(|cx| CoreTerminalView::new(id(name), link, look(), painter, false, cx));
                view.core_terminals.views.insert((tab, id(name)), pane);
            }
            let first = view.core_terminals.views[&(tab, id(names[0]))].clone();
            let focus = first.read(cx).focus_handle().clone();
            window.focus(&focus, cx);
            tab
        })
    });
    (view, cx, tab)
}

pub(super) fn flash(view: &Entity<SheprdWindow>, cx: &mut VisualTestContext) -> Option<String> {
    cx.update(|_, cx| {
        view.read(cx)
            .flash
            .as_ref()
            .map(|(flash, _)| flash.text.to_string())
    })
}

pub(super) fn run(view: &Entity<SheprdWindow>, cx: &mut VisualTestContext, command: Command) {
    cx.update(|window, cx| view.update(cx, |view, cx| view.command(command, window, cx)));
}

#[gpui::test]
fn a_command_a_core_pane_cannot_run_says_so(cx: &mut TestAppContext) {
    let (view, cx, tab) = window_with_split(cx, &["t1", "t2"]);
    // A mirror's pane is moved where it types.
    cx.update(|_, cx| {
        view.update(cx, |view, _| {
            view.core_terminals.tabs.get_mut(&tab).unwrap().observe = true;
        })
    });
    run(&view, cx, Command::MovePane);
    assert_eq!(flash(&view, cx), Some(fl!("core-terminals-unsupported")));
}

#[gpui::test]
fn zoom_and_the_pane_in_use_are_saved_with_the_tab(cx: &mut TestAppContext) {
    let (view, cx, tab) = window_with_split(cx, &["t1", "t2"]);
    run(&view, cx, Command::FocusRight);
    run(&view, cx, Command::Zoom);
    cx.update(|_, cx| view.update(cx, |view, cx| view.save_core_panes(cx)));
    let location = cx.update(|_, cx| {
        cx.global::<Store>()
            .get(tab)
            .and_then(|tab| tab.location.clone())
    });
    let Some(Location::CoreTerminal { active, zoomed, .. }) = location else {
        panic!("a terminal tab");
    };
    assert_eq!(active, Some(id("t2")));
    assert_eq!(zoomed, Some(id("t2")));
}
