//! The emulator: alacritty's `Term` and vte's parser behind a small, pure,
//! deterministic API. It never writes to the PTY, opens links, or touches the
//! clipboard: an OSC 52 write comes out as an [`Event`] for the caller to
//! apply. herdr-core answers terminal queries itself.

mod history;

use crate::{
    MAX_SCROLLBACK_CELLS, MAX_SCROLLBACK_LINES, Size, cells,
    damage::{Damage, Tracker},
    graphics::{Found, Graphics, Placed},
    listener::Listener,
    modes::Modes,
    scan::{Run, Scanner},
};
use alacritty_terminal::{
    Term,
    grid::{self, Dimensions},
    index::{Column, Line},
    term::{
        Config, Osc52, TermMode,
        cell::{Cell, Flags},
    },
    vte::ansi::{CursorShape, CursorStyle, Processor, Timeout},
};
use sheprd_backend::ScrollMetrics;
use sheprd_protocol::{ClientPaneInputEvent, FrameData, SurfaceGraphicsScene};
use std::{fmt, time::Duration};

/// Something the program did that is not on screen. Taking events coalesces
/// them: at most one title change, one clipboard write, and one bell count
/// per take.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// The bell rang `count` times.
    Bell { count: u32 },
    /// The program set the clipboard with OSC 52: UTF-8 text of at most
    /// [`crate::MAX_CLIPBOARD_BYTES`]. Reading the clipboard is never
    /// allowed.
    Clipboard(String),
    /// The title changed; `None` is a reset to the default.
    Title(Option<String>),
}

/// A move of the view through history.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scroll {
    /// Positive lines move up into history, negative back down.
    Lines(i32),
    PageUp,
    PageDown,
    Top,
    Bottom,
}

impl From<Scroll> for grid::Scroll {
    fn from(scroll: Scroll) -> Self {
        match scroll {
            Scroll::Lines(lines) => Self::Delta(lines),
            Scroll::PageUp => Self::PageUp,
            Scroll::PageDown => Self::PageDown,
            Scroll::Top => Self::Top,
            Scroll::Bottom => Self::Bottom,
        }
    }
}

/// Synchronized updates (DECSET 2026) without a clock: the parser buffers
/// until the program ends the update, vte's 2 MiB cap, or the caller ends it
/// with [`Emulator::end_sync`], so time never changes what a feed does.
#[derive(Debug, Default)]
struct SyncPending(bool);

impl Timeout for SyncPending {
    fn set_timeout(&mut self, _: Duration) {
        self.0 = true;
    }

    fn clear_timeout(&mut self) {
        self.0 = false;
    }

    fn pending_timeout(&self) -> bool {
        self.0
    }
}

struct Dims(Size);

impl Dimensions for Dims {
    fn total_lines(&self) -> usize {
        self.screen_lines()
    }

    fn screen_lines(&self) -> usize {
        usize::from(self.0.rows())
    }

    fn columns(&self) -> usize {
        usize::from(self.0.cols())
    }
}

pub struct Emulator {
    bells: u32,
    clipboard: Option<String>,
    damage: Tracker,
    graphics: Graphics,
    listener: Listener,
    processor: Processor<SyncPending>,
    /// Counts changes to the text, which history coordinates hold for.
    revision: u64,
    scanner: Scanner,
    scrollback_lines: usize,
    size: Size,
    term: Term<Listener>,
    title: Option<String>,
    title_changed: bool,
}

impl fmt::Debug for Emulator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Emulator")
            .field("size", &self.size)
            .field("title", &self.title)
            .finish_non_exhaustive()
    }
}

impl Emulator {
    /// A blank terminal keeping up to `scrollback_lines` of history, clamped
    /// to [`MAX_SCROLLBACK_LINES`] and [`MAX_SCROLLBACK_CELLS`].
    pub fn new(size: Size, scrollback_lines: usize) -> Self {
        let listener = Listener::default();
        let config = config(size, scrollback_lines);
        let term = Term::new(config, &Dims(size), listener.clone());
        Self {
            bells: 0,
            clipboard: None,
            damage: Tracker::default(),
            graphics: Graphics::default(),
            listener,
            processor: Processor::new(),
            revision: 0,
            scanner: Scanner::default(),
            scrollback_lines,
            size,
            term,
            title: None,
            title_changed: false,
        }
    }

