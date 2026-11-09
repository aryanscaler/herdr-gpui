//! Keyboard copy mode in a herdr-core pane. The mode is the one a Herdr 1
//! pane runs ([`crate::copy_mode::CopyMode`]), with the same keys, prompt,
//! marker, and highlights; this view's emulator answers its motions,
//! searches, and the final read through the worker instead of the daemon.

use super::{CoreTerminalView, history::scrollback};
use crate::{
    copy_mode::{Command, CopyMode, Outcome, Request, badge},
    i18n::fl,
    search_input::SearchInput,
    terminal_painter::Highlight,
};
use gpui::{prelude::*, *};
use sheprd_backend::History;
use sheprd_client::scrollback::{SearchDirection, TextRange};

pub(super) struct CopyModeState {
    mode: CopyMode,
    /// The search prompt `/` or `?` opened, until Enter submits it or
    /// Escape closes it.
    prompt: Option<Prompt>,
}

struct Prompt {
    direction: SearchDirection,
    input: Entity<SearchInput>,
}

impl CopyModeState {
    pub(super) fn forget_mark(&mut self) {
        self.mode.forget_mark();
    }

    /// Stops waiting on a request the old link would have answered.
    pub(super) fn relinked(&mut self) {
        self.mode.send_failed();
    }
}

impl CoreTerminalView {
    /// Enters copy mode at the terminal's cursor, or at the start of the
    /// last row when the cursor is hidden.
    pub(crate) fn enter_copy_mode(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(pane) = self.surface_pane() else {
            return;
        };
        let cursor = self
            .screen
            .frame
            .as_ref()
            .and_then(|frame| frame.cursor.as_ref())
            .filter(|cursor| cursor.visible)
            .map(|cursor| (cursor.x, cursor.y));
        self.leave_copy_mode(window, cx);
        self.selection = None;
        self.marked.clear();
        self.copy_mode = Some(CopyModeState {
            mode: CopyMode::new(&pane, cursor),
            prompt: None,
        });
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// Leaves copy mode, scrolling back to where the view was when it began.
    pub(crate) fn leave_copy_mode(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_copy_search(window, cx);
        let Some(state) = self.copy_mode.take() else {
            return;
        };
        if let Some(offset) = state.mode.entry_offset() {
            self.scroll_history(usize::try_from(offset).unwrap_or(usize::MAX));
        }
        cx.notify();
    }

    pub(crate) fn copy_mode_active(&self) -> bool {
        self.copy_mode.is_some()
    }

    /// The cells copy mode has marked with `v` or `V`, if any.
    pub(super) fn copy_mode_marked(&self) -> Option<TextRange> {
        let pane = self.surface_pane()?;
        self.copy_mode.as_ref()?.mode.marked(&pane)
    }

    /// Whether copy mode is on with no motion or search waiting on its
    /// answer.
    #[cfg(test)]
    pub(crate) fn copy_mode_settled(&self) -> bool {
        self.copy_mode
            .as_ref()
            .is_some_and(|state| state.mode.in_flight().is_none())
    }

    /// A key while copy mode is on: a copy-mode key runs and every other is
    /// swallowed rather than typed. Keys with Command are the window's.
    pub(super) fn copy_mode_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let keystroke = &event.keystroke;
        let modifiers = keystroke.modifiers;
        if modifiers.platform {
            return;
        }
        cx.stop_propagation();
        window.prevent_default();
        if modifiers.alt || modifiers.function {
            return;
        }
        if let Some(state) = &mut self.copy_mode
            && state.prompt.take().is_some()
        {
            cx.notify();
        }
        if let Some(command) = Command::from_key(&keystroke.key, modifiers.shift, modifiers.control)
        {
            self.copy_mode_command(command, None, window, cx);
        }
    }

    /// Text an input method committed while copy mode is on, read as keys.
    /// What follows a `/` or `?` starts the search query.
    pub(super) fn copy_mode_text(
        &mut self,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        for (index, c) in text.char_indices() {
            let Some(command) = Command::from_char(c) else {
                continue;
            };
            let rest = matches!(command, Command::Prompt(_))
                .then(|| &text[index.saturating_add(c.len_utf8())..]);
            self.copy_mode_command(command, rest, window, cx);
            if rest.is_some() {
                return;
            }
        }
    }

    /// Runs `command`; `query` starts the prompt it may open.
    fn copy_mode_command(
        &mut self,
        command: Command,
        query: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(pane) = self.surface_pane() else {
            return;
        };
        let Some(state) = &mut self.copy_mode else {
            return;
        };
        match state.mode.command(command, &pane) {
            // A dropped request's answer finds nothing waiting for it.
            Outcome::Nothing | Outcome::Abandoned(_) => {}
            Outcome::Moved => self.reveal_copy_cursor(),
            Outcome::Prompt(direction) => {
                self.open_copy_search(direction, query.unwrap_or_default(), window, cx);
            }
            Outcome::Send(request) => self.send_copy_request(&request),
            Outcome::Copy {
                range,
                content_revision,
            } => {
                match content_revision {
                    // A match is copied only as it was found: its text
                    // revision was doubled for copy mode.
                    Some(found) => self.copying = Some(self.read_at(range, found / 2, true)),
                    None => self.copy_range(range),
                }
                self.leave_copy_mode(window, cx);
            }
            Outcome::Exit => self.leave_copy_mode(window, cx),
        }
        cx.notify();
    }

