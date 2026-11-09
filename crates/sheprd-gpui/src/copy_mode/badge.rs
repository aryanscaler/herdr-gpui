//! Copy mode's marker and search prompt, shared by the window's over a
//! Herdr 1 pane and a herdr-core view's own: a small label in the pane's
//! bottom right corner, or the `/` and `?` prompt in its place.

use crate::{config::Theme, search_input::SearchInput};
use gpui::{prelude::*, *};
use sheprd_client::scrollback::SearchDirection;

/// The search prompt, holding its field. `on_key_down` sees the field's
/// keys first, for Enter and Escape.
pub(crate) fn prompt(
    input: &Entity<SearchInput>,
    direction: SearchDirection,
    theme: &Theme,
    text_size: f32,
    on_key_down: impl Fn(&KeyDownEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let marker = match direction {
        SearchDirection::Forward => "/",
        SearchDirection::Backward => "?",
    };
    div()
        .id("copy-search")
        .debug_selector(|| "copy-search".into())
        .occlude()
        .min_w_0()
        .w(px(280.))
        .flex_shrink(1.)
        .flex()
        .items_center()
        .gap(px(4.))
        .p(px(4.))
        .rounded(px(crate::config::corners::CONTROL))
        .border_1()
        .border_color(rgb(theme.active))
        .bg(rgb(theme.surface))
        .text_color(rgb(theme.foreground))
        .text_size(px(text_size))
        .shadow_md()
        .on_key_down(on_key_down)
        .child(div().flex_none().px(px(4.)).child(marker))
        .child(div().flex_1().min_w_0().child(input.clone()))
}

/// The marker saying copy mode is on, with the last search and its count.
pub(crate) fn label(label: String, theme: &Theme, text_size: f32) -> Div {
    div()
        .debug_selector(|| "copy-mode".into())
        .min_w_0()
        .overflow_hidden()
        .whitespace_nowrap()
        .text_ellipsis()
        .px(px(8.))
        .py(px(2.))
        .rounded(px(crate::config::corners::CONTROL))
        .bg(rgb(theme.palette[3]))
        .text_color(rgb(theme.text_on(theme.palette[3])))
        .text_size(px(text_size))
        .child(label)
}