    /// Runs PTY output through the terminal. How the bytes are split across
    /// calls never changes the result.
    pub fn feed(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        let Self {
            graphics,
            processor,
            scanner,
            term,
            ..
        } = self;
        scanner.feed(bytes, |run| match run {
            Run::Bytes(bytes) => processor.advance(term, bytes),
            Run::Graphics(command) => {
                // An image goes where the cursor is after what came before
                // it, so output a synchronized update holds lands first.
                if processor.sync_timeout().pending_timeout() {
                    processor.stop_sync(term);
                }
                if let Some(placed) = graphics.command(command) {
                    place(term, processor, graphics, placed);
                }
            }
        });
        self.damage.fed = true;
        self.revision = self.revision.wrapping_add(1);
        self.drain_events();
    }

    /// Whether a synchronized update is holding back output.
    pub fn sync_pending(&self) -> bool {
        self.processor.sync_timeout().pending_timeout()
    }

    /// Applies a synchronized update the program has not ended, for a caller
    /// whose own timer ran out (terminals use about 150 ms).
    pub fn end_sync(&mut self) {
        if !self.sync_pending() {
            return;
        }
        self.processor.stop_sync(&mut self.term);
        self.damage.fed = true;
        self.revision = self.revision.wrapping_add(1);
        self.drain_events();
    }

    pub fn size(&self) -> Size {
        self.size
    }

    pub fn resize(&mut self, size: Size) {
        if size == self.size {
            return;
        }
        self.size = size;
        self.term.resize(Dims(size));
        self.term.set_options(config(size, self.scrollback_lines));
        self.damage.full = true;
        self.revision = self.revision.wrapping_add(1);
    }

    /// How many times the text changed: output, a resize's rewrap, or a
    /// clear. History coordinates read at one revision hold until the next.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// The visible viewport, scrolled as [`Self::scroll`] left it. It does not
    /// reset damage: [`Damage::Full`] carries the frame later rows patch.
    pub fn frame(&self) -> FrameData {
        cells::frame(&self.term, &mut Vec::new())
    }

    /// What changed since the previous take; the first take is always full.
    pub fn take_damage(&mut self) -> Damage {
        let alt_screen = self.term.mode().contains(TermMode::ALT_SCREEN);
        self.damage.take(&mut self.term, alt_screen)
    }

    /// Whether each visible row's line goes on to the next row: the
    /// terminal wrapped it, rather than the program ending it at the edge.
    /// Frames cannot say so, and text read across rows needs it.
    pub fn wrapped_rows(&self) -> Vec<bool> {
        cells::wrapped(&self.term)
    }

    pub fn modes(&self) -> Modes {
        Modes::new(*self.term.mode(), self.scanner.win32_input())
    }

    /// The PTY bytes for `input` under the current modes.
    pub fn encode(&self, input: &ClientPaneInputEvent) -> Vec<u8> {
        crate::input::encode(input, &self.modes())
    }

    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    pub fn scroll(&mut self, scroll: Scroll) {
        let before = self.term.grid().display_offset();
        self.term.scroll_display(scroll.into());
        if self.term.grid().display_offset() != before {
            self.damage.full = true;
        }
    }

    pub fn scroll_metrics(&self) -> ScrollMetrics {
        let grid = self.term.grid();
        ScrollMetrics {
            offset: grid.display_offset(),
            history: grid.history_size(),
            rows: self.size.rows(),
            full: grid.history_size() >= history_limit(self.size, self.scrollback_lines),
        }
    }

    /// An emulator for a viewer that never shows images: placements keep
    /// moving the cursor as the program expects, and no pixels are kept.
    pub fn without_images(mut self) -> Self {
        self.graphics.drop_pixels();
        self
    }

