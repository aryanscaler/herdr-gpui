//! Notes on text selected in a herdr-core pane. The pane keeps its own
//! selection, which may reach into its history; the note card is the one a
//! Herdr 1 pane's note uses, laid over the pane in the window. A note goes
//! to the pane's agent, else the first agent of its tab, typed in once its
//! turn is over, as a Herdr 1 agent's is; with no agent there, or once the
//! agent left or asks a question, it is copied to the clipboard.

use super::*;
use crate::core_terminals::Host;
use sheprd_backend::TerminalId;
use sheprd_core_client::{
    agent::Until,
    reply::{self, Replied},
};
use std::{
    sync::atomic::AtomicBool,
    time::{Duration, Instant},
};

/// How long notes wait for a busy agent before they are typed in anyway, as
/// a Herdr 1 agent's notes do.
const HOLD: Duration = Duration::from_secs(120);

/// The Herdr 2 agent a note goes to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct CoreAgent {
    /// The core its terminal runs in.
    pub(super) host: Host,
    pub(super) terminal: TerminalId,
    /// Its name, so a note never reaches another program that took the
    /// terminal since.
    pub(super) agent: String,
}

impl SheprdWindow {
    /// Opens the note card about `view`'s selection once its text is read
    /// from the history.
    pub(super) fn annotate_core_selection(
        &mut self,
        view: Entity<CoreTerminalView>,
        cx: &mut Context<Self>,
    ) {
        if view.update(cx, |view, _| view.quote_selection()) {
            self.terminal_notes.core_quote = Some(view);
        } else {
            self.show_flash(Flash::warning(fl!("window-annotate-select")), cx);
        }
    }

    /// Opens the card once a quote read from a pane's history arrives.
    pub(super) fn poll_core_quote(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(view) = self.terminal_notes.core_quote.clone() else {
            return;
        };
        // The note is the pane's: one that lost the keyboard, to the
        // palette or anything else, while its quote was read drops it
        // rather than take the keyboard back.
        let focused = view.read(cx).focus_handle().contains_focused(window, cx);
        if !self.core_terminals.shows(&view) || !focused || self.menu.page.is_some() {
            self.terminal_notes.core_quote = None;
            return;
        }
        let Some(quote) = view.update(cx, |view, _| view.take_quote()) else {
            return;
        };
        self.terminal_notes.core_quote = None;
        match quote {
            Ok(quote) => self.compose_core_note(view, quote, window, cx),
            Err(error) => {
                tracing::warn!(%error, "Could not read a herdr-core selection to annotate");
                let reason = match error {
                    crate::core_terminals::Error::Moved => fl!("window-annotate-history-moved"),
                    _ => fl!("window-annotate-history-unread"),
                };
                self.show_flash(
                    Flash::warning(fl!("window-annotate-failed", error = reason)),
                    cx,
                );
            }
        }
    }

    fn compose_core_note(
        &mut self,
        view: Entity<CoreTerminalView>,
        quote: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if quote.trim().is_empty() {
            self.show_flash(Flash::warning(fl!("window-annotate-select")), cx);
            return;
        }
        let place = view.read(cx).title().unwrap_or_default().to_owned();
        let target = self
            .core_note_agent(&view, cx)
            .map(|agent| agent.terminal.to_string());
        self.open_terminal_note(Source::Core(view), target, place, quote, window, cx);
    }

    /// The agent a note on `view`'s pane goes to: the pane's own, else the
    /// first of its tab's panes that runs one. A watch-only mirror types
    /// into nothing, so its notes are copied. The shell shown in place of
    /// Herdr 1 is in no tab and runs in this computer's core.
    pub(super) fn core_note_agent(
        &self,
        view: &Entity<CoreTerminalView>,
        cx: &App,
    ) -> Option<CoreAgent> {
        let terminals = &self.core_terminals;
        if let Some(fallback) = self.core_fallback_view()
            && fallback == *view
        {
            let terminal = view.read(cx).terminal().clone();
            let info = terminals.reported(&Host::Local, &terminal)?;
            return Some(CoreAgent {
                host: Host::Local,
                agent: sheprd_core_client::agent_name(info)?,
                terminal,
            });
        }
        let (tab, terminal) = terminals.view_key(view).or_else(|| {
            let terminal = view.read(cx).terminal().clone();
            Some((terminals.tab_of(&terminal)?, terminal))
        })?;
        // A mirror types into nothing; a tab on another host's core does.
        if !terminals.types_into(tab) {
            return None;
        }
        let host = terminals.host_of(tab);
        let agent = |terminal: &TerminalId| {
            let info = terminals.info(tab, terminal)?;
            Some(CoreAgent {
                host: host.clone(),
                terminal: terminal.clone(),
                agent: sheprd_core_client::agent_name(info)?,
            })
        };
        // Every pane of the tab, a zoomed-away one too.
        agent(&terminal).or_else(|| terminals.tab_terminals(tab).into_iter().find_map(agent))
    }

