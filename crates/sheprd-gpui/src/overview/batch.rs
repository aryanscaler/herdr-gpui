//! Acting on several rows at once: Shift or Cmd/Ctrl click picks rows, and a
//! bar above the list settles, snoozes, or replies to all of them.

use super::{Key, Row, Sections, State, view::pill};
use crate::{SheprdWindow, devices_overview::Look, i18n::fl};
use gpui::{prelude::*, *};

impl SheprdWindow {
    /// Picks `row` into the selection, or a range ending there with `range`.
    pub(super) fn select_overview_row(&mut self, row: &Row, range: bool, cx: &mut Context<Self>) {
        let pick = row.pick();
        self.overview.menu = None;
        if range {
            let sections = self.overview_sections(cx);
            let unfolded = self.overview.idle_open;
            self.overview.selection.extend(
                &pick,
                &sections.active(unfolded).map(Row::pick).collect::<Vec<_>>(),
            );
        } else {
            self.overview.selection.toggle(&pick);
        }
        if self.overview.selection.is_empty() {
            self.overview.batch_menu = false;
        }
        cx.notify();
    }

    /// The selected rows of `sections`, as the page drew them.
    fn overview_selected(&self, sections: &Sections) -> Vec<Row> {
        sections
            .active(self.overview.idle_open)
            .filter(|row| self.overview.selection.contains(&row.pick()))
            .cloned()
            .collect()
    }

    fn clear_overview_selection(&mut self, cx: &mut Context<Self>) {
        self.overview.selection.clear();
        self.overview.batch_menu = false;
        cx.notify();
    }

    /// The bar over the list while rows are selected.
    pub(super) fn overview_batch(
        &self,
        look: &Look,
        sections: &Sections,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement> {
        if self.overview.selection.is_empty() {
            return None;
        }
        let theme = look.theme;
        let rows = self.overview_selected(sections);
        let targets: Vec<(Key, State)> = rows
            .iter()
            .map(|row| (row.key.clone(), row.state))
            .collect();
        let settled = targets.clone();
        let replies = rows.iter().any(|row| row.reply_pane().is_some());
        let mut bar = div()
            .debug_selector(|| "overview-batch".into())
            .relative()
            .flex_none()
            .px_5()
            .py_2()
            .flex()
            .items_center()
            .gap_2()
            .border_b_1()
            .border_color(rgb(theme.active))
            .bg(rgb(theme.surface))
            .child(div().flex_1().font_weight(FontWeight::MEDIUM).child(fl!(
                "overview-selected",
                count = self.overview.selection.len()
            )))
            .child(
                pill(
                    "overview-batch-settle".to_owned(),
                    "icons/check.svg",
                    fl!("overview-settle"),
                    look,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.mark_overview_row(cx, |marks| {
                        settled.iter().fold(false, |changed, (key, state)| {
                            marks.settle(key, *state) | changed
                        })
                    });
                    this.clear_overview_selection(cx);
                })),
            )
            .child(
                pill(
                    "overview-batch-snooze".to_owned(),
                    "icons/coffee.svg",
                    fl!("overview-snooze"),
                    look,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.overview.batch_menu = !this.overview.batch_menu;
                    cx.notify();
                })),
            );
        if replies {
            bar = bar.child(
                pill(
                    "overview-batch-reply".to_owned(),
                    "icons/pencil.svg",
                    fl!("overview-reply"),
                    look,
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.open_overview_reply(rows.clone(), window, cx);
                })),
            );
        }
        bar = bar.child(
            pill(
                "overview-batch-clear".to_owned(),
                "icons/x.svg",
                fl!("overview-selection-clear"),
                look,
            )
            .on_click(cx.listener(|this, _, _, cx| this.clear_overview_selection(cx))),
        );
        if self.overview.batch_menu {
            bar = bar.child(
                deferred(div().absolute().top(px(40.)).right(px(20.)).child(
                    crate::motion::pop_in(
                        self.snooze_menu(targets, look, cx),
                        "overview-batch-snooze-menu",
                    ),
                ))
                .with_priority(1),
            );
        }
        Some(crate::motion::pop_in(bar, "overview-batch-bar"))
    }
}
