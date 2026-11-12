use super::*;
use crate::{
    config::{HtmlRender, preferences::Preference},
    settings_window::controls::preferences::PreferenceIo,
};
use core::prelude::v1::test;
use std::sync::{Arc, Mutex};

#[gpui::test]
fn chat_code_rows_save_their_defaults(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(skill_fixture);
    let edits = Arc::new(Mutex::new(Vec::new()));
    let written = edits.clone();
    view.update(cx, |view, cx| {
        view.section = Section::Chat;
        view.config.features.acp_chats = true;
        view.controls.preference_io = Some(PreferenceIo::with_load(
            move |edit| written.lock().unwrap().push(edit),
            skill_load,
        ));
        cx.notify();
    });
    cx.simulate_resize(size(px(960.), px(3200.)));
    // The segment's selector is its index in the order Settings lists them.
    assert_eq!(HtmlRender::ALL[1], HtmlRender::Never);
    for (selector, edit) in [
        ("chat-wrap-code-toggle", Preference::ChatWrapCode(true)),
        (
            "chat-code-line-numbers-toggle",
            Preference::ChatCodeLineNumbers(true),
        ),
        (
            "chat-code-stripes-toggle",
            Preference::ChatCodeStripes(true),
        ),
        (
            "chat-render-html-control-1",
            Preference::ChatRenderHtml(HtmlRender::Never),
        ),
    ] {
        cx.update(|window, cx| crate::sidebar::layout_tests::full_draw(window, cx).clear(cx));
        let point = cx
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("{selector}"))
            .center();
        cx.simulate_click(point, Default::default());
        cx.run_until_parked();
        assert_eq!(edits.lock().unwrap().last(), Some(&edit), "{selector}");
    }
}

#[gpui::test]
fn the_chat_page_says_how_to_turn_chats_on(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(skill_fixture);
    view.update(cx, |view, cx| {
        view.section = Section::Chat;
        view.config.features.acp_chats = false;
        cx.notify();
    });
    cx.update(|window, cx| crate::sidebar::layout_tests::full_draw(window, cx).clear(cx));
    assert!(cx.debug_bounds("chat-off").is_some());
    assert!(cx.debug_bounds("chat-wrap-code-toggle").is_none());
}

#[gpui::test]
fn the_fonts_preview_wraps_by_its_own_button(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(skill_fixture);
    view.update(cx, |view, cx| {
        view.section = Section::Fonts;
        cx.notify();
    });
    cx.simulate_resize(size(px(960.), px(3200.)));
    let line = |cx: &mut VisualTestContext| cx.debug_bounds("chat-code-line-3").unwrap();
    cx.update(|window, cx| crate::sidebar::layout_tests::full_draw(window, cx).clear(cx));
    assert!(cx.debug_bounds("fonts-html-preview").is_some());
    let unwrapped = line(cx);
    let wrap = cx.debug_bounds("fonts-html-wrap").unwrap();
    cx.simulate_click(wrap.center(), Default::default());
    cx.update(|window, cx| crate::sidebar::layout_tests::full_draw(window, cx).clear(cx));
    assert!(
        line(cx).size.height > unwrapped.size.height,
        "the long paragraph wraps"
    );
    view.read_with(cx, |view, _| assert!(!view.config.chat.wrap_code));
}
