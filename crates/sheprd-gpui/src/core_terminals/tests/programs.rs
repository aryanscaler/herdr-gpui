//! How a program the app ran in a tab ended, as worktree archives read it:
//! only from this computer's tabs, since terminal IDs repeat across cores.

use super::{
    accessibility::look,
    commands::{id, window_with_split},
    worker::{Scripted, size, until},
};
use crate::{
    browser::{Location, Scope, Store},
    core_terminals::{CoreTerminalView, Host, TabPanes, panes::Panes, worker},
    terminal_painter::TerminalPainter,
};
use gpui::{AppContext as _, TestAppContext};
use sheprd_backend::{Exit, Role, SurfaceUpdate};
use std::{cell::RefCell, rc::Rc};

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod archive_split;

#[gpui::test]
fn another_hosts_ended_terminal_never_answers_for_this_computers(cx: &mut TestAppContext) {
    let (view, cx, local) = window_with_split(cx, &["t1"]);
    let ended = Scripted::default();
    ended.push(vec![SurfaceUpdate::Ended(Exit { code: Some(0) })]);
    let pane = cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            // The same terminal ID in a tab of another host's core.
            let (scope, workspace) = view.browser_key().unwrap();
            let location = Location::core_terminal(Panes::single(id("t1")), false);
            let remote = Store::update(cx, |store| {
                store.open(scope, &workspace, Some(location), None)
            })
            .unwrap();
            let host = Host::Remote(Scope::endpoint("ssh:box"));
            view.core_terminals
                .tabs
                .insert(remote, TabPanes::new(host, Panes::single(id("t1")), false));
            let link = worker::spawn(ended, id("t1"), size(80, 24), Role::Active).unwrap();
            let painter = Rc::new(RefCell::new(TerminalPainter::default()));
            let pane =
                cx.new(|cx| CoreTerminalView::new(id("t1"), link, look(), painter, false, cx));
            view.core_terminals
                .views
                .insert((remote, id("t1")), pane.clone());
            let _ = window;
            pane
        })
    });
    until(|| {
        cx.update(|window, cx| {
            pane.update(cx, |pane, cx| {
                pane.poll(window, cx);
                pane.ended().then_some(())
            })
        })
    });
    cx.update(|_, cx| {
        let view = view.read(cx);
        assert!(
            view.local_terminal_shown(&id("t1")),
            "this computer's tab {local:?}"
        );
        assert_eq!(
            view.core_terminal_exit(&id("t1"), cx),
            None,
            "the local one runs on"
        );
    });
}
