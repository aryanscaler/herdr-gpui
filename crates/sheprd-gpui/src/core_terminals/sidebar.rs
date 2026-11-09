//! The sidebar's Herdr 2 rows: every pane of the focused workspace's
//! herdr-core tabs, under the agents, with the status dot its agent reports
//! and the mark of the agent it runs.
//! Clicking a row shows its tab and gives that pane the keyboard. Mirrors are
//! left out: the terminal they watch is listed where it types.

use crate::{
    SheprdWindow,
    browser::{Location, Store, TabId},
    fonts::StyledFont,
    i18n::fl,
    sidebar::{label_text, status_indicator},
};
use gpui::{prelude::*, *};
use sheprd_backend::TerminalId;

impl SheprdWindow {
    /// Every pane of the focused workspace's herdr-core tabs that type into
    /// their terminals, tab by tab, as the sidebar and Go To list them.
    pub(crate) fn listed_core_panes(&self, cx: &App) -> Vec<(TabId, TerminalId)> {
        let (Some(store), Some((scope, workspace))) =
            (cx.try_global::<Store>(), self.browser_key())
        else {
            return Vec::new();
        };
        store
            .in_workspace(&scope, &workspace)
            .filter_map(|tab| match &tab.location {
                Some(Location::CoreTerminal {
                    panes,
                    observe: false,
                    ..
                }) => Some((tab.id, panes.terminals())),
                _ => None,
            })
            .flat_map(|(tab, terminals)| {
                terminals
                    .into_iter()
                    .map(move |terminal| (tab, terminal.clone()))
            })
            .collect()
    }

    /// The Herdr 2 rows, or nothing when the workspace has no herdr-core tab.
    pub(crate) fn core_sidebar_rows(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let panes = self.listed_core_panes(cx);
        if panes.is_empty() {
            return None;
        }
        let theme = &self.theme;
        let font = &self.config.sidebar;
        let indicators = self.core_indicators(cx);
        let hover = theme.active;
        let rows = panes.into_iter().map(|(tab, terminal)| {
            let (title, status) = self.core_pane_label(tab, &terminal, cx);
            // The tab's host's record: IDs repeat across machines.
            let agent = self
                .core_terminals
                .info(tab, &terminal)
                .and_then(sheprd_core_client::agent_kind);
            let id = SharedString::from(format!("core-sidebar-{}-{terminal}", u64::from(tab)));
            let clicked = terminal.clone();
            // The ID beside a title, unless the title already is the ID.
            let address = (title != terminal.as_str()).then(|| terminal.to_string());
            div()
                .id(id.clone())
                .debug_selector(move || id.to_string())
                .px_3()
                .py_1()
                .mx_1()
                .flex()
                .items_center()
                .gap_2()
                .rounded(px(crate::config::corners::CONTROL))
                .cursor_pointer()
                .hover(move |row| row.bg(rgb(hover)))
                .child(status_indicator(status, font, indicators).flex_none())
                .children(agent.map(|agent| {
                    crate::icons::AgentIcon::from_identity(Some(agent.name()))
                        .element(font.size, theme.muted)
                }))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_color(rgb(theme.foreground))
                        .child(label_text(&title)),
                )
                .children(address.map(|address| {
                    div()
                        .flex_none()
                        .text_color(rgb(theme.muted))
                        .child(label_text(&address))
                }))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.show_core_pane(tab, &clicked, window, cx);
                }))
        });
        Some(
            div()
                .flex()
                .flex_col()
                .pt_2()
                .text_font(font)
                .text_size(px(font.size))
                .child(
                    div()
                        .px_3()
                        .pb_1()
                        .text_color(rgb(theme.muted))
                        .child(label_text(&fl!("core-terminals-sidebar"))),
                )
                .children(rows)
                .into_any_element(),
        )
    }

    /// Shows a Herdr 2 agent the overview lists: its workspace, then its tab
    /// with its terminal in use, or the shell shown in place of Herdr 1.
    /// `tab` and `terminal` are as a `Session` names them.
    pub(crate) fn show_core_agent(
        &mut self,
        endpoint: &str,
        workspace: &str,
        tab: &str,
        terminal: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let in_front = self.endpoints[self.selected_endpoint].id == endpoint
            && self
                .browser_key()
                .is_some_and(|(_, shown)| shown == workspace);
        if !in_front {
            self.navigate_endpoint(endpoint, crate::NavigationTarget::Workspace(workspace), cx);
        }
        let Ok(tab) = tab.parse::<u64>() else {
            return;
        };
        if tab == super::SHELL_TAB {
            if let Some(view) = self.core_fallback_view() {
                window.focus(&view.read(cx).focus_handle().clone(), cx);
            }
            return;
        }
        let Ok(terminal) = TerminalId::new(terminal) else {
            return;
        };
        self.show_core_pane(TabId::from(tab), &terminal, window, cx);
    }

    /// Shows `tab` with `terminal` as its pane in use, unzoomed if another
    /// pane filled it, and gives that pane the keyboard.
    pub(crate) fn show_core_pane(
        &mut self,
        tab: TabId,
        terminal: &TerminalId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(shown) = self.core_terminals.tabs.get_mut(&tab) {
            if shown.zoom.is_open() && shown.zoomed.as_ref() != Some(terminal) {
                shown.set_zoom(None);
            }
            shown.activate(terminal.clone());
        }
        self.show_browser_tab(tab, window, cx);
        cx.notify();
    }
}
