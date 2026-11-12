//! A row whose pane the app draws as a chat rescrolls the transcript
//! client-locally: nothing goes to the daemon for it.

use super::{
    overlay::{attach_with, open},
    *,
};
use crate::{
    connection::ConnectionBridge,
    navigation::{AgentRowAction, RescrollTarget},
    terminal::InputTarget,
    window::{MockPeer, SheprdWindow},
};
use gpui::{Entity, TestAppContext, VisualTestContext};
use sheprd_client::{
    ConnectTarget,
    protocol::{ClientMessage, ClientPaneInputEvent},
};

fn behind(view: &Entity<SheprdWindow>, cx: &mut VisualTestContext) -> f32 {
    view.read_with(cx, |view, _| {
        let list = &view.chats.pane("w1:p1").unwrap().list;
        f32::from(list.max_offset_for_scrollbar().y + list.scroll_px_offset_for_scrollbar().y)
    })
}

fn redraw(view: &Entity<SheprdWindow>, cx: &mut VisualTestContext) {
    view.update(cx, |_, cx| cx.notify());
    cx.run_until_parked();
}

fn wheel(delta: f32, cx: &mut VisualTestContext) {
    let chat = cx.debug_bounds("chat-view").unwrap();
    cx.simulate_event(gpui::ScrollEvent {
        position: chat.center(),
        delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.), gpui::px(delta))),
        ..Default::default()
    });
}

/// Proves no request follows on the Herdr wire: an input sentinel anything
/// the rescroll sent would have to precede.
fn assert_wire_quiet(peer: &mut MockPeer, cx: &mut VisualTestContext) {
    let handle = peer.client.handle.clone();
    cx.update(|_, _| {
        ConnectionBridge::send_input(
            &handle,
            "boot-v1",
            &InputTarget::Pane("w1:p1".into()),
            ClientPaneInputEvent::TextCommit("quiet".into()),
        )
        .unwrap();
    });
    loop {
        match peer.receive() {
            ClientMessage::ClientShellPaneInput { events, .. } => {
                assert_eq!(events[0], ClientPaneInputEvent::TextCommit("quiet".into()));
                return;
            }
            ClientMessage::ClientShellEndpointRequest { request, .. } => {
                let request: serde_json::Value = serde_json::from_str(&request).unwrap();
                panic!(
                    "unexpected {} for the chat rescroll",
                    request["method"].as_str().unwrap_or("?")
                );
            }
            _ => {}
        }
    }
}

#[gpui::test]
fn a_shown_chat_means_a_local_transcript_rescroll(cx: &mut TestAppContext) {
    let (view, mut peer, cx) = open(cx, ConnectTarget::Local);
    let program = Program::new();
    let entries = (1..=41)
        .map(|id| agent(id, "A line of an earlier reply."))
        .collect();
    let _stream = attach_with(&view, &program, snapshot(cursor(1, 0), entries), cx);
    redraw(&view, cx);
    view.update(cx, |view, _| {
        // The classifier rescrolls only the pane the snapshot focuses,
        // `prepare`'s detach cleared the surface request, and the frame
        // follows the size the window asked.
        Arc::make_mut(view.live.snapshot.as_mut().unwrap()).focused_pane_id = Some("w1:p1".into());
        view.endpoints[0].request_surface_for_test();
        let surface = Arc::make_mut(view.live.surface.as_mut().unwrap());
        surface.frame.width = view.options.surface_size.cols;
        surface.frame.height = view.options.surface_size.rows;
    });
    view.read_with(cx, |view, _| {
        assert_eq!(
            view.agent_row_action("local", "w1:p1"),
            AgentRowAction::Rescroll(RescrollTarget::Chat {
                pane: "w1:p1".into()
            })
        );
    });
    wheel(200., cx);
    assert!(
        behind(&view, cx) > 1.,
        "the fixture starts away from the end"
    );
    view.update(cx, |view, _| {
        let pane = view.chats.pane_mut("w1:p1").unwrap();
        pane.selection.select_all(pane.prepared.text());
    });
    view.update(cx, |view, cx| {
        assert!(view.open_agent_row("local", "w1:p1", cx));
    });
    redraw(&view, cx);
    view.read_with(cx, |view, _| {
        let pane = view.chats.pane("w1:p1").unwrap();
        assert!(pane.selection.is_empty(), "the rescroll clears a selection");
    });
    assert!(behind(&view, cx) <= 1., "the rescroll reaches the live end");
    assert_wire_quiet(&mut peer, cx);
}
