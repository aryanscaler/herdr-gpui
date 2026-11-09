//! The view's questions to the history its emulator keeps, asked through
//! the worker and answered in its mailbox: searches for the find bar, the
//! text an editor opens, scrolling to a line, and clearing.

use super::CoreTerminalView;
pub(super) use crate::core_terminals::worker::Answer;
use crate::core_terminals::{Error, selection::Selection, worker::Command};
use gpui::Context;
use sheprd_backend::{History, HistoryAnswer, Matches, TextRange};
use sheprd_client::{
    protocol::{PaneSurfacePane, PaneSurfaceScrollMetrics, SurfaceRect},
    scrollback::{
        CopyMotionResult, CopySearchResult, EndpointErrorCode, ScrollbackResponse, SelectionResult,
    },
};

/// A read of the text between two points, and the text revision the view
/// showed when it asked, or the one a search match was found at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Reading {
    pub(super) id: u64,
    revision: u64,
    /// The text must be that of `revision`, as a search match's is: output
    /// may have rewritten it in place, even with no row moved.
    exact: bool,
    /// The screen's columns and rows when asked; a resize since rewrapped
    /// the rows the read named.
    grid: Option<(u16, u16)>,
    /// The history lines above the screen when asked; fewer since means a
    /// clear dropped lines the rows were counted from.
    history: usize,
    /// The selection this copies, said and settled once its text reaches
    /// the clipboard; none for copy mode's copies, which say nothing.
    announce: Option<Selection>,
}

impl CoreTerminalView {
    /// Reads the text of `range` from the history.
    pub(super) fn read(&mut self, range: TextRange) -> Reading {
        let revision = self.screen.revision;
        self.read_at(range, revision, false)
    }

    /// Reads the text of `range`, as it was at the text revision `revision`
    /// when `exact`, else wherever its rows still hold the same lines.
    pub(super) fn read_at(&mut self, range: TextRange, revision: u64, exact: bool) -> Reading {
        Reading {
            id: self.ask(History::Read {
                anchor: range.start,
                cursor: range.end,
            }),
            revision,
            exact,
            grid: self.screen.grid(),
            history: self.screen.history.history,
            announce: None,
        }
    }

    /// Whether the rows `reading` named may now hold other text: any output
    /// since an exact read's revision, or, since it was asked, output into
    /// a full history, which dropped its oldest lines, a clear, which
    /// dropped them all, or a resize, which rewrapped them. The worker shows each output's and resize's screen
    /// before it answers anything after it, and the answers are taken with
    /// the screen, so the screen here is never older than the read saw.
    fn moved(&self, reading: Reading) -> bool {
        self.screen.revision != reading.revision
            && (reading.exact
                || self.screen.history.full
                || self.screen.history.history < reading.history
                || self.screen.grid() != reading.grid)
    }

    /// Says the copy waiting on its text once it lands, rather than when
    /// it is asked for: the read may still be refused or fail.
    pub(super) fn announce_copy(&mut self) {
        if let Some(reading) = &mut self.copying {
            reading.announce = self.selection;
        }
    }

    /// Whether a copy waits on its text.
    #[cfg(test)]
    pub(crate) fn copy_pending(&self) -> bool {
        self.copying.is_some()
    }

    /// Sends `request` to the worker; its answer comes back under the ID.
    pub(super) fn ask(&mut self, request: History) -> u64 {
        self.next_request = self.next_request.wrapping_add(1);
        let id = self.next_request;
        self.link.send(Command::History { id, request });
        id
    }

    /// Hands each answer to what asked for it. Scrolls and clears answer
    /// only that they were done.
    pub(super) fn take_answers(&mut self, answers: Vec<(u64, Answer)>, cx: &mut Context<Self>) {
        for (id, answer) in answers {
            if self.awaits_find(id) {
                self.find_answered(id, answer);
                cx.notify();
            } else if self.awaits_copy_mode(id) {
                self.copy_mode_answered(id, answer);
                cx.notify();
            } else if let Some(reading) = self.copying.take_if(|reading| reading.id == id) {
                match answer {
                    // Copying other text than was selected is worse than
                    // copying nothing.
                    Ok(HistoryAnswer::Text(_)) if self.moved(reading) => {
                        tracing::info!("A herdr-core selection moved before it was copied");
                    }
                    Ok(HistoryAnswer::Text(text)) => {
                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
                        if let Some(selection) = reading.announce {
                            self.copied = Some(selection);
                        }
                    }
                    Ok(_) => tracing::warn!("A herdr-core history read answered no text"),
                    Err(error) => tracing::warn!(%error, "Could not read a herdr-core history"),
                }
            } else if let Some(reading) = self.quoting.take_if(|reading| reading.id == id) {
                self.quote_ready = Some(match answer {
                    // A note must quote the text that was selected.
                    Ok(HistoryAnswer::Text(_)) if self.moved(reading) => Err(Error::Moved),
                    Ok(HistoryAnswer::Text(text)) => Ok(text),
                    Ok(_) => Err(Error::Answer),
                    Err(error) => Err(Error::History(error)),
                });
            } else if self.editing == Some(id) {
                self.editing = None;
                self.edit_ready = Some(match answer {
                    Ok(HistoryAnswer::Text(text)) => Ok(text),
                    Ok(_) => Err(Error::Answer),
                    Err(error) => Err(Error::History(error)),
                });
            }
        }
    }

