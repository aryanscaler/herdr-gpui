//! The port chips of a workspace the app keeps for Herdr 2 follow it to the
//! front, with Herdr 1 or without.

use crate::{
    SheprdWindow,
    browser::Location,
    core_terminals::panes::Panes,
    listening_ports::parse,
    sidebar::layout_tests::{SidebarFixture, fixture_window, snapshot},
    usage::Host,
};
use gpui::{Entity, TestAppContext, VisualTestContext, prelude::*};
use sheprd_backend::TerminalId;
use std::{path::Path, sync::Arc};

fn window(
    cx: &mut TestAppContext,
    herdr_1: bool,
) -> (Entity<SheprdWindow>, &mut VisualTestContext) {
    let (fixture, cx) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| fixture_window(window, cx));
        SidebarFixture(view)
    });
    let view = cx.update(|_, cx| fixture.read(cx).0.clone());
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            if herdr_1 {
                let mut snapshot = snapshot(2);
                snapshot.focused_workspace_id = Some("w0".into());
                view.live.snapshot = Some(Arc::new(snapshot));
            } else {
                view.live.snapshot = None;
                view.live.missing_installation = true;
            }
            view.config.show_listening_ports = true;
            view.refresh_core_session(cx);
        })
    });
    (view, cx)
}

/// Keeps a workspace for `t1` and places that terminal's listener in it;
/// the chips the window shows then, with the workspace they open in.
fn kept_chips(view: &Entity<SheprdWindow>, cx: &mut VisualTestContext) -> (String, Vec<String>) {
    let id = cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            let location =
                Location::core_terminal(Panes::single(TerminalId::new("t1").unwrap()), false);
            view.open_kept_workspace(None, Path::new("/src/api"), location, window, cx)
        })
    });
    let id = id.unwrap();
    view.update(cx, |view, _| {
        let scope = crate::browser::scope(&view.endpoints[0]);
        view.listening_ports
            .seed(Host::Local, parse("L 10 *:5173 node\nC 10 t1\n").unwrap());
        view.listening_ports.place_terminals(vec![(
            Host::Local,
            "t1".to_owned(),
            (scope, id.clone()),
        )]);
    });
    view.read_with(cx, |view, _| {
        let (_, workspace, listed) = view.focused_listening_ports().unwrap();
        let ports = listed
            .ports
            .iter()
            .map(|port| port.number.to_string())
            .collect();
        assert_eq!(
            workspace, id,
            "a click opens the page in the kept workspace"
        );
        (id.clone(), ports)
    })
}

#[gpui::test]
fn without_herdr_1_a_kept_workspace_shows_its_own_ports(cx: &mut TestAppContext) {
    let (view, cx) = window(cx, false);
    let (_, ports) = kept_chips(&view, cx);
    assert_eq!(ports, ["5173"]);
}

#[gpui::test]
fn with_herdr_1_a_kept_workspace_in_front_hides_herdrs_ports(cx: &mut TestAppContext) {
    let (view, cx) = window(cx, true);
    let (_, ports) = kept_chips(&view, cx);
    assert_eq!(ports, ["5173"]);
}
