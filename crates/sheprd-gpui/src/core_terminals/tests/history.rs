//! Find, clear, and editing on a herdr-core view, answered by its own
//! history through the worker rather than by Herdr 1's daemon.

use super::{
    accessibility::open_on,
    worker::{Scripted, until},
};
use crate::{core_terminals::worker::Command, terminal_painter::Tint};
use gpui::{Entity, TestAppContext, VisualTestContext};
use sheprd_backend::{
    FrameData, History, HistoryAnswer, Matches, ScrollMetrics, SearchDirection, SurfaceUpdate,
    TextPoint, TextRange,
};
use sheprd_client::protocol::CellData;

use crate::core_terminals::CoreTerminalView;

pub(super) fn frame(width: u16, height: u16) -> FrameData {
    let blank = CellData {
        symbol: " ".into(),
        fg: 0,
        bg: 0,
        modifier: 0,
        skip: false,
        hyperlink: None,
    };
    FrameData {
        cells: vec![blank; usize::from(width) * usize::from(height)],
        width,
        height,
        cursor: None,
        hyperlinks: Vec::new(),
        graphics: Vec::new(),
    }
}

/// A view showing a ten-column, three-row screen under seven lines of
/// history, at text revision 4.
pub(super) fn shown(
    cx: &mut TestAppContext,
) -> (Entity<CoreTerminalView>, &mut VisualTestContext, Scripted) {
    let backend = Scripted::default();
    backend.push(vec![
        SurfaceUpdate::Frame(frame(10, 3)),
        SurfaceUpdate::Scroll {
            metrics: ScrollMetrics {
                offset: 0,
                history: 7,
                rows: 3,
                full: false,
            },
            revision: 4,
        },
    ]);
    let (view, cx, _) = open_on(cx, backend.clone(), false);
    // Releasing a selection copies nothing here, so a test's history reads
    // are only the ones it asks for.
    let quiet = crate::core_terminals::Look {
        copy_on_select: false,
        ..super::accessibility::look()
    };
    cx.update(|_, cx| view.update(cx, |view, cx| view.set_look(quiet, cx)));
    until(|| {
        cx.update(|window, cx| view.update(cx, |view, cx| view.poll(window, cx)))
            .then_some(())
    });
    (view, cx, backend)
}

/// The history requests the terminal received, once `count` arrived.
pub(super) fn requests(backend: &Scripted, count: usize) -> Vec<History> {
    until(|| {
        let requests: Vec<History> = backend
            .received
            .lock()
            .unwrap()
            .iter()
            .filter_map(|command| match command {
                Command::History { request, .. } => Some(request.clone()),
                _ => None,
            })
            .collect();
        (requests.len() >= count).then_some(requests)
    })
}

pub(super) fn point(row: u32, col: u16) -> TextPoint {
    TextPoint { row, col }
}

#[gpui::test]
fn find_searches_the_views_history_and_shows_its_match(cx: &mut TestAppContext) {
    let (view, cx, backend) = shown(cx);
    let found = TextRange {
        start: point(2, 1),
        end: point(2, 3),
    };
    // Typing searches for "f" at once; "foo" waits for that answer, which
    // no longer matters by then.
    backend.answers.lock().unwrap().push(HistoryAnswer::Done);
    backend
        .answers
        .lock()
        .unwrap()
        .push(HistoryAnswer::Matches(Matches {
            matches: vec![found],
            total: 1,
            current: Some(0),
            current_global: Some(0),
            revision: 4,
        }));
    cx.update(|window, cx| view.update(cx, |view, cx| view.open_find(window, cx)));
    cx.run_until_parked();
    cx.simulate_input("foo");
    let History::Search(first) = &requests(&backend, 1)[0] else {
        panic!("find searches first");
    };
    assert_eq!(first.query, "f");
    let search = until(|| {
        cx.update(|window, cx| view.update(cx, |view, cx| view.poll(window, cx)));
        requests(&backend, 1).get(1).cloned()
    });
    // The bar searches upward from the bottom of what the view shows.
    let History::Search(search) = search else {
        panic!("the typed query is searched next: {search:?}");
    };
    assert_eq!(search.query, "foo");
    assert_eq!(search.direction, SearchDirection::Backward);
    assert_eq!(search.cursor, point(10, 0));
    // The match is above the screen, so the view scrolls to center it.
    let scrolled = until(|| {
        cx.update(|window, cx| view.update(cx, |view, cx| view.poll(window, cx)));
        requests(&backend, 1).get(2).cloned()
    });
    assert_eq!(scrolled, History::ScrollTo { offset: 6 });

    // Once the view shows the match's row, it is tinted as current.
    backend.push(vec![SurfaceUpdate::Scroll {
        metrics: ScrollMetrics {
            offset: 6,
            history: 7,
            rows: 3,
            full: false,
        },
        revision: 4,
    }]);
    let highlights = until(|| {
        cx.update(|window, cx| {
            view.update(cx, |view, cx| {
                view.poll(window, cx);
                let highlights = view.find_highlights();
                (!highlights.is_empty()).then_some(highlights)
            })
        })
    });
    assert_eq!(highlights[0].row, 1);
    assert_eq!(highlights[0].columns, 1..4);
    assert_eq!(highlights[0].tint, Tint::CurrentMatch);

    // Closing keeps the query for the next bar and hands back the keyboard.
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.close_find(window, cx);
            assert!(view.find_highlights().is_empty());
            assert!(view.focus_handle().is_focused(window));
        })
    });
}

