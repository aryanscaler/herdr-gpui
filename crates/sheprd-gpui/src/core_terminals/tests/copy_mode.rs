//! Copy mode and selections reaching into the history on a herdr-core view,
//! answered by its own emulator through the worker.

use super::{
    history::{point, requests, shown},
    worker::until,
};
use gpui::{Modifiers, MouseButton, point as at, px};
use sheprd_backend::{CopyMotion, History, HistoryAnswer, ScrollMetrics, SurfaceUpdate};

#[gpui::test]
fn copy_mode_walks_the_history_and_copies_what_it_marked(cx: &mut gpui::TestAppContext) {
    let (view, cx, backend) = shown(cx);
    backend.answers.lock().unwrap().extend([
        HistoryAnswer::Point(point(9, 4)),
        HistoryAnswer::Text("ab".into()),
    ]);
    cx.update(|window, cx| view.update(cx, |view, cx| view.enter_copy_mode(window, cx)));
    // With no cursor shown, copy mode starts on the last row.
    cx.simulate_keystrokes("w");
    assert_eq!(
        requests(&backend, 1),
        vec![History::Motion {
            cursor: point(9, 0),
            motion: CopyMotion::NextWordStart,
        }]
    );
    // Keys typed while the motion is out wait for its answer.
    cx.simulate_keystrokes("v l y");
    let read = until(|| {
        cx.update(|window, cx| view.update(cx, |view, cx| view.poll(window, cx)));
        requests(&backend, 1).get(1).cloned()
    });
    assert_eq!(
        read,
        History::Read {
            anchor: point(9, 4),
            cursor: point(9, 5),
        }
    );
    // Leaving goes back to where the view was.
    assert_eq!(requests(&backend, 3)[2], History::ScrollTo { offset: 0 });
    let copied = until(|| {
        cx.update(|window, cx| {
            view.update(cx, |view, cx| view.poll(window, cx));
            cx.read_from_clipboard().and_then(|item| item.text())
        })
    });
    assert_eq!(copied, "ab");
    cx.update(|_, cx| assert!(!view.read(cx).copy_mode_active()));
}

#[gpui::test]
fn a_selection_reaching_past_the_screen_is_read_from_the_history(cx: &mut gpui::TestAppContext) {
    let (view, cx, backend) = shown(cx);
    cx.run_until_parked();
    let bounds = cx.update(|_, cx| view.read(cx).bounds()).unwrap();
    let cell =
        |column: f32, row: f32| bounds.origin + at(px(8.5 * column + 1.), px(17. * row + 1.));
    // Press on the bottom row, history row 9.
    cx.simulate_mouse_down(cell(2., 2.), MouseButton::Left, Modifiers::none());
    // Scrolled up five rows, the top row is history row 2.
    backend.push(vec![SurfaceUpdate::Scroll {
        metrics: ScrollMetrics {
            offset: 5,
            history: 7,
            rows: 3,
            full: false,
        },
        revision: 4,
    }]);
    until(|| {
        cx.update(|window, cx| {
            view.update(cx, |view, cx| {
                view.poll(window, cx);
                (view.top() == 2).then_some(())
            })
        })
    });
    cx.simulate_mouse_move(cell(1., 0.), Some(MouseButton::Left), Modifiers::none());
    cx.simulate_mouse_up(cell(1., 0.), MouseButton::Left, Modifiers::none());
    cx.update(|_, cx| view.update(cx, |view, _| assert!(view.copy_selection())));
    assert_eq!(
        requests(&backend, 1),
        vec![History::Read {
            anchor: point(2, 1),
            cursor: point(9, 2),
        }]
    );
}

