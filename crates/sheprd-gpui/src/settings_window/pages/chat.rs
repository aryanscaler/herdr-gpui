//! Chat: how agent chats draw (layout, width, scrolling), where tab strips
//! start them, and how their code blocks start out. Every row needs ACP chats, which General turns on.
// i18n: migrated
use super::{SettingsWindow, on_value, ui};
use crate::settings_window::inputs::NumberField;
use crate::{
    config::{ChatLauncher, ChatLayoutMode, HtmlRender, preferences::Preference},
    i18n::fl,
};
use gpui::{prelude::*, *};

impl SettingsWindow {
    pub(in crate::settings_window) fn render_chat(&self, cx: &mut Context<Self>) -> Div {
        let theme = &self.theme;
        let page = div().flex().flex_col();
        if !self.config.features.acp_chats {
            return page.child(
                div()
                    .debug_selector(|| "chat-off".into())
                    .child(self.control_note(fl!("settings-chat-off"))),
            );
        }
        let enabled = !self.quitting && self.closing.is_none();
        let chat = &self.config.chat;
        let layouts: Vec<_> = ChatLayoutMode::ALL
            .iter()
            .map(|&mode| (mode, mode.label()))
            .collect();
        let launchers: Vec<_> = ChatLauncher::ALL
            .iter()
            .map(|&launcher| (launcher, launcher.label()))
            .collect();
        let renders: Vec<_> = HtmlRender::ALL
            .iter()
            .map(|&mode| (mode, mode.label()))
            .collect();
        page.child(ui::section(theme, &fl!("settings-chat-section-view"), None))
            .child(
                ui::row(theme, "chat-layout", fl!("settings-chat-layout"))
                    .description(fl!("settings-chat-layout-description"))
                    .control(ui::segmented(
                        theme,
                        "chat-layout-control",
                        &layouts,
                        Some(chat.layout),
                        enabled,
                        on_value(cx, |this, mode, _, cx| {
                            this.save_preference(Preference::ChatLayout(mode), cx)
                        }),
                    )),
            )
            .child(self.chat_width_controls(cx))
            .child(
                ui::row(theme, "chat-launcher", fl!("settings-chat-launcher"))
                    .description(fl!("settings-chat-launcher-description"))
                    .control(ui::segmented(
                        theme,
                        "chat-launcher-control",
                        &launchers,
                        Some(chat.launcher),
                        enabled,
                        on_value(cx, |this, launcher, _, cx| {
                            this.save_preference(Preference::ChatLauncher(launcher), cx)
                        }),
                    )),
            )
            .child(
                ui::row(theme, "chat-glide", fl!("settings-chat-glide"))
                    .description(fl!("settings-chat-glide-description"))
                    .switch(self.preference_toggle(
                        "chat-glide-toggle",
                        chat.glide,
                        Preference::ChatGlide(!chat.glide),
                        cx,
                    )),
            )
            .child(ui::section(theme, &fl!("settings-chat-section-code"), None))
            .child(
                ui::row(theme, "chat-wrap-code", fl!("settings-chat-wrap-code"))
                    .description(fl!("settings-chat-wrap-code-description"))
                    .switch(self.preference_toggle(
                        "chat-wrap-code-toggle",
                        chat.wrap_code,
                        Preference::ChatWrapCode(!chat.wrap_code),
                        cx,
                    )),
            )
            .child(
                ui::row(
                    theme,
                    "chat-code-line-numbers",
                    fl!("settings-chat-code-line-numbers"),
                )
                .description(fl!("settings-chat-code-line-numbers-description"))
                .switch(self.preference_toggle(
                    "chat-code-line-numbers-toggle",
                    chat.code_line_numbers,
                    Preference::ChatCodeLineNumbers(!chat.code_line_numbers),
                    cx,
                )),
            )
            .child(
                ui::row(
                    theme,
                    "chat-code-stripes",
                    fl!("settings-chat-code-stripes"),
                )
                .description(fl!("settings-chat-code-stripes-description"))
                .switch(self.preference_toggle(
                    "chat-code-stripes-toggle",
                    chat.code_stripes,
                    Preference::ChatCodeStripes(!chat.code_stripes),
                    cx,
                )),
            )
            .child(
                ui::row(theme, "chat-render-html", fl!("settings-chat-render-html"))
                    .description(fl!("settings-chat-render-html-description"))
                    .control(ui::segmented(
                        theme,
                        "chat-render-html-control",
                        &renders,
                        Some(chat.render_html),
                        enabled,
                        on_value(cx, |this, mode, _, cx| {
                            this.save_preference(Preference::ChatRenderHtml(mode), cx)
                        }),
                    )),
            )
    }

    fn chat_width_controls(&self, cx: &mut Context<Self>) -> Div {
        let full = self.config.chat.max_width == 0;
        let field = NumberField::ChatMaxWidth;
        let value = self.number_value(field, f32::from(self.config.chat.max_width));
        let view = cx.entity().downgrade();
        div()
            .child(
                ui::row(
                    &self.theme,
                    "chat-full-width",
                    fl!("settings-chat-full-width"),
                )
                .description(fl!("settings-chat-full-width-description"))
                .switch(self.preference_toggle(
                    "chat-full-width-toggle",
                    full,
                    Preference::ChatMaxWidth(if full {
                        crate::config::chat::DEFAULT_MAX_WIDTH
                    } else {
                        0
                    }),
                    cx,
                )),
            )
            .when(!full, |controls| {
                controls.child(
                    ui::row(&self.theme, "chat-width", fl!("settings-chat-width"))
                        .description(fl!("settings-chat-width-description"))
                        .control(ui::number(
                            &self.theme,
                            ui::Number {
                                id: field.selector(),
                                value,
                                range: field.range(),
                                unit: ui::Unit::Pixels,
                                look: ui::Look::Slider,
                                enabled: !self.quitting && self.closing.is_none(),
                            },
                            self.number_edit_for(field),
                            on_value(cx, move |this, change, _, cx| {
                                this.commit_number(field, change, cx)
                            }),
                            move |window, cx| {
                                let _ = view.update(cx, |this, cx| {
                                    this.begin_number_edit(field, value, window, cx)
                                });
                            },
                        )),
                )
            })
    }
}
