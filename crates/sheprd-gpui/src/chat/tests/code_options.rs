//! A code block's own Wrap button over `[chat] wrap_code`, its line numbers
//! and stripes, and the switch between an HTML fence's page and its code.

use super::{
    overlay::{attach_with, open, poll},
    *,
};
use crate::{SheprdWindow, config::HtmlRender};
use gpui::{Entity, Modifiers, TestAppContext, VisualTestContext};
use sheprd_client::ConnectTarget;

fn said(text: &str) -> ServerMessage {
    let mut transcript = Transcript::new("Claude");
    transcript.status = Status::Idle;
    transcript.entries = vec![agent(1, text)];
    ServerMessage::Snapshot {
        cursor: cursor(1, 0),
        transcript: Box::new(transcript),
    }
}

fn long_code() -> String {
    format!(
        "```rust\nlet value = \"{}\";\n\nfn main() {{}}\n```",
        "long words ".repeat(60)
    )
}

fn set_chat(
    view: &Entity<SheprdWindow>,
    cx: &mut VisualTestContext,
    edit: impl FnOnce(&mut crate::config::chat::ChatConfig),
) {
    view.update(cx, |view, _| edit(&mut view.config.chat));
}

/// Scrolls the chat to its top, so a tall block's header is in the short
/// test window.
fn top(view: &Entity<SheprdWindow>, program: &Program, cx: &mut VisualTestContext) {
    view.update(cx, |view, _| {
        let pane = view.live.snapshot.as_ref().unwrap().agents[0]
            .pane_id
            .clone();
        let chat = view.chats.pane(&pane).unwrap();
        chat.list.scroll_to(gpui::ListOffset {
            item_ix: 0,
            offset_in_item: gpui::px(0.),
        });
        chat.glide.user_scrolled();
    });
    poll(view, program, cx);
    poll(view, program, cx);
}

fn wrapped(cx: &mut VisualTestContext) -> bool {
    let line = cx.debug_bounds("chat-code-line-0").unwrap();
    let blank = cx.debug_bounds("chat-code-line-1").unwrap();
    line.size.height > blank.size.height
}

#[gpui::test]
fn the_wrap_button_wraps_its_block_and_copy_keeps_the_source(cx: &mut TestAppContext) {
    let (view, _peer, cx) = open(cx, ConnectTarget::Local);
    let program = Program::new();
    let text = long_code();
    let _stream = attach_with(&view, &program, said(&text), cx);
    poll(&view, &program, cx);
    assert!(!wrapped(cx), "unwrapped by default");
    let wrap = cx.debug_bounds("chat-code-wrap").unwrap();
    let copy = cx.debug_bounds("chat-copy-code").unwrap();
    assert!(wrap.right() <= copy.left(), "Wrap stands left of Copy");
    cx.simulate_click(wrap.center(), Modifiers::default());
    top(&view, &program, cx);
    assert!(wrapped(cx));
    let scroll = cx.debug_bounds("chat-code-scroll").unwrap();
    let line = cx.debug_bounds("chat-code-line-0").unwrap();
    assert!(
        line.right() <= scroll.right(),
        "a wrapped line fits its block"
    );
    let copy = cx.debug_bounds("chat-copy-code").unwrap();
    cx.simulate_click(copy.center(), Modifiers::default());
    let source = text
        .strip_prefix("```rust\n")
        .and_then(|text| text.strip_suffix("```"))
        .unwrap();
    assert_eq!(
        cx.update(|_, cx| cx.read_from_clipboard())
            .and_then(|item| item.text())
            .as_deref(),
        Some(source)
    );
    let wrap = cx.debug_bounds("chat-code-wrap").unwrap();
    cx.simulate_click(wrap.center(), Modifiers::default());
    poll(&view, &program, cx);
    poll(&view, &program, cx);
    assert!(!wrapped(cx), "a second click unwraps");
}

#[gpui::test]
fn the_default_wraps_until_a_blocks_own_button_says_otherwise(cx: &mut TestAppContext) {
    let (view, _peer, cx) = open(cx, ConnectTarget::Local);
    let program = Program::new();
    set_chat(&view, cx, |chat| chat.wrap_code = true);
    let _stream = attach_with(&view, &program, said(&long_code()), cx);
    top(&view, &program, cx);
    assert!(wrapped(cx));
    let wrap = cx.debug_bounds("chat-code-wrap").unwrap();
    cx.simulate_click(wrap.center(), Modifiers::default());
    poll(&view, &program, cx);
    poll(&view, &program, cx);
    assert!(!wrapped(cx));
}