    fn send_copy_request(&mut self, request: &Request) {
        let history = match request {
            Request::Motion(params) => History::Motion {
                cursor: params.cursor,
                motion: params.motion,
            },
            Request::Search { params, .. } => History::Search(sheprd_backend::Search {
                query: params.query.clone(),
                direction: params.direction,
                cursor: params.cursor,
                previous: params.previous,
            }),
        };
        let id = self.ask(history);
        if let Some(state) = &mut self.copy_mode {
            state.mode.sent(id.to_string(), request);
        }
    }

    /// Reads `range` from the history and copies it once it arrives, as a
    /// selection reaching past the screen is copied.
    pub(super) fn copy_range(&mut self, range: TextRange) {
        self.copying = Some(self.read(range));
    }

    fn reveal_copy_cursor(&mut self) {
        let Some(pane) = self.surface_pane() else {
            return;
        };
        let offset = self
            .copy_mode
            .as_mut()
            .and_then(|state| state.mode.reveal(&pane));
        if let Some(offset) = offset {
            self.scroll_history(usize::try_from(offset).unwrap_or(usize::MAX));
        }
    }

    /// Whether `id` is the motion or search copy mode waits on.
    pub(super) fn awaits_copy_mode(&self, id: u64) -> bool {
        self.copy_mode
            .as_ref()
            .and_then(|state| state.mode.in_flight())
            .is_some_and(|request| request == id.to_string())
    }

    /// Applies a motion's or search's answer, keeping the cursor in view.
    pub(super) fn copy_mode_answered(&mut self, id: u64, answer: super::history::Answer) {
        let Some(pane) = self.surface_pane() else {
            return;
        };
        let Some(state) = &mut self.copy_mode else {
            return;
        };
        match state
            .mode
            .answer(&id.to_string(), scrollback(&pane, answer))
        {
            Ok(true) => self.reveal_copy_cursor(),
            Ok(false) => {}
            Err(error) => {
                tracing::warn!(%error, "A herdr-core copy-mode request failed");
            }
        }
    }

    /// Runs keys that waited behind a motion or search, once it answered.
    pub(super) fn poll_copy_mode(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        while let Some(command) = self
            .copy_mode
            .as_mut()
            .and_then(|state| state.mode.next_queued())
        {
            self.copy_mode_command(command, None, window, cx);
        }
    }

    pub(super) fn copy_mode_highlights(&self) -> Vec<Highlight> {
        match (&self.copy_mode, self.surface_pane()) {
            (Some(state), Some(pane)) => state.mode.highlights(&pane),
            _ => Vec::new(),
        }
    }

    fn open_copy_search(
        &mut self,
        direction: SearchDirection,
        query: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.copy_mode.is_none() {
            return;
        }
        let input = cx.new(SearchInput::new);
        let chrome = self.look.chrome.clone();
        input.update(cx, |input, cx| {
            input.set_placeholder_with(|| fl!("window-find-copy-search-placeholder"), cx);
            input.set_appearance(chrome.ui.clone(), chrome.theme.clone(), cx);
            if !query.is_empty() {
                input.replace_text_in_range(None, query, window, cx);
            }
        });
        window.focus(&input.read(cx).focus.clone(), cx);
        if let Some(state) = &mut self.copy_mode {
            state.prompt = Some(Prompt { direction, input });
        }
        cx.notify();
    }

    /// Closes the prompt, handing the keyboard back to copy mode, and
    /// returns the query and direction it held.
    fn close_copy_search(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<(String, SearchDirection)> {
        let prompt = self.copy_mode.as_mut()?.prompt.take()?;
        let input = prompt.input.read(cx);
        let query = input.text().to_owned();
        if input.focus.is_focused(window) {
            window.focus(&self.focus, cx);
        }
        cx.notify();
        Some((query, prompt.direction))
    }

    /// Whether the search prompt holds the keyboard.
    pub(super) fn copy_search_focused(&self, window: &Window, cx: &App) -> bool {
        self.copy_mode
            .as_ref()
            .and_then(|state| state.prompt.as_ref())
            .is_some_and(|prompt| prompt.input.read(cx).focus.is_focused(window))
    }

    /// Enter searches for the prompt's query and Escape closes the prompt;
    /// both stay in copy mode.
    fn copy_search_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(prompt) = self
            .copy_mode
            .as_ref()
            .and_then(|state| state.prompt.as_ref())
        else {
            return;
        };
        if prompt.input.read(cx).is_composing() || event.keystroke.modifiers.modified() {
            return;
        }
        match event.keystroke.key.as_str() {
            "escape" => {
                self.close_copy_search(window, cx);
            }
            "enter" => {
                if let Some((query, direction)) = self.close_copy_search(window, cx) {
                    self.copy_mode_command(Command::Search { query, direction }, None, window, cx);
                }
            }
            _ => return,
        }
        cx.stop_propagation();
        window.prevent_default();
    }

    /// The marker, or the prompt while one is open, in the view's bottom
    /// right corner.
    pub(super) fn render_copy_mode(&self, cx: &mut Context<Self>) -> Option<Div> {
        let state = self.copy_mode.as_ref()?;
        let chrome = &self.look.chrome;
        let corner = div()
            .absolute()
            .bottom_0()
            .left_0()
            .right_0()
            .flex()
            .justify_end()
            .p(px(6.));
        if let Some(prompt) = &state.prompt {
            return Some(corner.child(badge::prompt(
                &prompt.input,
                prompt.direction,
                &chrome.theme,
                chrome.ui.size,
                cx.listener(Self::copy_search_key_down),
            )));
        }
        let label = state.mode.search_label().map_or_else(
            || fl!("window-find-copy-mode"),
            |search| fl!("window-find-copy-mode-search", search = search),
        );
        Some(corner.child(badge::label(label, &chrome.theme, chrome.ui.size)))
    }
}
