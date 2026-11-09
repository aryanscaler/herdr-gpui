//! Notes on text selected in a herdr-core pane. The pane keeps its own
//! selection, which may reach into its history; the note card is the one a
//! Herdr 1 pane's note uses, laid over the pane in the window. Herdr 2
//! agents do not take notes yet, so a note is copied to the clipboard.

use super::*;

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
        self.open_terminal_note(Source::Core(view), None, place, quote, window, cx);
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