    /// The cell size in pixels programs are told, which sizes an image
    /// placed without a size in cells.
    pub fn set_cell_pixels(&mut self, width: u32, height: u32) {
        self.graphics.set_cell(width, height);
    }

    /// The images on screen, when they changed since the last take: kitty
    /// graphics placements as Herdr's graphics scene, with the bytes of each
    /// image the last scene did not show.
    pub fn take_graphics(&mut self) -> Option<SurfaceGraphicsScene> {
        if !self.graphics.active() {
            return None;
        }
        let found = self.markers();
        self.graphics.scene(&found)
    }

    /// The zero-width characters of the cell at `column`, `row` on screen.
    #[cfg(test)]
    pub(crate) fn zerowidth_at(&self, column: usize, row: i32) -> Vec<char> {
        self.term.grid()[Line(row)][Column(column)]
            .zerowidth()
            .map(<[char]>::to_vec)
            .unwrap_or_default()
    }

    /// The placement markers on screen and in the rows above it that a
    /// placement could still reach down from. Markers of placements that
    /// were replaced or deleted are swept out of their cells on the way, so
    /// what a program redraws in place never piles up.
    fn markers(&mut self) -> Vec<Found> {
        let graphics = &self.graphics;
        let grid = self.term.grid_mut();
        let offset = i64::try_from(grid.display_offset()).unwrap_or(0);
        let history = i64::try_from(grid.history_size()).unwrap_or(0);
        let above = i64::try_from(self.graphics.tallest()).unwrap_or(0);
        let rows = i64::try_from(grid.screen_lines()).unwrap_or(0);
        let first = (-above).max(offset - history);
        let mut found = Vec::new();
        for row in first..rows {
            let Ok(line) = i32::try_from(row - offset) else {
                continue;
            };
            let columns = grid.columns();
            let cells = &mut grid[Line(line)];
            for column in 0..columns {
                for marker in sweep(&mut cells[Column(column)], |marker| graphics.placed(marker)) {
                    found.push(Found {
                        marker,
                        row,
                        column: u16::try_from(column).unwrap_or(u16::MAX),
                    });
                }
            }
        }
        found
    }

    /// How many rows each line takes, from the oldest in history down to
    /// the screen's last row: a line wrapped over three rows counts 3.
    /// Comparing it across a resize tells where rewrapped rows went.
    pub fn line_rows(&self) -> Vec<u16> {
        let grid = self.term.grid();
        let Some(last) = grid.columns().checked_sub(1) else {
            return Vec::new();
        };
        let history = i32::try_from(grid.history_size()).unwrap_or(i32::MAX);
        let rows = i32::try_from(grid.screen_lines()).unwrap_or(i32::MAX);
        let mut lines = Vec::new();
        let mut current = 0u16;
        for line in -history..rows {
            current = current.saturating_add(1);
            if !grid[Line(line)][Column(last)]
                .flags
                .contains(Flags::WRAPLINE)
            {
                lines.push(current);
                current = 0;
            }
        }
        if current > 0 {
            lines.push(current);
        }
        lines
    }

    pub fn take_events(&mut self) -> Vec<Event> {
        let mut events = Vec::new();
        if std::mem::take(&mut self.title_changed) {
            events.push(Event::Title(self.title.clone()));
        }
        if let Some(text) = self.clipboard.take() {
            events.push(Event::Clipboard(text));
        }
        let count = std::mem::take(&mut self.bells);
        if count > 0 {
            events.push(Event::Bell { count });
        }
        events
    }

    fn drain_events(&mut self) {
        let inbox = self.listener.take();
        if let Some(title) = inbox.title {
            self.title_changed |= title != self.title;
            self.title = title;
        }
        self.bells = self.bells.saturating_add(inbox.bells);
        if inbox.clipboard.is_some() {
            self.clipboard = inbox.clipboard;
        }
    }
}

