use super::*;
use sheprd_backend::{
    CopyMotion, History, HistoryAnswer, MAX_SEARCH_MATCHES, Search, SearchDirection, TextPoint,
    TextRange,
};

fn point(row: u32, col: u16) -> TextPoint {
    TextPoint { row, col }
}

fn range(row: u32, start: u16, end: u16) -> TextRange {
    TextRange {
        start: point(row, start),
        end: point(row, end),
    }
}

fn search(query: &str, direction: SearchDirection, cursor: TextPoint) -> Search {
    Search {
        query: query.into(),
        direction,
        cursor,
        previous: None,
    }
}

/// Ten lines on a three-row screen: rows 0..=7 are history.
fn ten_lines() -> Emulator {
    fed(
        20,
        3,
        b"a0\r\nb1 foo\r\nc2\r\nd3 FOO\r\ne4\r\nf5 foo\r\ng6\r\nh7\r\ni8\r\nj9",
    )
}

#[test]
fn a_search_counts_every_match_and_finds_the_nearest_older_one() {
    let emulator = ten_lines();
    let found = emulator
        .search(&search("foo", SearchDirection::Backward, point(9, 0)))
        .unwrap();
    // Lowercase queries ignore case.
    assert_eq!(
        found.matches,
        vec![range(1, 3, 5), range(3, 3, 5), range(5, 3, 5)]
    );
    assert_eq!(found.total, 3);
    assert_eq!((found.current, found.current_global), (Some(2), Some(2)));
    // The matches hold for the text they were found in.
    assert_eq!(found.revision, emulator.revision());
}

#[test]
fn a_capital_letter_makes_the_search_match_case() {
    let emulator = ten_lines();
    let found = emulator
        .search(&search("FOO", SearchDirection::Backward, point(9, 0)))
        .unwrap();
    assert_eq!(found.matches, vec![range(3, 3, 5)]);
}

#[test]
fn stepping_from_a_match_moves_to_its_neighbors_and_wraps() {
    let emulator = ten_lines();
    let step = |direction, previous| {
        let found = emulator
            .search(&Search {
                previous: Some(previous),
                ..search("foo", direction, point(9, 0))
            })
            .unwrap();
        found.current_global
    };
    assert_eq!(step(SearchDirection::Backward, range(5, 3, 5)), Some(1));
    assert_eq!(step(SearchDirection::Forward, range(1, 3, 5)), Some(1));
    // Past either end the search wraps around.
    assert_eq!(step(SearchDirection::Backward, range(1, 3, 5)), Some(2));
    assert_eq!(step(SearchDirection::Forward, range(5, 3, 5)), Some(0));
}

#[test]
fn queries_are_matched_literally_and_bounded() {
    let emulator = fed(20, 3, b"a.c abc (x)");
    let found = emulator
        .search(&search("a.c", SearchDirection::Forward, point(0, 0)))
        .unwrap();
    assert_eq!(found.matches, vec![range(0, 0, 2)]);
    let found = emulator
        .search(&search("(x)", SearchDirection::Forward, point(0, 0)))
        .unwrap();
    assert_eq!(found.total, 1);
    assert_eq!(
        emulator
            .search(&search("", SearchDirection::Forward, point(0, 0)))
            .unwrap()
            .total,
        0
    );
    let long = "x".repeat(sheprd_backend::MAX_SEARCH_QUERY_BYTES + 1);
    assert!(matches!(
        emulator.search(&search(&long, SearchDirection::Forward, point(0, 0))),
        Err(Error::QueryTooLarge)
    ));
}

#[test]
fn many_matches_return_a_window_around_the_current_one() {
    let lines = MAX_SEARCH_MATCHES * 2;
    let mut emulator = Emulator::new(size(10, 3), lines + 10);
    emulator.feed(&"x\r\n".repeat(lines).into_bytes());
    let found = emulator
        .search(&search("x", SearchDirection::Forward, point(10, 0)))
        .unwrap();
    assert_eq!(found.total, lines as u64);
    assert_eq!(found.matches.len(), MAX_SEARCH_MATCHES);
    assert_eq!(found.current_global, Some(11));
    let current = found.matches[found.current.unwrap() as usize];
    assert_eq!(current, range(11, 0, 0));
}

#[test]
fn a_match_across_a_wrapped_row_is_found() {
    let emulator = fed(5, 3, b"abcdefgh");
    let found = emulator
        .search(&search("def", SearchDirection::Forward, point(0, 0)))
        .unwrap();
    assert_eq!(
        found.matches,
        vec![TextRange {
            start: point(0, 3),
            end: point(1, 0)
        }]
    );
}