    /// Types each Herdr 2 agent's notes into its terminal, off the UI
    /// thread, once its turn is over. Notes whose agent's core cannot be
    /// reached go to `clipboard`, with the rest of the Send's copies.
    pub(super) fn send_core_notes(
        &mut self,
        queued: Vec<Queued>,
        clipboard: &crate::agent_notes::Copies,
        cx: &mut Context<Self>,
    ) {
        let mut queued = queued;
        while let Some(first) = queued.first() {
            let Some(agent) = first.core.clone() else {
                queued.remove(0);
                continue;
            };
            let (batch, rest): (Vec<_>, Vec<_>) = queued
                .into_iter()
                .partition(|queued| queued.core.as_ref() == Some(&agent));
            queued = rest;
            let text = terminal_notes::prompt(batch.iter().map(|queued| &queued.note));
            let Some(backend) = self.core_terminals.host_backend(&agent.host).cloned() else {
                clipboard.extend([text]);
                self.show_flash(Flash::warning(fl!("window-annotate-core-unreachable")), cx);
                continue;
            };
            self.deliver_core_notes(backend, agent, text, clipboard.clone(), cx);
        }
    }

    /// Types `text` into `agent`'s terminal once its turn is over. What it
    /// does not take joins the Send's other copies, `copies`, so a later
    /// failure does not overwrite an earlier one on the clipboard.
    fn deliver_core_notes(
        &mut self,
        backend: sheprd_core_client::CoreBackend,
        agent: CoreAgent,
        text: String,
        copies: crate::agent_notes::Copies,
        cx: &mut Context<Self>,
    ) {
        self.show_flash(Flash::success(fl!("agent-notes-will-send")), cx);
        let work = cx.background_executor().spawn(async move {
            let stop = AtomicBool::new(false);
            let until = Until {
                deadline: Instant::now() + HOLD,
                stop: &stop,
            };
            let sent = reply::when_free(
                backend.core(),
                agent.terminal.as_str(),
                &agent.agent,
                &text,
                until,
            );
            (sent, text)
        });
        cx.spawn(async move |this, cx| {
            let (sent, text) = work.await;
            this.update(cx, |this, cx| {
                this.core_notes_delivered(sent, text, &copies, cx);
            })
            .ok();
        })
        .detach();
    }

    /// Says where the notes went; what the agent did not take is copied,
    /// with the rest of its Send's copies.
    pub(super) fn core_notes_delivered(
        &mut self,
        sent: sheprd_core_client::Result<Replied>,
        text: String,
        copies: &crate::agent_notes::Copies,
        cx: &mut Context<Self>,
    ) {
        let delivered = matches!(sent, Ok(Replied::Sent));
        let flash = match sent {
            Ok(Replied::Sent) => Flash::success(fl!("agent-notes-sent")),
            Ok(Replied::Gone) => Flash::warning(fl!("window-annotate-core-gone")),
            Err(sheprd_core_client::Error::Refused {
                kind: Some(kind), ..
            }) if kind == sheprd_core_client::wire::BLOCKED => {
                Flash::warning(fl!("window-annotate-core-blocked"))
            }
            Err(error) => {
                tracing::warn!(%error, "Could not send notes to a Herdr 2 agent");
                Flash::warning(fl!("window-annotate-core-unreachable"))
            }
        };
        if !delivered {
            copies.extend([text]);
            if let Some(text) = copies.text() {
                cx.write_to_clipboard(ClipboardItem::new_string(text));
            }
        }
        self.show_flash(flash, cx);
    }

    /// The card over the herdr-core pane its note is about, beside the
    /// selection's last row the pane shows. The pane's bounds are the
    /// window's, so the card is anchored to the window rather than to the
    /// root, which a client-drawn window frame insets.
    pub(crate) fn render_core_note(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        if self.menu.page.is_some() {
            return None;
        }
        let composer = self.terminal_notes.composer.as_ref()?;
        let Source::Core(view) = &composer.source else {
            return None;
        };
        let view = view.read(cx);
        let pane = view.bounds()?;
        let anchor = view
            .selection_anchor()
            .unwrap_or_else(|| Bounds::new(pane.bottom_left(), size(px(0.), px(0.))));
        let mut font = self.config.ui.clone();
        font.size = font.size.max(14.);
        let bounds = render::placement(pane, anchor, render::card_size(&font));
        let card = self.size_terminal_note(Some(composer), bounds.size, &font, cx);
        Some(deferred(anchored().position(bounds.origin).child(card)))
    }
}