#[gpui::test]
fn line_numbers_stay_beside_unwrapped_code_and_ride_wrapped_rows(cx: &mut TestAppContext) {
    let (view, _peer, cx) = open(cx, ConnectTarget::Local);
    let program = Program::new();
    set_chat(&view, cx, |chat| {
        chat.code_line_numbers = true;
        chat.code_stripes = true;
    });
    let _stream = attach_with(&view, &program, said(&long_code()), cx);
    poll(&view, &program, cx);
    let gutter = cx.debug_bounds("chat-code-gutter").unwrap();
    let scroll = cx.debug_bounds("chat-code-scroll").unwrap();
    assert!(
        gutter.right() <= scroll.left(),
        "outside the sideways scroll"
    );
    set_chat(&view, cx, |chat| chat.wrap_code = true);
    poll(&view, &program, cx);
    poll(&view, &program, cx);
    assert!(wrapped(cx));
    assert!(
        cx.debug_bounds("chat-code-gutter").is_none(),
        "each wrapped row carries its own number"
    );
}

#[gpui::test]
fn an_html_card_switches_to_its_code_and_back(cx: &mut TestAppContext) {
    let (view, _peer, cx) = open(cx, ConnectTarget::Local);
    let program = Program::new();
    let _stream = attach_with(&view, &program, said("Look:\n```html\n<p>hi</p>\n```"), cx);
    assert!(cx.debug_bounds("chat-html-1").is_some());
    let show = cx.debug_bounds("chat-show-code").unwrap();
    cx.simulate_click(show.center(), Modifiers::default());
    poll(&view, &program, cx);
    poll(&view, &program, cx);
    assert!(cx.debug_bounds("chat-html-1").is_none());
    assert!(cx.debug_bounds("chat-html-code-1").is_some());
    assert!(cx.debug_bounds("chat-code-wrap").is_some());
    let render = cx.debug_bounds("chat-page-render").unwrap();
    assert!(
        render.right() <= cx.debug_bounds("chat-code-wrap").unwrap().left(),
        "the switch leads the header's buttons"
    );
    if crate::browser::EMBEDDED {
        cx.simulate_click(render.center(), Modifiers::default());
        poll(&view, &program, cx);
        poll(&view, &program, cx);
        assert!(cx.debug_bounds("chat-html-code-1").is_none());
    } else {
        // The browser opens and the code stays.
        cx.simulate_click(render.center(), Modifiers::default());
        cx.run_until_parked();
        poll(&view, &program, cx);
        assert!(view.read_with(cx, |view, _| view.flash.is_some()));
        assert!(cx.debug_bounds("chat-html-code-1").is_some());
    }
}

#[gpui::test]
fn never_draws_html_as_code_with_its_switch(cx: &mut TestAppContext) {
    let (view, _peer, cx) = open(cx, ConnectTarget::Local);
    let program = Program::new();
    set_chat(&view, cx, |chat| chat.render_html = HtmlRender::Never);
    let _stream = attach_with(&view, &program, said("Look:\n```html\n<p>hi</p>\n```"), cx);
    poll(&view, &program, cx);
    assert!(cx.debug_bounds("chat-html-code-1").is_some());
    assert!(cx.debug_bounds("chat-page-render").is_some());
    assert!(cx.debug_bounds("chat-html-1").is_none());
}

#[gpui::test]
fn show_code_wins_over_a_page_still_loading(cx: &mut TestAppContext) {
    use crate::chat::page::{PageKey, PageSource};
    let (view, _peer, cx) = open(cx, ConnectTarget::Local);
    let program = Program::new();
    let _stream = attach_with(&view, &program, said("Look:\n```html\n<p>hi</p>\n```"), cx);
    let pane = view.read_with(cx, |view, _| {
        view.live.snapshot.as_ref().unwrap().agents[0]
            .pane_id
            .clone()
    });
    let key = PageKey {
        entry: EntryId(1),
        part: 1,
    };
    // Show code lands while the page is still being copied: both happen
    // before any task runs, so the copy finishes after the choice.
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.show_chat_page(&pane, key, PageSource::Html("<p>hi</p>".into()), window, cx);
            let chat = view.chats.pane_mut(&pane).unwrap();
            chat.code.set_shows_code(key, true);
            chat.remeasure(key.entry);
        })
    });
    cx.run_until_parked();
    poll(&view, &program, cx);
    view.read_with(cx, |view, _| {
        let chat = view.chats.pane(&pane).unwrap();
        assert!(chat.pages.is_empty(), "the page did not open over the code");
        assert_eq!(chat.error, None);
    });
    assert!(cx.debug_bounds("chat-html-code-1").is_some());
}
