use super::*;
use crate::chat::{
    code_block,
    prepared::tool::Code,
    rich::{
        CodeText, MAX_CODE_LINE_CHARS, MAX_CODE_LINES,
        diff::{self, Kind as DiffKind},
    },
    selection::{Cell, Segment},
    view::{Hues, text_ink},
};
use crate::config::Theme;
use crate::i18n::fl;
use crate::review::highlight::{Contrast, Span};
use std::sync::Arc;

/// How much of a change's hue tints its row, in percent.
const DIFF_TINT: u32 = 17;

impl SheprdWindow {
    pub(super) fn rich_code(
        &self,
        id: VisualId,
        language: &str,
        source: &Arc<str>,
        block: usize,
        context: &mut RichContext<'_, '_, '_>,
    ) -> Div {
        let code = CodeView {
            pane_id: context.pane_id,
            id,
            language,
            source,
            block,
        };
        self.render_code_block(
            code,
            context.visuals,
            Some(context.selection),
            None,
            context.cx,
        )
    }

    /// A tool card's source, or an HTML fence shown as code, drawn like a
    /// message's code block. `extra` leads the header's buttons.
    pub(in crate::chat::view) fn render_chat_tool_code(
        &self,
        pane_id: &str,
        code: &Code,
        selection: Option<&Scope<'_>>,
        visuals: &Visuals,
        extra: Option<AnyElement>,
        cx: &mut Context<Self>,
    ) -> Div {
        let view = CodeView {
            pane_id,
            id: code.id,
            language: &code.language,
            source: &code.source,
            block: 0,
        };
        self.render_code_block(view, visuals, selection, extra, cx)
    }

    fn render_code_block(
        &self,
        code: CodeView<'_>,
        visuals: &Visuals,
        selection: Option<&Scope<'_>>,
        extra: Option<AnyElement>,
        cx: &mut Context<Self>,
    ) -> Div {
        let CodeView {
            pane_id,
            id,
            language,
            source,
            block,
        } = code;
        let chat = &self.config.chat;
        let wraps = self
            .chats
            .pane(pane_id)
            .map_or(chat.wrap_code, |pane| pane.code.wraps(id, chat.wrap_code));
        let look = code_block::Look {
            wrap: wraps,
            numbers: chat.code_line_numbers,
            stripes: chat.code_stripes,
        };
        let is_diff = diff::is_diff(language);
        let diff_rows = is_diff.then(|| diff::rows(source, MAX_CODE_LINES));
        let spans = visuals.code(id);
        // One pass: the lines past the limit are only checked for, not counted.
        let mut raw = source.lines();
        let lines: Vec<_> = raw
            .by_ref()
            .take(MAX_CODE_LINES)
            .enumerate()
            .map(|(index, raw)| {
                let spans = spans
                    .and_then(|spans| spans.get(index))
                    .map_or(&[][..], Vec::as_slice);
                let cell = Cell {
                    block,
                    row: index,
                    column: 0,
                };
                match selection
                    .and_then(|selection| selection.segment(cell))
                    .and_then(Segment::code)
                {
                    Some(code) => highlighted(code, raw, spans),
                    None => code_line(raw, spans),
                }
            })
            .collect();
        let truncated = raw.next().is_some() || lines.iter().any(|line| line.truncated);
        let rows = lines
            .into_iter()
            .enumerate()
            .map(|(index, line)| {
                let kind = diff_rows
                    .as_ref()
                    .and_then(|rows| rows.get(index))
                    .map(|row| row.kind);
                let (own, ink) = diff_colors(&self.theme, kind);
                let background = code_block::row_fill(&self.theme, index, own, look.stripes);
                let ink = text_ink(ink, background);
                let sign = diff_sign(&self.theme, kind, background).map(|color| {
                    let style = HighlightStyle {
                        color: Some(rgb(color).into()),
                        ..Default::default()
                    };
                    (0..1, style)
                });
                let marks = sign.into_iter().chain(line.spans.into_iter().map(|span| {
                    (
                        span.start..span.end,
                        HighlightStyle {
                            color: Some(
                                rgb(span.token.colour(&self.theme, Contrast::On(background)))
                                    .into(),
                            ),
                            ..Default::default()
                        },
                    )
                }));
                let fragment = selection.and_then(|selection| {
                    selection.fragment(Cell {
                        block,
                        row: index,
                        column: 0,
                    })
                });
                let styled = styled_text(fragment.as_ref(), line.text, marks, &self.theme);
                let layout = styled.layout().clone();
                let text = div()
                    .text_color(rgb(ink))
                    .when(is_diff, |text| text.px(px(6.)))
                    .child(styled);
                code_block::Row {
                    fill: background,
                    text: selectable(fragment.as_ref(), text, layout, cx).into_any_element(),
                }
            })
            .collect();
        let name = if is_diff {
            fl!("chat-diff")
        } else if language.is_empty() {
            fl!("chat-code")
        } else {
            language.to_owned()
        };
        let pane = pane_id.to_owned();
        let wrap = self
            .rich_button(
                "chat-code-wrap",
                if wraps {
                    fl!("chat-code-unwrap")
                } else {
                    fl!("chat-code-wrap")
                },
            )
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                cx.stop_propagation();
                if let Some(pane) = this.chats.pane_mut(&pane) {
                    pane.code.set_wrap(id, !wraps);
                    pane.list.remeasure();
                }
                cx.notify();
            }));
        let copy = self.rich_copy(
            if is_diff { copy::DIFF } else { copy::CODE },
            Arc::clone(source),
            cx,
        );
        let buttons = extra
            .into_iter()
            .chain([wrap.into_any_element(), copy.into_any_element()]);
        code_block::frame(&self.theme)
            .child(code_block::header(
                &self.theme,
                self.chat_text_size() - 2.,
                name,
                buttons,
            ))
            .child(code_block::body(
                &self.theme,
                &self.config.terminal,
                look,
                rows,
            ))
            .when(truncated, |view| {
                view.child(
                    div()
                        .debug_selector(|| "chat-code-truncated".into())
                        .px(px(14.))
                        .pb(px(6.))
                        .text_color(rgb(self.theme.muted))
                        .child(truncation_note()),
                )
            })
    }
}