#[gpui::test]
fn a_drag_held_above_the_screen_scrolls_the_history(cx: &mut gpui::TestAppContext) {
    let (view, cx, backend) = shown(cx);
    cx.run_until_parked();
    let bounds = cx.update(|_, cx| view.read(cx).bounds()).unwrap();
    let inside = bounds.origin + at(px(5.), px(5.));
    cx.simulate_mouse_down(inside, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(
        bounds.origin + at(px(5.), px(-20.)),
        Some(MouseButton::Left),
        Modifiers::none(),
    );
    cx.update(|window, cx| view.update(cx, |view, cx| view.poll(window, cx)));
    assert_eq!(requests(&backend, 1), vec![History::ScrollTo { offset: 1 }]);
}

#[gpui::test]
fn a_shown_selection_is_read_from_the_history_too(cx: &mut gpui::TestAppContext) {
    let (view, cx, backend) = shown(cx);
    cx.run_until_parked();
    let bounds = cx.update(|_, cx| view.read(cx).bounds()).unwrap();
    let cell =
        |column: f32, row: f32| bounds.origin + at(px(8.5 * column + 1.), px(17. * row + 1.));
    // Rows 8 and 9 of the history, both on the screen: copied the same
    // way as one reaching past it, so wrapped rows join alike.
    cx.simulate_mouse_down(cell(1., 1.), MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(cell(3., 2.), Some(MouseButton::Left), Modifiers::none());
    cx.simulate_mouse_up(cell(3., 2.), MouseButton::Left, Modifiers::none());
    cx.update(|_, cx| view.update(cx, |view, _| assert!(view.copy_selection())));
    assert_eq!(
        requests(&backend, 1),
        vec![History::Read {
            anchor: point(8, 1),
            cursor: point(9, 3),
        }]
    );
}

#[gpui::test]
fn output_into_a_full_history_drops_the_selection(cx: &mut gpui::TestAppContext) {
    let (view, cx, backend) = shown(cx);
    cx.run_until_parked();
    let bounds = cx.update(|_, cx| view.read(cx).bounds()).unwrap();
    let cell =
        |column: f32, row: f32| bounds.origin + at(px(8.5 * column + 1.), px(17. * row + 1.));
    cx.simulate_mouse_down(cell(1., 1.), MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(cell(3., 2.), Some(MouseButton::Left), Modifiers::none());
    cx.simulate_mouse_up(cell(3., 2.), MouseButton::Left, Modifiers::none());
    let full = |revision| {
        vec![SurfaceUpdate::Scroll {
            metrics: ScrollMetrics {
                offset: 0,
                history: 7,
                rows: 3,
                full: true,
            },
            revision,
        }]
    };
    // Waits for the screen of `revision` itself: a poll may first apply an
    // update that was already on its way.
    let shown_revision = |revision: u64, cx: &mut gpui::VisualTestContext| {
        until(|| {
            cx.update(|window, cx| {
                view.update(cx, |view, cx| {
                    view.poll(window, cx);
                    (view.screen_revision() == revision).then_some(())
                })
            })
        })
    };
    // Full, but nothing new: the rows still hold the selected text.
    backend.push(full(4));
    shown_revision(4, cx);
    cx.update(|_, cx| view.update(cx, |view, _| assert!(view.copy_selection())));
    // New output drops the oldest line, moving every row under it.
    backend.push(full(5));
    shown_revision(5, cx);
    cx.update(|_, cx| view.update(cx, |view, _| assert!(!view.copy_selection())));
}

#[gpui::test]
fn a_copy_whose_rows_moved_while_it_was_read_copies_nothing(cx: &mut gpui::TestAppContext) {
    let (view, cx, backend) = shown(cx);
    cx.run_until_parked();
    let bounds = cx.update(|_, cx| view.read(cx).bounds()).unwrap();
    let cell =
        |column: f32, row: f32| bounds.origin + at(px(8.5 * column + 1.), px(17. * row + 1.));
    cx.simulate_mouse_down(cell(1., 1.), MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(cell(3., 2.), Some(MouseButton::Left), Modifiers::none());
    cx.simulate_mouse_up(cell(3., 2.), MouseButton::Left, Modifiers::none());
    backend
        .answers
        .lock()
        .unwrap()
        .push(HistoryAnswer::Text("shifted".into()));
    cx.update(|window, cx| view.update(cx, |view, cx| view.poll(window, cx)));
    // Output into the full history reaches the worker, but not yet the
    // view, before the copy is asked for.
    backend.push(vec![SurfaceUpdate::Scroll {
        metrics: ScrollMetrics {
            offset: 0,
            history: 7,
            rows: 3,
            full: true,
        },
        revision: 5,
    }]);
    until(|| cx.update(|_, cx| view.read(cx).changed().then_some(())));
    cx.update(|_, cx| view.update(cx, |view, _| assert!(view.copy_selection())));
    until(|| {
        cx.update(|window, cx| {
            view.update(cx, |view, cx| {
                view.poll(window, cx);
                (!view.copy_pending()).then_some(())
            })
        })
    });
    cx.update(|_, cx| assert!(cx.read_from_clipboard().is_none()));
}

#[gpui::test]
fn a_match_rewritten_while_it_was_read_copies_nothing(cx: &mut gpui::TestAppContext) {
    let (view, cx, backend) = shown(cx);
    let found = sheprd_backend::TextRange {
        start: point(8, 1),
        end: point(8, 2),
    };
    backend.answers.lock().unwrap().extend([
        HistoryAnswer::Matches(sheprd_backend::Matches {
            matches: vec![found],
            total: 1,
            current: Some(0),
            current_global: Some(0),
            revision: 4,
        }),
        HistoryAnswer::Text("new".into()),
    ]);
    cx.update(|window, cx| view.update(cx, |view, cx| view.enter_copy_mode(window, cx)));
    cx.simulate_keystrokes("/");
    cx.simulate_input("ab");
    cx.simulate_keystrokes("enter");
    let History::Search(search) = requests(&backend, 1)[0].clone() else {
        panic!("copy mode searches first");
    };
    assert_eq!(search.query, "ab");
    until(|| {
        cx.update(|window, cx| {
            view.update(cx, |view, cx| {
                view.poll(window, cx);
                view.copy_mode_settled().then_some(())
            })
        })
    });
    // Output rewrites the match's row in place, with no row moved, and
    // reaches the worker, but not yet the view, before the copy.
    backend.push(vec![SurfaceUpdate::Scroll {
        metrics: ScrollMetrics {
            offset: 0,
            history: 7,
            rows: 3,
            full: false,
        },
        revision: 5,
    }]);
    until(|| cx.update(|_, cx| view.read(cx).changed().then_some(())));
    cx.simulate_keystrokes("y");
    assert_eq!(
        requests(&backend, 2)[1],
        History::Read {
            anchor: found.start,
            cursor: found.end,
        }
    );
    until(|| {
        cx.update(|window, cx| {
            view.update(cx, |view, cx| {
                view.poll(window, cx);
                (!view.copy_pending()).then_some(())
            })
        })
    });
    cx.update(|_, cx| assert!(cx.read_from_clipboard().is_none()));
}

#[gpui::test]
fn output_into_a_full_history_drops_copy_modes_mark(cx: &mut gpui::TestAppContext) {
    let (view, cx, backend) = shown(cx);
    cx.update(|window, cx| view.update(cx, |view, cx| view.enter_copy_mode(window, cx)));
    cx.simulate_keystrokes("v l");
    // New output drops the oldest line, moving the marked rows under it.
    backend.push(vec![SurfaceUpdate::Scroll {
        metrics: ScrollMetrics {
            offset: 0,
            history: 7,
            rows: 3,
            full: true,
        },
        revision: 5,
    }]);
    until(|| {
        cx.update(|window, cx| view.update(cx, |view, cx| view.poll(window, cx).then_some(())))
    });
    // With nothing marked, `y` leaves without reading anything.
    cx.simulate_keystrokes("y");
    cx.update(|_, cx| assert!(!view.read(cx).copy_mode_active()));
    let sent = requests(&backend, 1);
    assert!(
        sent.iter()
            .all(|request| !matches!(request, History::Read { .. })),
        "{sent:?}"
    );
}
