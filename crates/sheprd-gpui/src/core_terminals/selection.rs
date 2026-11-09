//! Text selected in a herdr-core pane: a drag from one cell to another, a
//! double-clicked word, or a triple-clicked line. Its ends are history
//! points, rows counted from the top of the retained history, so a
//! selection keeps its text as the view scrolls and may reach past the
//! screen. Pure: cells in, ranges and text out.

use sheprd_backend::{FrameData, TextPoint, TextRange};
use std::ops::Range;

/// A cell of the visible screen, column and row from the top left.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Cell {
    pub(crate) row: u16,
    pub(crate) column: u16,
}

impl Cell {
    /// This cell as a history point, for a screen whose top row is `top`.
    pub(crate) fn point(self, top: u32) -> TextPoint {
        TextPoint {
            row: top.saturating_add(u32::from(self.row)),
            col: self.column,
        }
    }
}

/// From where a press started to where the pointer is, either way round.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Selection {
    anchor: TextPoint,
    head: TextPoint,
    /// A double- or triple-clicked word or line, which selects its cells
    /// even when it is one cell wide.
    picked: bool,
}

impl Selection {
    /// A press at `at`, selecting nothing until the pointer moves.
    pub(crate) fn at(at: TextPoint) -> Self {
        Self {
            anchor: at,
            head: at,
            picked: false,
        }
    }

    /// The word under `at`, on a screen whose top row is `top`: the run of
    /// non-blank cells around it.
    pub(crate) fn word(frame: &FrameData, top: u32, at: Cell) -> Self {
        let blank = |column: u16| symbol(frame, at.row, column).trim().is_empty();
        if blank(at.column) {
            return Self::at(at.point(top));
        }
        let start = (0..at.column)
            .rev()
            .take_while(|column| !blank(*column))
            .last()
            .unwrap_or(at.column);
        let end = (at.column..frame.width)
            .take_while(|column| !blank(*column))
            .last()
            .unwrap_or(at.column);
        Self {
            anchor: Cell {
                column: start,
                ..at
            }
            .point(top),
            head: Cell { column: end, ..at }.point(top),
            picked: true,
        }
    }

    /// The whole row under `at`, on a screen whose top row is `top`.
    pub(crate) fn line(frame: &FrameData, top: u32, at: Cell) -> Self {
        Self {
            anchor: Cell { column: 0, ..at }.point(top),
            head: Cell {
                column: frame.width.saturating_sub(1),
                ..at
            }
            .point(top),
            picked: true,
        }
    }

    /// The cells of `range`, as copy mode marks them.
    pub(crate) fn of(range: TextRange) -> Self {
        Self {
            anchor: range.start,
            head: range.end,
            picked: true,
        }
    }

    pub(crate) fn extend(&mut self, to: TextPoint) {
        self.head = to;
    }

    /// Whether this is a press whose pointer never left the cell it
    /// pressed, rather than a word or line, however short.
    pub(crate) fn is_empty(&self) -> bool {
        !self.picked && self.anchor == self.head
    }

    /// The selected cells, first to last, or `None` when nothing is.
    pub(crate) fn range(&self) -> Option<TextRange> {
        if self.is_empty() {
            return None;
        }
        let (start, end) = if self.anchor <= self.head {
            (self.anchor, self.head)
        } else {
            (self.head, self.anchor)
        };
        Some(TextRange { start, end })
    }

    /// Each selected row's columns, as a terminal selects: to the end of
    /// the first row, whole rows between, and the last row up to the head.
    pub(crate) fn rows(&self, width: u16) -> Vec<(u32, Range<u16>)> {
        let Some(TextRange { start, end }) = self.range().filter(|_| width > 0) else {
            return Vec::new();
        };
        (start.row..=end.row)
            .map(|row| {
                let from = if row == start.row { start.col } else { 0 };
                let to = if row == end.row {
                    end.col.saturating_add(1).min(width)
                } else {
                    width
                };
                (row, from.min(to)..to)
            })
            .collect()
    }

    /// The selected rows a screen `height` rows tall, whose top row is
    /// `top`, shows, as rows of that screen.
    pub(crate) fn visible_rows(&self, width: u16, top: u32, height: u16) -> Vec<(u16, Range<u16>)> {
        self.rows(width)
            .into_iter()
            .filter_map(|(row, columns)| {
                let row = u16::try_from(row.checked_sub(top)?).ok()?;
                (row < height).then_some((row, columns))
            })
            .collect()
    }
}

/// What a cell shows. A wide character's second cell shows nothing, so
/// the character is read once, and a concealed cell reads as the blank
/// it is drawn as, never its hidden text.
pub(super) fn symbol(frame: &FrameData, row: u16, column: u16) -> &str {
    let index = usize::from(row) * usize::from(frame.width) + usize::from(column);
    frame
        .cells
        .get(index)
        .filter(|cell| !cell.skip)
        .map_or("", |cell| {
            if cell.modifier & crate::terminal::HIDDEN != 0 {
                " "
            } else {
                cell.symbol.as_str()
            }
        })
}
