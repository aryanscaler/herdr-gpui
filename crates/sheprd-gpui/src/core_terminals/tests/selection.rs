use crate::core_terminals::selection::{Cell, Selection, symbol};
use sheprd_backend::FrameData;
use sheprd_backend::TextPoint;
use sheprd_client::protocol::CellData;

/// The selected text as the screen shows it, read from `frame` whose top
/// row is `top`: each row's cells with trailing blanks dropped, rows joined
/// by newlines. `None` when part of the selection is off the screen.
trait Shown {
    fn text(&self, frame: &FrameData, top: u32) -> Option<String>;
}

impl Shown for Selection {
    fn text(&self, frame: &FrameData, top: u32) -> Option<String> {
        let rows = self.rows(frame.width);
        let visible = self.visible_rows(frame.width, top, frame.height);
        if visible.len() != rows.len() {
            return None;
        }
        Some(
            visible
                .into_iter()
                .map(|(row, columns)| {
                    let text: String = columns.map(|column| symbol(frame, row, column)).collect();
                    text.trim_end().to_owned()
                })
                .collect::<Vec<_>>()
                .join("\n"),
        )
    }
}

/// A frame showing `rows`, each padded with blanks to `width`.
fn frame(width: u16, rows: &[&str]) -> FrameData {
    let cells = rows
        .iter()
        .flat_map(|row| {
            let mut symbols: Vec<String> = row.chars().map(String::from).collect();
            symbols.resize(usize::from(width), " ".into());
            symbols
        })
        .map(|symbol| CellData {
            symbol,
            fg: 0,
            bg: 0,
            modifier: 0,
            skip: false,
            hyperlink: None,
        })
        .collect();
    FrameData {
        cells,
        width,
        height: u16::try_from(rows.len()).unwrap(),
        cursor: None,
        hyperlinks: Vec::new(),
        graphics: Vec::new(),
    }
}

fn at(row: u16, column: u16) -> Cell {
    Cell { row, column }
}

fn point(row: u16, column: u16) -> TextPoint {
    at(row, column).point(0)
}

#[test]
fn a_press_that_never_moves_selects_nothing() {
    let shown = frame(8, &["hello"]);
    let selection = Selection::at(point(0, 2));
    assert!(selection.is_empty());
    assert!(selection.rows(8).is_empty());
    assert_eq!(selection.text(&shown, 0).unwrap(), "");
}

#[test]
fn a_drag_selects_to_the_ends_of_rows_between_and_reads_either_way() {
    let shown = frame(8, &["ab cd", "efgh", "ij kl"]);
    let mut forward = Selection::at(point(0, 3));
    forward.extend(point(2, 1));
    assert_eq!(forward.rows(8), [(0, 3..8), (1, 0..8), (2, 0..2)]);
    assert_eq!(forward.text(&shown, 0).unwrap(), "cd\nefgh\nij");
    let mut backward = Selection::at(point(2, 1));
    backward.extend(point(0, 3));
    assert_eq!(
        backward.text(&shown, 0).unwrap(),
        forward.text(&shown, 0).unwrap()
    );
}

#[test]
fn double_and_triple_clicks_take_a_word_and_a_line() {
    let shown = frame(16, &["git push origin"]);
    assert_eq!(
        Selection::word(&shown, 0, at(0, 6))
            .text(&shown, 0)
            .unwrap(),
        "push"
    );
    assert_eq!(
        Selection::word(&shown, 0, at(0, 0))
            .text(&shown, 0)
            .unwrap(),
        "git"
    );
    assert!(Selection::word(&shown, 0, at(0, 3)).is_empty());
    assert_eq!(
        Selection::line(&shown, 0, at(0, 6))
            .text(&shown, 0)
            .unwrap(),
        "git push origin"
    );
}

#[test]
fn a_wide_character_is_copied_once() {
    // The second cell stands in for the wide character's right half.
    let mut shown = frame(4, &["界x a"]);
    shown.cells[1].skip = true;
    shown.cells[1].symbol = String::new();
    let mut selection = Selection::at(point(0, 0));
    selection.extend(point(0, 3));
    assert_eq!(selection.text(&shown, 0).unwrap(), "界 a");
}

#[test]
fn a_one_letter_word_or_line_is_still_selected() {
    let shown = frame(3, &["x y"]);
    let word = Selection::word(&shown, 0, at(0, 0));
    assert!(!word.is_empty());
    assert_eq!(word.text(&shown, 0).unwrap(), "x");
    let narrow = frame(1, &["z"]);
    assert_eq!(
        Selection::line(&narrow, 0, at(0, 0))
            .text(&narrow, 0)
            .unwrap(),
        "z"
    );
}

#[test]
fn a_selection_keeps_its_history_rows_as_the_view_scrolls() {
    // Rows 10 and 11 of the history, on a screen whose top row is 10.
    let shown = frame(6, &["one", "two"]);
    let mut selection = Selection::at(at(0, 0).point(10));
    selection.extend(at(1, 2).point(10));
    assert_eq!(selection.visible_rows(6, 10, 2), [(0, 0..6), (1, 0..3)]);
    assert_eq!(selection.text(&shown, 10).unwrap(), "one\ntwo");
    // Scrolled down a row, only the second row shows, as the top row.
    assert_eq!(selection.visible_rows(6, 11, 2), [(0, 0..3)]);
    // Part of it is off the screen: only the history can read it.
    assert_eq!(selection.text(&shown, 11), None);
    assert_eq!(
        selection
            .range()
            .map(|range| (range.start.row, range.end.row)),
        Some((10, 11))
    );
}

#[test]
fn concealed_text_is_copied_as_the_blanks_it_shows() {
    let mut shown = frame(12, &["pw: secret o"]);
    for cell in &mut shown.cells[4..10] {
        cell.modifier |= crate::terminal::HIDDEN;
    }
    let mut selection = Selection::at(point(0, 0));
    selection.extend(point(0, 11));
    assert_eq!(selection.text(&shown, 0).unwrap(), "pw:        o");
    // A selection of hidden cells alone copies nothing.
    let mut hidden = Selection::at(point(0, 4));
    hidden.extend(point(0, 9));
    assert_eq!(hidden.text(&shown, 0).unwrap(), "");
}
