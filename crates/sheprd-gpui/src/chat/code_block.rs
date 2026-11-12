//! A code block's frame, which chat messages and the Settings preview share:
//! a header with the block's name and buttons, then its rows, wrapped or
//! scrolling sideways, optionally numbered and striped. Callers make each
//! row's text and fill and the buttons; this lays them out.

use crate::config::{FontConfig, Theme, mix};
use crate::fonts::StyledFont;
use gpui::{prelude::*, *};

/// How much of the ink a striped row mixes into the block's fill, in percent.
const STRIPE: u32 = 4;
/// The same under high contrast, so the stripes stay visible.
const STRIPE_HIGH: u32 = 8;
/// Space around the rows and between the gutter and the code.
const PAD: f32 = 14.;
const GUTTER_GAP: f32 = 10.;

/// How a block draws its rows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Look {
    pub(crate) wrap: bool,
    pub(crate) numbers: bool,
    pub(crate) stripes: bool,
}

/// One source line: its text element and its fill.
pub(crate) struct Row {
    pub(crate) fill: u32,
    pub(crate) text: AnyElement,
}

/// The fill of every second row when stripes are on: the block's fill
/// moved slightly toward its ink, so darker in a light theme and lighter in
/// a dark one, and syntax colors keep their contrast on either.
pub(crate) fn stripe(theme: &Theme) -> u32 {
    let percent = match theme.contrast {
        crate::contrast::Contrast::High => STRIPE_HIGH,
        crate::contrast::Contrast::Standard => STRIPE,
    };
    mix(theme.surface, theme.foreground, percent)
}

/// Row `index`'s fill. Only rows on the block's own fill are striped, so a
/// diff's tints keep their meaning.
pub(crate) fn row_fill(theme: &Theme, index: usize, own: u32, stripes: bool) -> u32 {
    if stripes && index % 2 == 1 && own == theme.surface {
        stripe(theme)
    } else {
        own
    }
}

/// Line `number` padded to `width` digits. Code is drawn in the terminal
/// face, which is monospaced, so every number takes the same width.
pub(crate) fn line_number(number: usize, width: usize) -> String {
    format!("{number:>width$}")
}

/// The digits of the largest of `count` line numbers.
pub(crate) fn digits(count: usize) -> usize {
    count.max(1).ilog10() as usize + 1
}

/// The block's box, which the caller fills with [`header`] and [`body`].
pub(crate) fn frame(theme: &Theme) -> Div {
    div()
        .debug_selector(|| "chat-code-block".into())
        .min_w_0()
        .rounded(px(6.))
        .border_1()
        .border_color(rgb(theme.active))
        .bg(rgb(theme.surface))
        .text_color(rgb(theme.foreground))
        .overflow_hidden()
}

/// The bar over the rows: the block's name, then its buttons in order.
pub(crate) fn header(
    theme: &Theme,
    size: f32,
    name: String,
    buttons: impl IntoIterator<Item = AnyElement>,
) -> Div {
    div()
        .flex()
        .justify_between()
        .items_center()
        .gap(px(8.))
        .px(px(PAD))
        .py(px(6.))
        .border_b_1()
        .border_color(rgb(theme.active))
        .text_size(px(size))
        .text_color(rgb(theme.muted))
        .child(div().min_w_0().truncate().child(name))
        .child(
            div()
                .flex_none()
                .flex()
                .items_center()
                .gap(px(4.))
                .children(buttons),
        )
}

/// A header button, as the copy button looks: quiet text that fills on
/// hover. `backdrop` fills it when the chat's layout lays a ground under it.
pub(crate) fn button(
    theme: &Theme,
    size: f32,
    backdrop: Option<u32>,
    name: &'static str,
    label: String,
) -> Stateful<Div> {
    let hover = theme.active;
    div()
        .id(name)
        .debug_selector(move || name.into())
        .cursor_pointer()
        .text_size(px(size))
        .text_color(rgb(theme.subtext()))
        .px(px(6.))
        .py(px(2.))
        .rounded(px(4.))
        .when_some(backdrop, |button, fill| button.bg(rgb(fill)))
        .hover(move |button| button.bg(rgb(hover)))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(label)
}

/// The rows, in the terminal face. Wrapped rows each carry their number;
/// unwrapped ones scroll sideways beside a gutter that stays put, which
/// lines up because each unwrapped row is exactly one line tall.
pub(crate) fn body(theme: &Theme, font: &FontConfig, look: Look, rows: Vec<Row>) -> Div {
    let height = px(font.size * 1.5);
    let width = digits(rows.len());
    let muted = theme.muted;
    let number = move |index: usize| {
        div()
            .flex_none()
            .whitespace_nowrap()
            .pl(px(PAD))
            .pr(px(GUTTER_GAP))
            .min_h(height)
            .text_color(rgb(muted))
            .child(line_number(index + 1, width))
    };
    let text_start = if look.numbers { 0. } else { PAD };
    let base = div()
        .min_w_0()
        .py(px(PAD))
        .text_size(px(font.size))
        .line_height(relative(1.5))
        .text_font(font);
    if look.wrap {
        return base
            .debug_selector(|| "chat-code-scroll".into())
            .flex()
            .flex_col()
            .children(rows.into_iter().enumerate().map(|(index, row)| {
                div()
                    .debug_selector(move || format!("chat-code-line-{index}"))
                    .flex()
                    .w_full()
                    .min_h(height)
                    .bg(rgb(row.fill))
                    .when(look.numbers, |line| line.child(number(index)))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .pl(px(text_start))
                            .pr(px(PAD))
                            .child(row.text),
                    )
            }));
    }
    let gutter = look.numbers.then(|| {
        div()
            .debug_selector(|| "chat-code-gutter".into())
            .flex_none()
            .flex()
            .flex_col()
            .children(
                rows.iter()
                    .enumerate()
                    .map(|(index, row)| number(index).bg(rgb(row.fill))),
            )
    });
    base.flex().children(gutter).child(
        div()
            .id("code-scroll")
            .debug_selector(|| "chat-code-scroll".into())
            .flex_1()
            .min_w_0()
            .overflow_x_scroll()
            .flex()
            .flex_col()
            .items_start()
            .children(rows.into_iter().enumerate().map(|(index, row)| {
                div()
                    .debug_selector(move || format!("chat-code-line-{index}"))
                    .flex_none()
                    .min_w_full()
                    .whitespace_nowrap()
                    .min_h(height)
                    .bg(rgb(row.fill))
                    .pl(px(text_start))
                    .pr(px(PAD))
                    .child(row.text)
            })),
    )
}

#[cfg(test)]
mod tests;
