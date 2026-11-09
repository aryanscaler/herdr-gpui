//! The selected text a note quotes, and where the note sits. The text is
//! read from the history, shown or not, as a copy reads it.

use super::CoreTerminalView;
use crate::core_terminals::{Error, selection::Selection};
use gpui::{Bounds, Context, Pixels, point, px, size};

impl CoreTerminalView {
    /// Starts reading the selected text, for a note about it; false when
    /// nothing is selected. [`Self::take_quote`] has it once it arrives; a
    /// quote asked for earlier is dropped.
    pub(crate) fn quote_selection(&mut self) -> bool {
        self.quote_ready = None;
        let Some(range) = self.noted().and_then(|selection| selection.range()) else {
            self.quoting = None;
            return false;
        };
        self.quoting = Some(self.read(range));
        true
    }

    /// What a note quotes: the cells copy mode marked while it is on, else
    /// the pointer's selection.
    fn noted(&self) -> Option<Selection> {
        match self.copy_mode_active() {
            true => self.copy_mode_marked().map(Selection::of),
            false => self.selection.filter(|selection| !selection.is_empty()),
        }
    }

    /// The text [`Self::quote_selection`] read from the history, once.
    pub(crate) fn take_quote(&mut self) -> Option<Result<String, Error>> {
        self.quote_ready.take()
    }

    pub(crate) fn clear_selection(&mut self, cx: &mut Context<Self>) {
        if self.selection.take().is_some() {
            cx.notify();
        }
    }

    /// The last selected row the screen shows, in the window, for a note to
    /// sit beside.
    pub(crate) fn selection_anchor(&self) -> Option<Bounds<Pixels>> {
        let bounds = self.bounds?;
        let frame = self.screen.frame.as_ref()?;
        let (row, columns) = self
            .noted()?
            .visible_rows(frame.width, self.top(), frame.height)
            .pop()?;
        let (width, height) = (self.look.cell_width, self.look.cell_height);
        Some(Bounds::new(
            bounds.origin
                + point(
                    px(f32::from(columns.start) * width),
                    px(f32::from(row) * height),
                ),
            size(
                px(f32::from(columns.end.saturating_sub(columns.start)) * width),
                px(height),
            ),
        ))
    }
}