/// What a cut code block says under its last line, from the limits that
/// cut it.
fn truncation_note() -> String {
    fl!(
        "chat-code-truncated",
        lines = grouped(MAX_CODE_LINES),
        characters = grouped(MAX_CODE_LINE_CHARS),
    )
}

/// `number` with commas between its groups of thousands.
fn grouped(number: usize) -> String {
    let digits = number.to_string();
    let mut text = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            text.push(',');
        }
        text.push(digit);
    }
    text
}

/// A row's fill and ink: tinted for a changed line, muted for a patch's
/// own headers, and the block's own otherwise.
fn diff_colors(theme: &Theme, kind: Option<DiffKind>) -> (u32, u32) {
    let hues = Hues::of(theme);
    let background = match kind {
        Some(DiffKind::Added) => crate::config::mix(theme.surface, hues.green, DIFF_TINT),
        Some(DiffKind::Removed) => crate::config::mix(theme.surface, hues.red, DIFF_TINT),
        _ => theme.surface,
    };
    let ink = if kind == Some(DiffKind::Meta) {
        theme.muted
    } else {
        theme.foreground
    };
    (background, text_ink(ink, background))
}

/// The ink of a changed line's leading sign, so the change does not rest on
/// its row's tint alone.
fn diff_sign(theme: &Theme, kind: Option<DiffKind>, background: u32) -> Option<u32> {
    let hues = Hues::of(theme);
    let color = match kind? {
        DiffKind::Added => hues.green,
        DiffKind::Removed => hues.red,
        DiffKind::Context | DiffKind::Meta => return None,
    };
    Some(text_ink(color, background))
}

struct CodeView<'a> {
    pane_id: &'a str,
    id: VisualId,
    language: &'a str,
    source: &'a Arc<str>,
    block: usize,
}

struct CodeLine {
    text: String,
    spans: Vec<Span>,
    truncated: bool,
}

/// A line without a selectable segment, as when selection is unavailable.
fn code_line(raw: &str, spans: &[Span]) -> CodeLine {
    highlighted(&CodeText::expand(raw), raw, spans)
}

/// Tabs are layout, not unsafe controls. Grammar byte offsets map through
/// the drawn line's expansion; the original source remains the copy payload.
/// A prepared segment's line is reused, so drawing and selecting expanded
/// tabs cannot drift apart and the map is not rebuilt per frame.
fn highlighted(code: &CodeText, raw: &str, spans: &[Span]) -> CodeLine {
    let spans = spans
        .iter()
        .filter_map(|span| {
            let start = code.shown(span.start)?;
            let end = code.shown(span.end.min(code.end))?;
            (start < end).then_some(Span {
                start,
                end,
                token: span.token,
            })
        })
        .collect();
    CodeLine {
        text: code.text.clone(),
        spans,
        truncated: code.end < raw.len(),
    }
}

#[cfg(test)]
mod tests;
