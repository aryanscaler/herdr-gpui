//! Annotating terminal text: a note field over the pane the user selected
//! text in, and the queue of notes waiting to go to that pane's agent. What a
//! note holds and the prompt it becomes live in [`crate::terminal_notes`].
//!
//! As with the find field, typing in the note field never reaches the
//! terminal: its key handler steps aside while the field has focus.
// i18n: migrated

use super::{Flash, SheprdWindow};
use crate::i18n::{Localize, fl};
use crate::{
    Error,
    agent_notes::Origin,
    core_terminals::CoreTerminalView,
    search_input::SearchInput,
    terminal::Selection,
    terminal_notes::{self, MAX_NOTES, Note},
};
use gpui::{prelude::*, *};
use sheprd_client::protocol::ClientShellSnapshot;

mod core;
mod offer;
mod render;

/// A queued note and the daemon whose agent it goes to, or the Herdr 2
/// agent, by the note's target terminal; a note with neither is copied.
struct Queued {
    origin: Option<Origin>,
    core: Option<core::CoreAgent>,
    note: Note,
}

/// Where the text a note quotes was selected.
enum Source {
    /// A Herdr 1 pane of the selected daemon.
    Pane {
        origin: Origin,
        pane_id: String,
        selection: Selection,
    },
    /// A herdr-core view, which keeps its own selection.
    Core(Entity<CoreTerminalView>),
}

/// The note being written, about the text selected in `source`.
struct Composer {
    input: Entity<SearchInput>,
    source: Source,
    target: Option<String>,
    place: String,
    quote: String,
}

/// The window's terminal notes: the one being written and those queued.
#[derive(Default)]
pub(crate) struct TerminalNotes {
    composer: Option<Composer>,
    /// A herdr-core pane whose selection is being read for a note.
    core_quote: Option<Entity<CoreTerminalView>>,
    offer: Option<offer::Offer>,
    queued: Vec<Queued>,
}

impl TerminalNotes {
    #[cfg(test)]
    pub(crate) fn queued(&self) -> usize {
        self.queued.len()
    }

    #[cfg(test)]
    pub(crate) fn composing(&self) -> bool {
        self.composer.is_some()
    }
}

/// The agent a note on `pane_id` goes to: the pane's own, or else the first in
/// its workspace. Also where the pane is, as `workspace / tab`.
fn destination(snapshot: &ClientShellSnapshot, pane_id: &str) -> (Option<String>, String) {
    let Some(pane) = snapshot.panes.iter().find(|pane| pane.pane_id == pane_id) else {
        return (None, String::new());
    };
    let target = crate::agent_notes::agent(snapshot, pane_id)
        .or_else(|| {
            snapshot
                .agents
                .iter()
                .find(|agent| agent.workspace_id == pane.workspace_id)
        })
        .map(|agent| agent.pane_id.clone());
    let workspace = snapshot
        .workspaces
        .iter()
        .find(|workspace| workspace.workspace_id == pane.workspace_id)
        .map(|workspace| workspace.label.trim())
        .filter(|label| !label.is_empty());
    let tab = snapshot
        .tabs
        .iter()
        .find(|tab| tab.tab_id == pane.tab_id)
        .and_then(|tab| sheprd_client::text::caption(&tab.label));
    let place = [workspace, tab].into_iter().flatten().collect::<Vec<_>>();
    (target, place.join(" / "))
}

