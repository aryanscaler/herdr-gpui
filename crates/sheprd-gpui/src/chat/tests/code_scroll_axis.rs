//! Each wheel axis scrolls one thing over a code block: vertical input the
//! chat log, horizontal input (Shift + wheel) the code block, and a diagonal
//! trackpad swipe both, each on its own axis.

use super::{
    overlay::{attach_with, open, poll},
    *,
};
use crate::SheprdWindow;
use gpui::{
    Entity, Pixels, ScrollDelta, ScrollEvent, TestAppContext, VisualTestContext, point, px,
};
use sheprd_client::ConnectTarget;

fn log_offset(view: &Entity<SheprdWindow>, cx: &mut VisualTestContext) -> Pixels {
    view.read_with(cx, |view, _| {
        let list = &view.chats.pane("w1:p1").unwrap().list;
        list.scroll_px_offset_for_scrollbar().y
    })
}

fn code_left(cx: &mut VisualTestContext) -> Pixels {
    cx.debug_bounds("chat-code-line-0").unwrap().left()
}

fn redraw(cx: &mut VisualTestContext) {
    cx.update(|window, cx| crate::sidebar::layout_tests::full_draw(window, cx).clear(cx));
}

fn wheel(x: f32, y: f32, cx: &mut VisualTestContext) {
    let code = cx.debug_bounds("chat-code-scroll").unwrap();
    cx.simulate_event(ScrollEvent {
        position: code.center(),
        delta: ScrollDelta::Pixels(point(px(x), px(y))),
        ..Default::default()
    });
    redraw(cx);
}

#[gpui::test]
fn each_wheel_axis_scrolls_one_thing_over_a_code_block(cx: &mut TestAppContext) {
    let (view, _peer, cx) = open(cx, ConnectTarget::Local);
    let program = Program::new();
    let code = format!("```rust\nlet value = \"{}\";\n```", "long_value".repeat(70));
    let mut entries: Vec<_> = (1..=40)
        .map(|id| agent(id, "A line of an earlier reply."))
        .collect();
    entries.push(agent(41, &code));
    let _stream = attach_with(&view, &program, snapshot(cursor(1, 0), entries), cx);
    wait_until(|| {
        poll(&view, &program, cx);
        cx.debug_bounds("chat-code-scroll").is_some()
    });
    redraw(cx);

    let (log, left) = (log_offset(&view, cx), code_left(cx));
    wheel(-120., 0., cx);
    assert_eq!(
        log_offset(&view, cx),
        log,
        "Shift + wheel leaves the log in place"
    );
    assert!(
        code_left(cx) < left,
        "Shift + wheel scrolls the code sideways"
    );

    // The code is off its left edge, so wheel input mapped onto it would show.
    let (log, left) = (log_offset(&view, cx), code_left(cx));
    wheel(0., 30., cx);
    assert_ne!(log_offset(&view, cx), log, "a plain wheel scrolls the log");
    assert_eq!(
        code_left(cx),
        left,
        "a plain wheel leaves the code in place"
    );

    let (log, left) = (log_offset(&view, cx), code_left(cx));
    wheel(-60., 30., cx);
    assert_ne!(
        log_offset(&view, cx),
        log,
        "a diagonal swipe scrolls the log"
    );
    assert!(
        code_left(cx) < left,
        "a diagonal swipe scrolls the code sideways"
    );
}