/// Marks the cursor's cell with a placement and, unless the command kept
/// it, moves the cursor as Ghostty does: down to the image's last row,
/// scrolling as a line feed would, and to the column after it. The rows
/// below the cursor are walked and the rest scrolled a screen at a time, so
/// a tall image costs a few steps rather than one per row.
fn place(
    term: &mut Term<Listener>,
    processor: &mut Processor<SyncPending>,
    graphics: &Graphics,
    placed: Placed,
) {
    let point = term.grid().cursor.point;
    let cell = &mut term.grid_mut()[point];
    // A placement redrawn in place replaces the one it was: its marker goes.
    sweep(cell, |marker| graphics.placed(marker));
    for c in placed.marker {
        cell.push_zerowidth(c);
    }
    if !placed.moves_cursor {
        return;
    }
    // Rows and columns are at most `graphics::MAX_SIDE`, within a CSI
    // parameter.
    let rows = placed.rows.saturating_sub(1);
    let screen = u32::try_from(term.screen_lines()).unwrap_or(u32::MAX);
    let down = rows.min(screen);
    if down > 0 {
        // Cursor down stops at the bottom margin, as an index would before
        // it scrolls.
        processor.advance(term, format!("\x1b[{down}B").as_bytes());
    }
    let moved = term.grid().cursor.point.line.0 - point.line.0;
    // A scroll moves at most a screen at once, so the rest goes a screen
    // at a time: a few steps even for the tallest image.
    let mut scroll = rows.saturating_sub(u32::try_from(moved).unwrap_or(0));
    while scroll > 0 {
        let step = scroll.min(screen.max(1));
        processor.advance(term, format!("\x1b[{step}S").as_bytes());
        scroll -= step;
    }
    let column = u64::try_from(point.column.0)
        .unwrap_or(u64::MAX)
        .saturating_add(u64::from(placed.cols))
        .saturating_add(1);
    processor.advance(term, format!("\x1b[{column}G").as_bytes());
}

/// Drops the markers in `cell` whose placement is gone, keeping its other
/// zero-width characters, and returns the markers kept.
fn sweep(cell: &mut Cell, live: impl Fn(u32) -> bool) -> Vec<u32> {
    let Some(chars) = cell.zerowidth() else {
        return Vec::new();
    };
    let markers: Vec<u32> = crate::graphics::markers_in(chars).collect();
    let kept: Vec<u32> = markers
        .iter()
        .copied()
        .filter(|&marker| live(marker))
        .collect();
    if kept.len() == markers.len() {
        return kept;
    }
    let others: Vec<char> = chars
        .iter()
        .copied()
        .filter(|&c| !crate::graphics::is_marker(c))
        .collect();
    // alacritty empties a cell's zero-width characters only with its wide
    // flag; the flag goes back and the characters that stay are pushed anew.
    let wide = cell.flags.contains(Flags::WIDE_CHAR);
    cell.clear_wide();
    if wide {
        cell.flags.insert(Flags::WIDE_CHAR);
    }
    for c in others {
        cell.push_zerowidth(c);
    }
    for c in kept
        .iter()
        .filter_map(|&marker| crate::graphics::marker_chars(marker))
        .flatten()
    {
        cell.push_zerowidth(c);
    }
    kept
}

/// The history lines a terminal `size` keeps when asked for
/// `scrollback_lines`.
fn history_limit(size: Size, scrollback_lines: usize) -> usize {
    let cell_limit = MAX_SCROLLBACK_CELLS / usize::from(size.cols());
    scrollback_lines.min(MAX_SCROLLBACK_LINES).min(cell_limit)
}

fn config(size: Size, scrollback_lines: usize) -> Config {
    Config {
        scrolling_history: history_limit(size, scrollback_lines),
        // Stands for "no program chose a style" (see `cells::cursor`).
        default_cursor_style: CursorStyle {
            shape: CursorShape::HollowBlock,
            blinking: false,
        },
        kitty_keyboard: true,
        osc52: Osc52::OnlyCopy,
        semantic_escape_chars: history::WORD_SEPARATORS.to_owned(),
        ..Config::default()
    }
}