#[gpui::test]
fn clearing_asks_the_view_to_drop_its_history(cx: &mut TestAppContext) {
    let (view, cx, backend) = shown(cx);
    cx.update(|_, cx| view.update(cx, |view, cx| view.clear_history(cx)));
    assert_eq!(requests(&backend, 1), vec![History::Clear]);
}

#[gpui::test]
fn editing_reads_the_whole_text_once(cx: &mut TestAppContext) {
    let (view, cx, backend) = shown(cx);
    backend
        .answers
        .lock()
        .unwrap()
        .push(HistoryAnswer::Text("one\ntwo\n".into()));
    cx.update(|_, cx| view.update(cx, |view, _| view.request_edit()));
    assert_eq!(requests(&backend, 1), vec![History::Text]);
    let text = until(|| {
        cx.update(|window, cx| {
            view.update(cx, |view, cx| {
                view.poll(window, cx);
                view.take_edit()
            })
        })
    });
    assert_eq!(text.unwrap(), "one\ntwo\n");
    cx.update(|_, cx| view.update(cx, |view, _| assert!(view.take_edit().is_none())));
}

#[cfg(not(windows))]
#[test]
fn the_editor_runs_through_a_shell_and_removes_its_file() {
    let spawn = crate::core_terminals::history::editor_spawn(std::path::Path::new("/tmp/a b.txt"));
    assert_eq!(spawn.argv[0], "/bin/sh");
    assert!(spawn.argv[2].contains("${VISUAL:-${EDITOR:-vi}}"));
    assert!(spawn.argv[2].contains(r#"rm -f "$1""#));
    // The path is an argument, never spliced into the script.
    assert_eq!(spawn.argv.last().map(String::as_str), Some("/tmp/a b.txt"));
}

#[test]
fn the_editor_opens_beside_a_running_pane_and_in_a_tab_for_an_ended_one() {
    use crate::{browser::Scope, core_terminals::Purpose, core_terminals::history::editor_purpose};
    let pane = (
        crate::browser::TabId::test(1),
        sheprd_backend::TerminalId::new("t1").unwrap(),
    );
    let key = |workspace: &str| Some((Scope::endpoint("local"), workspace.to_owned()));
    assert!(matches!(
        editor_purpose(Some(pane.clone()), false, key("tab's"), key("current")),
        Some(Purpose::Split { at, .. }) if at == pane.1
    ));
    // An ended pane would be closed out of a split made beside it, so its
    // editor opens in a tab of the pane's workspace, not the one shown now.
    assert!(matches!(
        editor_purpose(Some(pane.clone()), true, key("tab's"), key("current")),
        Some(Purpose::Tab { workspace, .. }) if workspace == "tab's"
    ));
    assert!(editor_purpose(Some(pane), true, None, key("current")).is_none());
    // The stand-in shell has no tab: its editor opens where the window is.
    assert!(matches!(
        editor_purpose(None, false, None, key("current")),
        Some(Purpose::Tab { workspace, .. }) if workspace == "current"
    ));
}
