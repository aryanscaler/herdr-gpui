//! The right-click menus of herdr-core (Herdr 2) tabs and panes. Herdr's
//! own menus act through its daemon; these act on the tab's panes as the
//! window keeps them, in the same rows and order where the action exists:
//! a new tab, renaming, splitting, zooming, mirroring, and closing.

use super::{Purpose, panes::Axis};
use crate::{
    SheprdWindow,
    browser::{Store, TabId},
    i18n::fl,
    menu::Page,
    notifications::safe_text,
    search_input::SearchInput,
};
use gpui::{prelude::*, *};
use sheprd_backend::TerminalId;

/// A row of the menu's page: an action, or a place the pane can move to.
#[derive(Clone, Debug)]
enum Row {
    Action(Action, String),
    Move(super::moves::Destination, String),
}

/// The most characters a name the user gives keeps.
const MAX_NAME_CHARS: usize = 128;

/// What the menu acts on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Target {
    Tab(TabId),
    Pane { tab: TabId, terminal: TerminalId },
}

impl Target {
    fn tab(&self) -> TabId {
        match self {
            Self::Tab(tab) | Self::Pane { tab, .. } => *tab,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Action {
    NewTab,
    Rename,
    SplitRight,
    SplitDown,
    Zoom,
    Unzoom,
    MoveTo,
    Mirror,
    Close,
}

impl Action {
    fn label(self, target: &Target) -> String {
        match (self, target) {
            (Self::NewTab, _) => fl!("tab-menu-new-tab"),
            (Self::Rename, Target::Tab(_)) => fl!("tab-menu-rename"),
            (Self::Rename, Target::Pane { .. }) => fl!("pane-menu-rename"),
            (Self::SplitRight, _) => fl!("pane-menu-split-right"),
            (Self::SplitDown, _) => fl!("pane-menu-split-down"),
            (Self::Zoom, _) => fl!("pane-menu-zoom"),
            (Self::Unzoom, _) => fl!("pane-menu-unzoom"),
            (Self::MoveTo, _) => fl!("pane-menu-move-to"),
            (Self::Mirror, _) => fl!("core-terminals-menu-mirror"),
            (Self::Close, Target::Tab(_)) => fl!("tab-menu-close"),
            (Self::Close, Target::Pane { .. }) => fl!("pane-menu-close"),
        }
    }
}

pub(crate) struct CoreMenu {
    target: Target,
    selected: Option<usize>,
    /// The name being typed on the rename page.
    input: Option<Entity<SearchInput>>,
    /// What held the keyboard as the menu opened: a pane of another tab, or
    /// a chat's composer over the target's pane, still takes the keys back.
    return_focus: Option<FocusHandle>,
    /// Whether `return_focus` was a pane's, which may have gone since.
    from_pane: bool,
}

impl CoreMenu {
    #[cfg(test)]
    pub(crate) fn target(&self) -> &Target {
        &self.target
    }
}

/// `name` as a tab or pane keeps it: one safe, bounded line, or none when
/// blank, which gives the program's title back.
pub(crate) fn clean_name(name: &str) -> Option<String> {
    let name: String = safe_text(name, MAX_NAME_CHARS * 4)
        .chars()
        .take(MAX_NAME_CHARS)
        .collect();
    let name = name.trim();
    (!name.is_empty()).then(|| name.to_owned())
}

impl SheprdWindow {
    /// The rows the menu offers, in Herdr's order. A mirror only watches,
    /// so it neither splits nor mirrors again.
    pub(crate) fn core_menu_actions(&self, target: &Target) -> Vec<Action> {
        let Some(shown) = self.core_terminals.tabs.get(&target.tab()) else {
            return Vec::new();
        };
        match target {
            Target::Tab(_) => vec![Action::NewTab, Action::Rename, Action::Close],
            Target::Pane { terminal, .. } => {
                let mut actions = Vec::new();
                if !shown.observe {
                    actions.extend([Action::Rename, Action::SplitRight, Action::SplitDown]);
                }
                if shown.panes.count() > 1 {
                    let zoomed = shown.zoom.is_open() && shown.zoomed.as_ref() == Some(terminal);
                    actions.push(if zoomed { Action::Unzoom } else { Action::Zoom });
                }
                if !shown.observe {
                    actions.extend([Action::MoveTo, Action::Mirror]);
                }
                actions.push(Action::Close);
                actions
            }
        }
    }

    /// Opens the menu of `target` at `anchor`, as a right-click on a
    /// herdr-core tab or pane does.
    pub(crate) fn open_core_menu(
        &mut self,
        target: Target,
        anchor: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Read before the menu takes the keyboard.
        let return_focus = window.focused(cx);
        let from_pane = return_focus
            .as_ref()
            .is_some_and(|focus| self.is_core_pane_focus(focus, cx));
        if !self.core_terminals.tabs.contains_key(&target.tab()) || !self.open_menu(window, cx) {
            return;
        }
        self.menu.anchor = anchor;
        self.menu.page = Some(Page::CoreMenu);
        self.menu.core = Some(CoreMenu {
            target,
            selected: None,
            input: None,
            return_focus,
            from_pane,
        });
    }

    /// Opens the rename dialog of the focused herdr-core tab or pane, as its
    /// menu's Rename row would, just below the tab strip.
    pub(super) fn rename_core(
        &mut self,
        target: Target,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_core_menu(target, self.bounds.origin, window, cx);
        if self.menu.page == Some(Page::CoreMenu) {
            self.activate_core_menu(Action::Rename, window, cx);
        }
    }

    pub(super) fn activate_core_menu(
        &mut self,
        action: Action,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(target) = self.menu.core.as_ref().map(|menu| menu.target.clone()) else {
            return;
        };
        let tab = target.tab();
        if let (Action::MoveTo, Target::Pane { terminal, .. }) = (action, &target) {
            self.open_core_move(tab, terminal.clone(), window, cx);
            return;
        }
        if action == Action::Rename {
            let current = self.core_name(&target).unwrap_or_default();
            let input = cx.new(SearchInput::new);
            input.update(cx, |input, cx| {
                input.set_appearance(self.config.ui.clone(), self.theme.clone(), cx);
                input.set_placeholder_with(|| fl!("pane-menu-rename-placeholder"), cx);
                input.set_text_selected(&current, cx);
                window.focus(&input.focus, cx);
            });
            if let Some(menu) = &mut self.menu.core {
                menu.input = Some(input);
            }
            self.menu.page = Some(Page::RenameCore);
            cx.notify();
            return;
        }
        self.dismiss_menu(window, cx);
        // A pane that exited while its menu was open is no longer the tab's:
        // acting on it would zoom, split, or mirror nothing, or close the rest.
        if let Target::Pane { terminal, .. } = &target
            && !self
                .core_terminals
                .tabs
                .get(&tab)
                .is_some_and(|shown| shown.panes.contains(terminal))
        {
            cx.notify();
            return;
        }
        match (action, target) {
            (Action::NewTab, _) => self.new_core_terminal(window, cx),
            (Action::Close, Target::Tab(_)) => self.close_browser_tab(tab, window, cx),
            (Action::Close, Target::Pane { terminal, .. }) => {
                self.close_core_pane(tab, terminal, window, cx)
            }
            (Action::SplitRight | Action::SplitDown, Target::Pane { terminal, .. }) => {
                let axis = match action {
                    Action::SplitRight => Axis::Row,
                    _ => Axis::Column,
                };
                self.start_core_program(
                    Purpose::Split {
                        tab,
                        at: terminal,
                        axis,
                    },
                    sheprd_backend::Spawn::default(),
                    cx,
                );
            }
            (Action::Zoom | Action::Unzoom, Target::Pane { terminal, .. }) => {
                if let Some(shown) = self.core_terminals.tabs.get_mut(&tab) {
                    shown.activate(terminal.clone());
                    shown.set_zoom((action == Action::Zoom).then_some(terminal));
                    shown.unsaved = true;
                }
                self.focus_core_terminal(tab, window, cx);
                cx.notify();
            }
            (Action::Mirror, Target::Pane { terminal, .. }) => {
                self.mirror_core_pane(tab, terminal, window, cx)
            }
            _ => {}
        }
    }

    /// Whether `focus` is a herdr-core pane's the window still shows.
    fn is_core_pane_focus(&self, focus: &FocusHandle, cx: &App) -> bool {
        self.core_terminals
            .views
            .values()
            .any(|view| view.read(cx).focus_handle() == focus)
    }

    /// Where the keyboard goes back to as a herdr-core menu closes: what
    /// held it as the menu opened, else the pane in use of the menu's tab,
    /// rather than the window, whose keys a Herdr pane would take.
    pub(crate) fn core_menu_return_focus(&self, cx: &App) -> Option<FocusHandle> {
        let menu = self.menu.core.as_ref()?;
        // A pane whose program exited while the menu was open is gone: the
        // keys go to the pane left in use instead.
        if let Some(focus) = &menu.return_focus
            && (!menu.from_pane || self.is_core_pane_focus(focus, cx))
        {
            return Some(focus.clone());
        }
        Some(
            self.core_pane_view(menu.target.tab())?
                .read(cx)
                .focus_handle()
                .clone(),
        )
    }

    /// Closes a pane: its terminal ends, unless the tab only mirrors it. The
    /// last pane closes as its tab does.
    pub(super) fn close_core_pane(
        &mut self,
        tab: TabId,
        terminal: TerminalId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // A menu left open over a pane that has since exited names one
        // the tab no longer holds: closing it must stop nothing else.
        let Some(shown) = self
            .core_terminals
            .tabs
            .get(&tab)
            .filter(|shown| shown.panes.contains(&terminal))
        else {
            return;
        };
        if shown.panes.count() <= 1 {
            self.close_browser_tab(tab, window, cx);
            return;
        }
        // A mirror's pane stops watching; the terminal runs on.
        let (mirror, host) = (shown.observe, shown.host.clone());
        if self.remove_core_pane(tab, &terminal, window, cx) && !mirror {
            self.end_core_terminal(&host, terminal);
        }
    }

    /// Starts the menu's page with nothing selected.
    pub(super) fn reset_core_selection(&mut self) {
        if let Some(menu) = &mut self.menu.core {
            menu.selected = None;
        }
    }

    /// The rows the menu's page lists: its actions, or where its pane can
    /// move.
    fn core_menu_rows(&self, cx: &App) -> Vec<Row> {
        let Some(menu) = &self.menu.core else {
            return Vec::new();
        };
        match (&menu.target, self.menu.page) {
            (Target::Pane { tab, terminal }, Some(Page::CoreMove)) => self
                .core_moves(*tab, terminal, cx)
                .into_iter()
                .map(|row| Row::Move(row.destination, row.place))
                .collect(),
            (target, _) => self
                .core_menu_actions(target)
                .into_iter()
                .map(|action| Row::Action(action, action.label(target)))
                .collect(),
        }
    }

    fn activate_core_row(&mut self, row: Row, window: &mut Window, cx: &mut Context<Self>) {
        match row {
            Row::Action(action, _) => self.activate_core_menu(action, window, cx),
            Row::Move(destination, _) => {
                let Some(Target::Pane { tab, terminal }) =
                    self.menu.core.as_ref().map(|menu| menu.target.clone())
                else {
                    return;
                };
                self.dismiss_menu(window, cx);
                self.move_core_pane(tab, terminal, destination, window, cx);
            }
        }
    }

    /// The name the user gave `target`, if any.
    fn core_name(&self, target: &Target) -> Option<String> {
        let shown = self.core_terminals.tabs.get(&target.tab())?;
        match target {
            Target::Tab(_) => shown.name.clone(),
            Target::Pane { terminal, .. } => shown.names.get(terminal).cloned(),
        }
    }

    /// Names the menu's tab or pane what was typed, or clears the name.
    fn submit_core_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(menu) = &self.menu.core else {
            return;
        };
        let name = menu
            .input
            .as_ref()
            .and_then(|input| clean_name(input.read(cx).text()));
        let target = menu.target.clone();
        self.set_core_name(&target, name, cx);
        self.dismiss_menu(window, cx);
    }

    /// Names `target`, or clears its name, and saves it with the tab.
    pub(crate) fn set_core_name(&mut self, target: &Target, name: Option<String>, cx: &mut App) {
        let tab = target.tab();
        let Some(shown) = self.core_terminals.tabs.get_mut(&tab) else {
            return;
        };
        match target {
            Target::Tab(_) => shown.name.clone_from(&name),
            Target::Pane { terminal, .. } => match &name {
                Some(name) => {
                    shown.names.insert(terminal.clone(), name.clone());
                }
                None => {
                    shown.names.remove(terminal);
                }
            },
        }
        shown.unsaved = true;
        // The strip shows the new title at once rather than next tick. With
        // the name cleared and no title from the program yet, the tab reads
        // as it did when it opened.
        let Some(shown) = self.core_terminals.tabs.get(&tab) else {
            return;
        };
        let title = (self.core_tab_title(tab, shown, cx))
            .unwrap_or_else(|| shown.location().default_title());
        Store::update(cx, |store| store.visited(tab, None, Some(&title)));
    }

    pub(crate) fn core_menu_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(menu) = &self.menu.core else {
            return;
        };
        let key = event.keystroke.key.as_str();
        let renaming = self.menu.page == Some(Page::RenameCore);
        // The name field takes every key but Escape and Enter, and those too
        // while an input method composes.
        if renaming
            && (menu
                .input
                .as_ref()
                .is_some_and(|input| input.read(cx).is_composing())
                || !matches!(key, "escape" | "enter"))
        {
            return;
        }
        cx.stop_propagation();
        window.prevent_default();
        let rows = self.core_menu_rows(cx);
        let selected = menu.selected;
        match key {
            "escape" => self.dismiss_menu(window, cx),
            "enter" if renaming => self.submit_core_rename(window, cx),
            "up" | "down" if !rows.is_empty() => {
                let count = rows.len();
                let next = match (selected, key) {
                    (None, "up") => count - 1,
                    (None, _) => 0,
                    (Some(i), "up") => (i + count - 1) % count,
                    (Some(i), _) => (i + 1) % count,
                };
                if let Some(menu) = &mut self.menu.core {
                    menu.selected = Some(next);
                }
                cx.notify();
            }
            "enter" => {
                if let Some(row) = selected.and_then(|index| rows.into_iter().nth(index)) {
                    self.activate_core_row(row, window, cx);
                }
            }
            _ => {}
        }
    }

    pub(crate) fn render_core_menu(&self, cx: &mut Context<Self>) -> Div {
        let Some(menu) = &self.menu.core else {
            return div();
        };
        let theme = &self.theme;
        if self.menu.page == Some(Page::RenameCore) {
            let title = match menu.target {
                Target::Tab(_) => fl!("tab-menu-rename-title"),
                Target::Pane { .. } => fl!("pane-menu-rename-title"),
            };
            return div()
                .flex()
                .flex_col()
                .p(px(8.))
                .gap(px(12.))
                .child(div().font_weight(FontWeight::SEMIBOLD).child(title))
                .child(
                    div()
                        .text_color(rgb(theme.muted))
                        .child(fl!("pane-menu-rename-hint")),
                )
                .children(menu.input.clone())
                .child(
                    div()
                        .flex()
                        .justify_end()
                        .gap(px(8.))
                        .child(
                            div()
                                .id("core-rename-cancel")
                                .p(px(6.))
                                .cursor_pointer()
                                .child(fl!("pane-menu-cancel"))
                                .on_click(
                                    cx.listener(|this, _, window, cx| {
                                        this.dismiss_menu(window, cx)
                                    }),
                                ),
                        )
                        .child(
                            div()
                                .id("core-rename-submit")
                                .p(px(6.))
                                .rounded(px(crate::config::corners::CONTROL))
                                .bg(rgb(theme.active))
                                .cursor_pointer()
                                .child(fl!("pane-menu-rename-submit"))
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.submit_core_rename(window, cx)
                                })),
                        ),
                );
        }
        let mut body = div().flex().flex_col();
        if self.menu.page == Some(Page::CoreMove) {
            body = body.child(
                div()
                    .px(px(8.))
                    .py(px(4.))
                    .text_color(rgb(theme.muted))
                    .child(fl!("pane-menu-move-heading")),
            );
        }
        for (index, row) in self.core_menu_rows(cx).into_iter().enumerate() {
            let label = match &row {
                Row::Action(_, label) | Row::Move(_, label) => label.clone(),
            };
            body = body.child(
                div()
                    .id(("core-menu-action", index))
                    .debug_selector(move || format!("core-menu-{index}"))
                    .min_h(px(self.config.ui.line_height() + 12.))
                    .px(px(8.))
                    .flex()
                    .items_center()
                    .cursor_pointer()
                    .when(menu.selected == Some(index), |row| {
                        row.bg(rgb(theme.active))
                    })
                    .hover(|row| row.bg(rgb(theme.active)))
                    .child(div().min_w_0().truncate().child(label))
                    .on_hover(cx.listener(move |this, hovered, _, cx| {
                        if *hovered && let Some(menu) = &mut this.menu.core {
                            menu.selected = Some(index);
                            cx.notify();
                        }
                    }))
                    .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                        // The picker opens under the pointer; a double-click's
                        // second click must not pick the row it lands on.
                        if event.click_count() > 1 {
                            return;
                        }
                        this.activate_core_row(row.clone(), window, cx)
                    })),
            );
        }
        body
    }
}