impl SheprdWindow {
    /// Opens the note field over the pane holding the selection.
    pub(crate) fn annotate_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // A newer note replaces one whose quote is still being read.
        self.terminal_notes.core_quote = None;
        if let Some(view) = self.core_terminals.focused_view(window, cx) {
            self.annotate_core_selection(view, cx);
            return;
        }
        let Some(selection) = self.selection.clone() else {
            if self.terminal_notes.offer.is_some() {
                self.open_terminal_annotation_offer(window, cx);
            } else {
                self.show_flash(Flash::warning(fl!("window-annotate-select")), cx);
            }
            return;
        };
        self.compose_terminal_selection(selection, window, cx);
    }

    fn compose_terminal_selection(
        &mut self,
        selection: Selection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(snapshot) = self.live.snapshot.clone() else {
            return;
        };
        let Some(surface) = &self.live.surface else {
            self.show_flash(Flash::warning(fl!("window-annotate-select")), cx);
            return;
        };
        let Some(pane_id) = selection.pane_id().map(str::to_owned) else {
            self.show_flash(Flash::warning(fl!("window-annotate-pane-only")), cx);
            return;
        };
        let quote =
            match selection.text(surface, self.cell_width, self.config.terminal.line_height()) {
                Ok(quote) if !quote.trim().is_empty() => quote,
                Ok(_) => {
                    self.show_flash(Flash::warning(fl!("window-annotate-select")), cx);
                    return;
                }
                Err(Error::SelectionOffscreen) => {
                    self.show_flash(Flash::warning(fl!("window-annotate-offscreen")), cx);
                    return;
                }
                Err(error) => {
                    self.show_flash(
                        Flash::warning(fl!("window-annotate-failed", error = error.localize())),
                        cx,
                    );
                    return;
                }
            };
        let (target, place) = destination(&snapshot, &pane_id);
        let Some(origin) = Origin::of(self) else {
            return;
        };
        let source = Source::Pane {
            origin,
            pane_id,
            selection,
        };
        self.open_terminal_note(source, target, place, quote, window, cx);
    }

    /// Opens the note field about `quote`, focused.
    fn open_terminal_note(
        &mut self,
        source: Source,
        target: Option<String>,
        place: String,
        quote: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // A quote still being read would replace this note when it arrives.
        self.terminal_notes.core_quote = None;
        let input = cx.new(SearchInput::new);
        input.update(cx, |input, cx| {
            input.set_placeholder_with(|| fl!("window-annotate-placeholder"), cx);
            input.set_frameless(cx);
            let mut font = self.config.ui.clone();
            font.size = font.size.max(14.);
            input.set_appearance(font, self.theme.clone(), cx);
        });
        let focus = input.read(cx).focus.clone();
        self.terminal_notes.offer = None;
        self.terminal_notes.composer = Some(Composer {
            input,
            source,
            target,
            place,
            quote,
        });
        window.focus(&focus, cx);
        cx.notify();
    }

    /// Drops the note being written and hands the keyboard back.
    pub(crate) fn close_terminal_note(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(composer) = self.terminal_notes.composer.take() else {
            return;
        };
        if composer.input.read(cx).focus.is_focused(window) {
            // A herdr-core pane takes back the keyboard it gave the note.
            match &composer.source {
                Source::Core(view) if self.core_terminals.shows(view) => {
                    window.focus(&view.read(cx).focus_handle().clone(), cx);
                }
                _ => window.focus(&self.focus, cx),
            }
        }
        cx.notify();
    }

    /// Whether the note field holds the keyboard, so the terminal must not
    /// act on a keystroke bubbling out of it.
    pub(crate) fn terminal_note_focused(&self, window: &Window, cx: &App) -> bool {
        self.terminal_notes
            .composer
            .as_ref()
            .is_some_and(|composer| composer.input.read(cx).focus.is_focused(window))
    }

    /// Runs every tick: retires the field when its pane or connection is gone
    /// or its pane left the screen.
    pub(crate) fn follow_terminal_note(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.follow_terminal_annotation_offer(cx);
        self.poll_core_quote(window, cx);
        let Some(composer) = &self.terminal_notes.composer else {
            return;
        };
        let (origin, pane_id) = match &composer.source {
            Source::Pane {
                origin, pane_id, ..
            } => (origin, pane_id),
            Source::Core(view) => {
                if !self.core_terminals.shows(view) {
                    self.close_terminal_note(window, cx);
                }
                return;
            }
        };
        let current =
            origin.current(self)
                && self.live.snapshot.as_ref().is_some_and(|snapshot| {
                    snapshot.panes.iter().any(|pane| pane.pane_id == *pane_id)
                })
                && (!self.live.surface_ready()
                    || self.live.surface.as_deref().is_some_and(|surface| {
                        surface.popup.is_none()
                            && surface.panes.iter().any(|pane| pane.pane_id == *pane_id)
                    }));
        if !current {
            self.close_terminal_note(window, cx);
        }
    }

    /// Queues the note being written, then sends every queued note when
    /// `send` says so. An empty field still sends what is queued.
    pub(crate) fn add_terminal_note(
        &mut self,
        send: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(composer) = &self.terminal_notes.composer else {
            return;
        };
        let comment = composer.input.read(cx).text().to_owned();
        let note = Note::new(
            composer.target.clone(),
            &composer.place,
            &composer.quote,
            &comment,
        );
        match note {
            Some(note) => {
                if self.terminal_notes.queued.len() >= MAX_NOTES {
                    self.show_flash(
                        Flash::warning(fl!("window-annotate-full", count = MAX_NOTES)),
                        cx,
                    );
                    return;
                }
                let (origin, core) = match &composer.source {
                    Source::Pane { origin, .. } => (Some(origin.clone()), None),
                    Source::Core(view) => (None, self.core_note_agent(view, cx)),
                };
                // The agent a Herdr 2 note goes to is the one there now.
                let note = match &core {
                    Some(agent) => Note {
                        target: Some(agent.terminal.to_string()),
                        ..note
                    },
                    None if origin.is_none() => Note {
                        target: None,
                        ..note
                    },
                    None => note,
                };
                self.terminal_notes
                    .queued
                    .push(Queued { origin, core, note });
            }
            None if send && !self.terminal_notes.queued.is_empty() => {}
            None => {
                self.show_flash(Flash::warning(fl!("window-annotate-empty")), cx);
                return;
            }
        }
        let core = match &composer.source {
            Source::Core(view) => Some(view.clone()),
            Source::Pane { .. } => None,
        };
        self.close_terminal_note(window, cx);
        match core {
            Some(view) => view.update(cx, |view, cx| view.clear_selection(cx)),
            None => self.clear_retained_selection(cx),
        }
        if send {
            self.send_terminal_notes(cx);
        } else {
            let count = self.terminal_notes.queued.len();
            self.show_flash(
                Flash::success(fl!("window-annotate-queued", count = count)),
                cx,
            );
        }
    }

    /// Sends the queued notes, one batch per agent, in the order they were
    /// written.
    pub(crate) fn send_terminal_notes(&mut self, cx: &mut Context<Self>) {
        let clipboard = crate::agent_notes::Copies::default();
        let (core, rest): (Vec<_>, Vec<_>) = std::mem::take(&mut self.terminal_notes.queued)
            .into_iter()
            .partition(|queued| queued.core.is_some() && queued.note.target.is_some());
        self.send_core_notes(core, &clipboard, cx);
        let (mut queued, copied): (Vec<_>, Vec<_>) = rest.into_iter().partition(|queued| {
            queued
                .origin
                .as_ref()
                .is_some_and(|origin| origin.current(self))
                && queued.note.target.is_some()
        });
        while let Some(first) = queued.first() {
            let (origin, target) = (first.origin.clone(), first.note.target.clone());
            let (batch, rest): (Vec<_>, Vec<_>) = queued
                .into_iter()
                .partition(|queued| queued.origin == origin && queued.note.target == target);
            queued = rest;
            let text = terminal_notes::prompt(batch.iter().map(|queued| &queued.note));
            let target = target.map(|pane_id| crate::browser::FeedbackKey {
                scope: crate::browser::scope(&self.endpoints[self.selected_endpoint]),
                pane_id,
            });
            clipboard.extend(
                self.deliver_notes(target, text, None, Some(clipboard.clone()), cx)
                    .copy(),
            );
        }
        if !copied.is_empty() {
            // Feedback is keyed by daemon, not session: once the session a
            // note was written in is gone, its pane ID may name another
            // pane, so even the fallback could reach the wrong agent.
            let stale = copied.iter().any(|queued| {
                queued
                    .origin
                    .as_ref()
                    .is_some_and(|origin| !origin.current(self))
            });
            let text = terminal_notes::prompt(copied.iter().map(|queued| &queued.note));
            clipboard.extend(
                self.deliver_notes(None, text, None, Some(clipboard.clone()), cx)
                    .copy(),
            );
            if stale {
                self.show_flash(Flash::warning(fl!("window-annotate-stale-copied")), cx);
            }
        }
        if let Some(text) = clipboard.text() {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }

    /// Keys the field leaves alone: Escape drops the note, Enter adds it and
    /// sends the queue, Shift-Enter only queues it.
    fn terminal_note_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(composer) = &self.terminal_notes.composer else {
            return;
        };
        if composer.input.read(cx).is_composing() {
            return;
        }
        let modifiers = event.keystroke.modifiers;
        match event.keystroke.key.as_str() {
            "escape" if !modifiers.modified() => self.close_terminal_note(window, cx),
            "enter" if modifiers.shift => self.add_terminal_note(false, window, cx),
            "enter" if !modifiers.modified() => self.add_terminal_note(true, window, cx),
            _ => return,
        }
        cx.stop_propagation();
        window.prevent_default();
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests;