#[test]
fn motions_follow_words_lines_and_paragraphs_without_scrolling() {
    let mut emulator = fed(30, 4, b"one two.three\r\n\r\nfour   \r\nfive");
    emulator.scroll(Scroll::Lines(0));
    let at = |emulator: &mut Emulator, from, motion| emulator.motion(from, motion);
    assert_eq!(
        at(&mut emulator, point(0, 0), CopyMotion::NextWordStart),
        point(0, 4)
    );
    // A small word stops at punctuation; a big one runs to the blank.
    assert_eq!(
        at(&mut emulator, point(0, 4), CopyMotion::NextWordStart),
        point(0, 7)
    );
    assert_eq!(
        at(&mut emulator, point(0, 4), CopyMotion::NextBigWordStart),
        point(2, 0)
    );
    assert_eq!(
        at(&mut emulator, point(0, 0), CopyMotion::LineEnd),
        point(0, 12)
    );
    assert_eq!(
        at(&mut emulator, point(0, 5), CopyMotion::NextParagraph),
        point(1, 0)
    );
    assert_eq!(emulator.scroll_metrics().offset, 0);
}

#[test]
fn a_motion_leaves_a_scrolled_view_where_it_was() {
    let mut emulator = ten_lines();
    emulator.scroll(Scroll::Lines(2));
    let landed = emulator.motion(point(9, 0), CopyMotion::PreviousParagraph);
    assert_eq!(landed, point(0, 0));
    assert_eq!(emulator.scroll_metrics().offset, 2);
}

#[test]
fn reading_joins_wrapped_rows_and_takes_either_order() {
    let emulator = fed(5, 4, b"abcdefgh\r\nxy");
    assert_eq!(emulator.read(point(0, 2), point(2, 1)), "cdefgh\nxy");
    assert_eq!(emulator.read(point(2, 1), point(0, 2)), "cdefgh\nxy");
}

#[test]
fn the_whole_text_spans_history_and_screen() {
    let emulator = ten_lines();
    let text = emulator.text();
    assert!(text.starts_with("a0\nb1 foo\n"), "{text:?}");
    assert!(text.ends_with("i8\nj9\n"), "{text:?}");
}

#[test]
fn scrolling_to_an_offset_is_held_to_the_history() {
    let mut emulator = ten_lines();
    emulator.scroll_to(3);
    assert_eq!(emulator.scroll_metrics().offset, 3);
    emulator.scroll_to(500);
    assert_eq!(emulator.scroll_metrics().offset, 7);
    emulator.scroll_to(0);
    assert_eq!(emulator.scroll_metrics().offset, 0);
}

#[test]
fn clearing_keeps_the_cursor_line_and_its_wrapped_input() {
    let mut emulator = fed(5, 4, b"old\r\nold\r\nold\r\nold\r\n$ abcdefg");
    emulator.scroll(Scroll::Lines(1));
    let before = emulator.revision();
    assert_eq!(
        emulator.history(&History::Clear).unwrap(),
        HistoryAnswer::Done
    );
    assert!(emulator.revision() > before);
    let metrics = emulator.scroll_metrics();
    assert_eq!((metrics.history, metrics.offset), (0, 0));
    let frame = emulator.frame();
    assert_eq!(line(&frame, 0), "$ abc");
    assert_eq!(line(&frame, 1), "defg");
    assert_eq!(line(&frame, 2), "");
    assert!(!emulator.text().contains("old"));
    // Output goes on after the kept input.
    emulator.feed(b"h");
    assert_eq!(line(&emulator.frame(), 1), "defgh");
}

#[test]
fn clearing_keeps_input_that_wrapped_into_the_history() {
    let mut emulator = fed(5, 3, b"old\r\nold\r\nold\r\n$ abcdefghijklmnopq");
    emulator.clear();
    // The input's first row stays in the history; the older lines go.
    assert_eq!(emulator.scroll_metrics().history, 1);
    assert_eq!(emulator.text().trim_end(), "$ abcdefghijklmnopq");
    // The history grows again as output scrolls.
    emulator.feed(b"\r\nr\r\ns\r\nt");
    assert_eq!(emulator.scroll_metrics().history, 4);
}

#[test]
fn clearing_leaves_an_alternate_screen_alone() {
    let mut emulator = fed(10, 3, b"one\r\ntwo\r\nthree\r\nfour\x1b[?1049halt app");
    let before = emulator.frame();
    emulator.clear();
    assert_eq!(emulator.frame(), before);
    emulator.feed(b"\x1b[?1049l");
    assert!(emulator.text().contains("one"));
}

#[test]
fn output_resizes_and_clears_advance_the_revision() {
    let mut emulator = emulator(10, 3);
    let start = emulator.revision();
    emulator.feed(b"x");
    assert_eq!(emulator.revision(), start + 1);
    emulator.scroll(Scroll::Lines(1));
    assert_eq!(emulator.revision(), start + 1, "scrolling changes no text");
    emulator.resize(size(12, 3));
    assert_eq!(emulator.revision(), start + 2);
}

#[test]
fn a_history_is_full_once_it_holds_all_it_keeps() {
    let mut emulator = Emulator::new(Size::new(20, 3).unwrap(), 4);
    emulator.feed(b"a\r\nb\r\nc\r\nd\r\ne\r\nf");
    let metrics = emulator.scroll_metrics();
    assert_eq!((metrics.history, metrics.full), (3, false));
    emulator.feed(b"\r\ng");
    let metrics = emulator.scroll_metrics();
    assert_eq!((metrics.history, metrics.full), (4, true));
    // Another line drops the oldest; the history stays full.
    emulator.feed(b"\r\nh");
    assert!(emulator.scroll_metrics().full);
}
