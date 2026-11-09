use super::*;
use sheprd_backend::{
    History, HistoryAnswer, ScrollMetrics, Search, SearchDirection, TextPoint, TextRange,
};

fn scroll(updates: &[SurfaceUpdate]) -> Option<ScrollMetrics> {
    updates.iter().find_map(|update| match update {
        SurfaceUpdate::Scroll { metrics, .. } => Some(*metrics),
        _ => None,
    })
}

#[test]
fn a_view_searches_scrolls_and_clears_its_own_history() {
    let (backend, mut mock) = backend();
    let mut terminal = attached(&backend, &mut mock, Role::Active);
    let lines: String = (0..10).map(|line| format!("line{line}\r\n")).collect();
    mock.send(5, Kind::Data, lines.as_bytes());
    let live = terminal.next(WAIT).unwrap();
    assert_eq!(
        scroll(&live),
        Some(ScrollMetrics {
            offset: 0,
            history: 7,
            rows: 4,
            full: false,
        })
    );

    let found = terminal
        .history(&History::Search(Search {
            query: "line3".into(),
            direction: SearchDirection::Backward,
            cursor: TextPoint { row: 11, col: 0 },
            previous: None,
        }))
        .unwrap();
    let HistoryAnswer::Matches(found) = found else {
        panic!("a search answers matches: {found:?}");
    };
    assert_eq!(
        found.matches,
        vec![TextRange {
            start: TextPoint { row: 3, col: 0 },
            end: TextPoint { row: 3, col: 4 },
        }]
    );

    terminal.history(&History::ScrollTo { offset: 2 }).unwrap();
    let scrolled = terminal.next(Duration::from_millis(50)).unwrap();
    assert_eq!(scroll(&scrolled).map(|metrics| metrics.offset), Some(2));
    assert_eq!(row_text(full_frame(&scrolled).unwrap(), 0), "line5");

    terminal.history(&History::Clear).unwrap();
    let cleared = terminal.next(Duration::from_millis(50)).unwrap();
    assert_eq!(scroll(&cleared).map(|metrics| metrics.history), Some(0));
    let HistoryAnswer::Text(text) = terminal.history(&History::Text).unwrap() else {
        panic!("the text answers text");
    };
    assert!(!text.contains("line3"), "{text:?}");
}
