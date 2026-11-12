//! The agents-list row's scroll on the wire: a re-click of the shown pane
//! sends one `pane.scroll` to the bottom and no `pane.focus`, and a send
//! that fails reports the row's own label, not copy mode's.

use super::*;
use crate::state::ConnectionStatus;
use gpui::Entity;
use sheprd_client::protocol::ClientMessage;
use std::sync::Arc;

/// The next `pane.` request on the wire, answering others.
fn next_pane_request(peer: &mut MockPeer) -> Value {
    loop {
        if let ClientMessage::ClientShellEndpointRequest { boot_id, request } = peer.receive() {
            let request: Value = serde_json::from_str(&request).unwrap();
            if request["method"]
                .as_str()
                .is_some_and(|method| method.starts_with("pane."))
            {
                return request;
            }
            let id = request["id"].as_str().unwrap();
            peer.respond(&boot_id, id, &json!({"id": id, "result": {"type": "ok"}}));
        }
    }
}

/// A window ready for input whose snapshot focuses the fixture pane, so a
/// row click on `w1:p1` resolves to the rescroll.
fn shown(cx: &mut TestAppContext, peer: &MockPeer) -> Entity<SheprdWindow> {
    let (view, cx) = cx.add_window_view(|window, cx| {
        let mut view = fixture_window(window, cx);
        peer.prepare(&mut view, cx);
        // `prepare` detaches, which clears the surface request; a coherent
        // 80x24 surface needs the request flag back.
        view.endpoints[0].request_surface_for_test();
        Arc::make_mut(view.live.snapshot.as_mut().unwrap()).focused_pane_id = Some("w1:p1".into());
        view.live.status = ConnectionStatus::Connected;
        view
    });
    view.update(cx, |view, _| {
        // The frame follows the size the window asked, as a real handshake's
        // reply would.
        let surface = Arc::make_mut(view.live.surface.as_mut().unwrap());
        surface.frame.width = view.options.surface_size.cols;
        surface.frame.height = view.options.surface_size.rows;
        assert!(
            view.input_ready(),
            "fixture window should be ready for input"
        );
    });
    view
}

#[gpui::test]
fn a_row_rescroll_sends_one_scroll_per_click_and_no_focus(cx: &mut TestAppContext) {
    let mut peer = MockPeer::advertising(&["pane.focus", "pane.scroll"]);
    let view = shown(cx, &peer);
    view.update(cx, |view, cx| {
        assert!(view.open_agent_row("local", "w1:p1", cx));
        assert!(view.open_agent_row("local", "w1:p1", cx));
    });
    for _ in 0..2 {
        let scroll = next_pane_request(&mut peer);
        assert_eq!(scroll["method"], "pane.scroll");
        assert_eq!(
            scroll["params"],
            json!({"pane_id": "w1:p1", "offset_from_bottom": 0})
        );
        let id = scroll["id"].as_str().unwrap();
        peer.respond("boot-v1", id, &json!({"id": id, "result": {"type": "ok"}}));
    }
}

#[gpui::test]
fn a_failed_scroll_reports_the_row_label(cx: &mut TestAppContext) {
    let peer = MockPeer::advertising(&[]);
    let view = shown(cx, &peer);
    peer.client.handle.disconnect();
    view.update(cx, |view, cx| {
        assert!(view.open_agent_row("local", "w1:p1", cx));
        let error = view.local_error.as_deref().unwrap_or_default();
        assert!(
            error.starts_with("Scroll to bottom not sent:"),
            "the row's scroll reports under its own label: {error}"
        );
        // Copy mode's own scroll keeps its label.
        view.local_error = None;
        let boot = view.live.snapshot.as_ref().unwrap().boot_id.clone();
        view.scroll_pane(&boot, "w1:p1", 100, cx);
        let error = view.local_error.as_deref().unwrap_or_default();
        assert!(
            error.starts_with("Scroll not sent:"),
            "copy mode's scroll keeps its label: {error}"
        );
    });
}