    /// The view as the shared find and copy mode read a pane: its own grid
    /// from the top left, its place in its history, and its text revision,
    /// doubled because those count an odd revision as a write in progress.
    pub(super) fn surface_pane(&self) -> Option<PaneSurfacePane> {
        let frame = self.screen.frame.as_ref()?;
        let history = self.screen.history;
        let rect = SurfaceRect {
            x: 0,
            y: 0,
            width: frame.width,
            height: frame.height,
        };
        Some(PaneSurfacePane {
            pane_id: self.id.to_string(),
            content_revision: self.screen.revision.wrapping_mul(2),
            rect,
            inner_rect: rect,
            scrollbar_rect: None,
            scroll: Some(PaneSurfaceScrollMetrics {
                offset_from_bottom: u64::try_from(history.offset).unwrap_or(u64::MAX),
                max_offset_from_bottom: u64::try_from(history.history).unwrap_or(u64::MAX),
                viewport_rows: u64::from(frame.height),
            }),
            focused: true,
            mouse_reporting: self.screen.mouse_reporting,
            sgr_pixel_mouse: false,
            alternate_screen_active: false,
            pixel_width: 0,
            pixel_height: 0,
        })
    }

    /// Scrolls the view to `offset` lines above the live screen.
    pub(super) fn scroll_history(&mut self, offset: usize) {
        self.ask(History::ScrollTo { offset });
    }

    /// Clears this view's history and the screen above the prompt, as the
    /// `clear_pane` command does to a Herdr 1 pane. Only this view changes:
    /// the program and the terminal's other views keep what they show.
    pub(crate) fn clear_history(&mut self, cx: &mut Context<Self>) {
        // What was selected, marked, or being copied names rows the clear
        // drops.
        self.selection = None;
        self.copying = None;
        if let Some(state) = &mut self.copy_mode {
            state.forget_mark();
        }
        self.ask(History::Clear);
        cx.notify();
    }

    /// Asks for the whole text to open in an editor; [`Self::take_edit`]
    /// has it once it arrives.
    pub(crate) fn request_edit(&mut self) {
        self.editing = Some(self.ask(History::Text));
    }

    /// The text [`Self::request_edit`] asked for, once.
    pub(crate) fn take_edit(&mut self) -> Option<Result<String, Error>> {
        self.edit_ready.take()
    }
}

/// `answer` as Herdr 1's daemon would have given it for `pane`, so the
/// shared find and copy mode read both generations alike. A failure keeps
/// its message for the user; the emulator refuses nothing as stale.
pub(super) fn scrollback(
    pane: &PaneSurfacePane,
    answer: Answer,
) -> sheprd_client::Result<ScrollbackResponse> {
    let pane_id = pane.pane_id.clone();
    match answer {
        Ok(HistoryAnswer::Matches(Matches {
            matches,
            total,
            current,
            current_global,
            revision,
        })) => Ok(ScrollbackResponse::PaneCopySearch(CopySearchResult {
            pane_id,
            // Doubled as the pane's is: see `CoreTerminalView::surface_pane`.
            content_revision: revision.wrapping_mul(2),
            matches,
            total,
            current,
            current_global,
        })),
        Ok(HistoryAnswer::Point(cursor)) => {
            Ok(ScrollbackResponse::PaneCopyMotion(CopyMotionResult {
                pane_id,
                cursor,
                content_revision: pane.content_revision,
            }))
        }
        Ok(HistoryAnswer::Text(text)) => Ok(ScrollbackResponse::PaneSelection(SelectionResult {
            pane_id,
            text,
        })),
        Ok(HistoryAnswer::Done) => Ok(ScrollbackResponse::Ok {}),
        Err(error) => Err(sheprd_client::Error::Endpoint {
            code: EndpointErrorCode::Other("history".into()),
            message: error.to_string(),
        }),
    }
}
