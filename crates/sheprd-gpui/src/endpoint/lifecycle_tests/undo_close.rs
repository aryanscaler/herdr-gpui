//! Undo close over a real connection: a tab closed with `[undo_close_tab]`
//! on reaches Herdr only once its undo window passes, and never when it is
//! reopened within it.
use super::*;
use crate::config::UndoCloseTab;
use std::time::Duration;

/// A window connected to `server`'s daemon, whose focused workspace has two
/// more idle tabs, `first` and `second`, so their closes ask nothing.
fn connected<'a>(
    cx: &'a mut gpui::TestAppContext,
    id: &str,
    window_length: Duration,
) -> (
    gpui::Entity<SheprdWindow>,
    &'a mut gpui::VisualTestContext,
    Server,
) {
    let (endpoint, server) = connected_endpoint(id);
    let (fixture, cx) = cx.add_window_view(|window, cx| {
        Fixture(cx.new(|cx| crate::sidebar::layout_tests::fixture_window(window, cx)))
    });
    let view = fixture.read_with(cx, |fixture, _| fixture.0.clone());
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.endpoints = vec![endpoint];
            view.options = ConnectOptions::default();
            view.config.undo_close_tab = UndoCloseTab {
                enabled: true,
                window: window_length,
            };
            view.reset_selected(cx);
            project_until(view, cx, "initial surface", SheprdWindow::input_ready);
            let shown = Arc::make_mut(view.live.snapshot.as_mut().unwrap());
            for id in ["first", "second"] {
                let mut tab = shown.tabs[0].clone();
                tab.tab_id = id.into();
                tab.workspace_id = shown.focused_workspace_id.clone().unwrap();
                tab.focused = false;
                tab.agent_status = AgentStatus::Idle;
                shown.tabs.push(tab);
            }
            *view.endpoints[0].connection.inbox.lock().unwrap() = view.live.clone();
        })
    });
    (view, cx, server)
}

/// The endpoint requests the daemon receives up to the first `tab.close`,
/// answering each, as one is in flight at a time; the close comes last.
fn requests_to_close(server: &mut Server) -> Vec<serde_json::Value> {
    let mut requests = Vec::new();
    loop {
        let ClientMessage::ClientShellEndpointRequest { request, .. } = server.receive() else {
            continue;
        };
        let request: serde_json::Value = serde_json::from_str(&request).unwrap();
        let close = request["method"] == Method::TabClose.as_str();
        if !close {
            server.respond(&request);
        }
        requests.push(request);
        if close {
            return requests;
        }
    }
}

fn tab_of(request: &serde_json::Value) -> (&str, &str) {
    (
        request["method"].as_str().unwrap(),
        request["params"]["tab_id"].as_str().unwrap_or_default(),
    )
}

#[gpui::test]
fn a_tab_reopened_in_its_undo_window_never_reaches_herdr(cx: &mut gpui::TestAppContext) {
    let (view, cx, mut server) = connected(cx, "undo-close-reopen", Duration::from_secs(60));
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.open_tab_close("first", window, cx);
            assert!(
                view.menu.page.is_none(),
                "an idle tab closes without asking"
            );
            assert!(view.undo_waits("first"));
            view.poll_undo_close(cx);
            // Requests leave in order, so a close of `first` would come
            // before this marker.
            let boot = view.live.snapshot.as_ref().unwrap().boot_id.clone();
            let marker = serde_json::json!({ "tab_id": "marker" });
            let handle = view.endpoints[0].connection.handle.as_ref().unwrap();
            handle.request(&boot, Method::TabFocus, marker).unwrap();
            view.reopen_closed_tab(window, cx);
            assert!(!view.undo_waits("first"));
        })
    });
    let mut seen = Vec::new();
    while seen.len() < 2 {
        let ClientMessage::ClientShellEndpointRequest { request, .. } = server.receive() else {
            continue;
        };
        let request: serde_json::Value = serde_json::from_str(&request).unwrap();
        let (method, tab) = tab_of(&request);
        seen.push((method.to_owned(), tab.to_owned()));
        server.respond(&request);
    }
    // Nothing went to Herdr in the window, and reopening only focuses it.
    assert_eq!(
        seen,
        [
            ("tab.focus".into(), "marker".into()),
            ("tab.focus".into(), "first".into())
        ]
    );
}

#[gpui::test]
fn a_tab_closes_in_herdr_once_its_undo_window_passes(cx: &mut gpui::TestAppContext) {
    let (view, cx, mut server) = connected(cx, "undo-close-due", Duration::ZERO);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.open_tab_close("first", window, cx);
            assert!(view.undo_waits("first"));
            view.poll_undo_close(cx);
            assert!(!view.undo_waits("first"));
        })
    });
    let requests = requests_to_close(&mut server);
    assert_eq!(requests.last().map(tab_of), Some(("tab.close", "first")));
}
