//! A terminal's history: the points and ranges of its text, and what a view
//! may ask of the lines it keeps. Herdr 1's daemon answers the same questions
//! through its API (`sheprd_client::scrollback` re-exports these types with
//! its wire spellings); a herdr-core view answers them from its own emulator.
//!
//! Rows are absolute, counted from the top of the retained history, so a
//! point stays put while the view scrolls. The viewport's top row is
//! `history - offset`.

use serde::{Deserialize, Serialize};

/// The longest search query either backend accepts, in bytes.
pub const MAX_SEARCH_QUERY_BYTES: usize = 4096;
/// The most matches one search answers, around its current match.
pub const MAX_SEARCH_MATCHES: usize = 1024;

/// A cell of the history. Declared row first so the derived order is
/// reading order.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
pub struct TextPoint {
    pub row: u32,
    pub col: u16,
}

/// A span of cells. `end` is inclusive: it names the last cell of the match,
/// which for a wide glyph is its second column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TextRange {
    pub start: TextPoint,
    pub end: TextPoint,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchDirection {
    /// Toward newer output: the first match starting after the origin.
    Forward,
    /// Toward older output: the last match ending before the origin.
    Backward,
}

/// A copy-mode motion resolved against the terminal's own text: word
/// classes, line ends, and paragraphs, which the painted cells alone cannot
/// tell. Spellings match Herdr's `PaneCopyMotion`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CopyMotion {
    LineEnd,
    FirstNonBlank,
    NextWordStart,
    PreviousWordStart,
    NextWordEnd,
    NextBigWordStart,
    PreviousBigWordStart,
    NextBigWordEnd,
    PreviousParagraph,
    NextParagraph,
}

/// Where a view is in its history.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ScrollMetrics {
    /// Lines the view is scrolled up from the live screen; 0 is live.
    pub offset: usize,
    /// History lines above the live screen. An alternate screen keeps none.
    pub history: usize,
    /// Rows the view shows.
    pub rows: u16,
    /// Whether the history holds all it keeps, so each new line drops the
    /// oldest and every row counted from the top moves up one.
    pub full: bool,
}

/// A search of the history for `query`, matched as text: case-insensitive
/// unless it has a capital letter. The origin is `previous` when given (its
/// end searching forward, its start backward), otherwise `cursor`; a search
/// finding nothing past its origin wraps around.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Search {
    pub query: String,
    pub direction: SearchDirection,
    pub cursor: TextPoint,
    pub previous: Option<TextRange>,
}

/// What a search found: at most [`MAX_SEARCH_MATCHES`] matches around the
/// current one, and how many the whole history holds.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Matches {
    pub matches: Vec<TextRange>,
    /// Every match in the history.
    pub total: u64,
    /// The current match's index within `matches`.
    pub current: Option<u32>,
    /// The current match's index among all `total`, top first.
    pub current_global: Option<u64>,
    /// The text revision searched, which the matches' points hold for.
    pub revision: u64,
}

/// A question about, or a change to, the history an attached view keeps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum History {
    Search(Search),
    /// Where `motion` from `cursor` lands.
    Motion {
        cursor: TextPoint,
        motion: CopyMotion,
    },
    /// The text from `anchor` to `cursor`, both inclusive, in either order.
    Read {
        anchor: TextPoint,
        cursor: TextPoint,
    },
    /// The whole history and screen as text, wrapped lines joined.
    Text,
    /// Scrolls the view to `offset` lines above the live screen.
    ScrollTo {
        offset: usize,
    },
    /// Drops the history and the screen above the cursor's line, as a
    /// terminal's clear does; an alternate screen is left alone. Nothing
    /// reaches the program.
    Clear,
}

/// The answer to a [`History`] request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HistoryAnswer {
    Matches(Matches),
    Point(TextPoint),
    Text(String),
    /// A scroll or clear was done.
    Done,
}
