//! The Fonts page's code block: an HTML sample in the terminal face, drawn
//! as a chat draws code and following Settings > Chat's code rows, with a
//! paragraph long enough to show wrapping.
// i18n: migrated
use super::SettingsWindow;
use crate::{
    chat::code_block,
    i18n::fl,
    review::highlight::{Contrast, Span, colour_file},
};
use gpui::{prelude::*, *};
use std::sync::LazyLock;

/// The sample: code, the same in every language.
pub(in crate::settings_window) const SAMPLE: &str = "<!doctype html>\n<html lang=\"en\">\n  <body>\n    <p>This paragraph is a single line of source, long enough to run past the edge of the block, so it shows whether code blocks wrap long lines or scroll them sideways.</p>\n  </body>\n</html>"; // i18n: data

/// The sample's colors, worked out once: they depend on nothing else.
static SPANS: LazyLock<Vec<Vec<Span>>> =
    LazyLock::new(|| colour_file("index.html", SAMPLE, usize::MAX)); // i18n: data

impl SettingsWindow {
    pub(in crate::settings_window) fn render_html_preview(&self, cx: &mut Context<Self>) -> Div {
        let theme = &self.theme;
        let chat = &self.config.chat;
        let font = &self.config.terminal;
        let wraps = self.controls.html_wrap.unwrap_or(chat.wrap_code);
        let look = code_block::Look {
            wrap: wraps,
            numbers: chat.code_line_numbers,
            stripes: chat.code_stripes,
        };
        let rows = SAMPLE
            .lines()
            .enumerate()
            .map(|(index, line)| {
                let fill = code_block::row_fill(theme, index, theme.surface, look.stripes);
                let marks: Vec<_> = SPANS
                    .get(index)
                    .into_iter()
                    .flatten()
                    .map(|span| {
                        let color = span.token.colour(theme, Contrast::On(fill));
                        (
                            span.start..span.end,
                            HighlightStyle {
                                color: Some(rgb(color).into()),
                                ..Default::default()
                            },
                        )
                    })
                    .collect();
                code_block::Row {
                    fill,
                    text: div()
                        .child(StyledText::new(line.to_owned()).with_highlights(marks))
                        .into_any_element(),
                }
            })
            .collect();
        let size = (self.config.ui.size - 2.).max(10.);
        let wrap = code_block::button(
            theme,
            size,
            None,
            "fonts-html-wrap",
            if wraps {
                fl!("chat-code-unwrap")
            } else {
                fl!("chat-code-wrap")
            },
        )
        .on_click(cx.listener(move |this, _, _, cx| {
            this.controls.html_wrap = Some(!wraps);
            cx.notify();
        }));
        let copy = code_block::button(
            theme,
            size,
            None,
            "fonts-html-copy",
            fl!("chat-copy-code-label"),
        )
        .on_click(|_, _, cx| cx.write_to_clipboard(ClipboardItem::new_string(SAMPLE.to_owned())));
        div()
            .debug_selector(|| "fonts-html-preview".into())
            .mt(px(12.))
            .child(
                code_block::frame(theme)
                    .child(code_block::header(
                        theme,
                        size,
                        "html".to_owned(), // i18n: data
                        [wrap.into_any_element(), copy.into_any_element()],
                    ))
                    .child(code_block::body(theme, font, look, rows)),
            )
    }
}
