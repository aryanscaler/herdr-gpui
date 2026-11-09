//! The text a note on a herdr-core pane quotes, and where its card sits.

use super::{
    history::{point, requests, shown},
    worker::until,
};
use gpui::{Modifiers, MouseButton, point as at, px};
use sheprd_backend::{History, HistoryAnswer, ScrollMetrics, SurfaceUpdate};

#[gpui::test]
fn a_selection_off_the_screen_is_quoted_from_the_history(cx: &mut gpui::TestAppContext) {
    let (view, cx, backend) = shown(cx);
    cx.run_until_parked();
    cx.update(|_, cx| view.update(cx, |view, _| assert!(!view.quote_selection())));
    let bounds = cx.update(|_, cx| view.read(cx).bounds()).unwrap();
    let cell =
        |column: f32, row: f32| bounds.origin + at(px(8.5 * column + 1.), px(17. * row + 1.));
    // From the bottom row, history row 9, to the second row, row 8.
    cx.simulate_mouse_down(cell(3., 2.), MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(cell(0., 1.), Some(MouseButton::Left), Modifiers::none());
    cx.simulate_mouse_up(cell(0., 1.), MouseButton::Left, Modifiers::none());
    cx.update(|_, cx| {
        view.update(cx, |view, _| {
            // The card sits beside the last selected row.
            let anchor = view.selection_anchor().unwrap();
            assert_eq!(anchor.origin, bounds.origin + at(px(0.), px(34.)));
            assert_eq!(anchor.size.width, px(8.5 * 4.));
        })
    });
    // Scrolled so the selection is off the screen, it is read the same way.
    backend
        .answers
        .lock()
        .unwrap()
        .push(HistoryAnswer::Text("quoted".into()));
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
    cx.update(|_, cx| {
        view.update(cx, |view, _| {
            assert!(view.quote_selection());
            assert!(view.selection_anchor().is_none());
        })
    });
    assert_eq!(
        requests(&backend, 1),
        vec![History::Read {
            anchor: point(8, 0),
            cursor: point(9, 3),
        }]
    );
    let quote = until(|| {
        cx.update(|window, cx| {
            view.update(cx, |view, cx| {
                view.poll(window, cx);
                view.take_quote()
            })
        })
    });
    assert_eq!(quote.unwrap(), "quoted");
}

#[gpui::test]
fn a_newer_quote_drops_the_answer_to_an_older_one(cx: &mut gpui::TestAppContext) {
    let (view, cx, backend) = shown(cx);
    cx.run_until_parked();
    let bounds = cx.update(|_, cx| view.read(cx).bounds()).unwrap();
    let cell =
        |column: f32, row: f32| bounds.origin + at(px(8.5 * column + 1.), px(17. * row + 1.));
    cx.simulate_mouse_down(cell(3., 2.), MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(cell(0., 1.), Some(MouseButton::Left), Modifiers::none());
    cx.simulate_mouse_up(cell(0., 1.), MouseButton::Left, Modifiers::none());
    backend.answers.lock().unwrap().extend([
        HistoryAnswer::Text("older".into()),
        HistoryAnswer::Text("newer".into()),
    ]);
    cx.update(|_, cx| {
        view.update(cx, |view, _| {
            assert!(view.quote_selection());
            assert!(view.quote_selection());
        })
    });
    let quote = until(|| {
        cx.update(|window, cx| {
            view.update(cx, |view, cx| {
                view.poll(window, cx);
                view.take_quote()
            })
        })
    });
    assert_eq!(quote.unwrap(), "newer");
}

#[gpui::test]
fn only_a_view_drawn_in_the_last_frame_is_shown(cx: &mut gpui::TestAppContext) {
    let (view, _, _) = shown(cx);
    let mut terminals = crate::core_terminals::CoreTerminals::default();
    // A cached view of a tab no group shows is not on screen.
    assert!(!terminals.shows(&view));
    terminals.draw(&view);
    assert!(terminals.shows(&view));
    // A frame that does not draw it, as after switching tabs, hides it.
    terminals.start_frame();
    assert!(!terminals.shows(&view));
}

#[gpui::test]
fn a_quote_whose_rows_moved_while_it_was_read_fails(cx: &mut gpui::TestAppContext) {
    let (view, cx, backend) = shown(cx);
    cx.run_until_parked();
    let bounds = cx.update(|_, cx| view.read(cx).bounds()).unwrap();
    let cell =
        |column: f32, row: f32| bounds.origin + at(px(8.5 * column + 1.), px(17. * row + 1.));
    cx.simulate_mouse_down(cell(3., 2.), MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(cell(0., 1.), Some(MouseButton::Left), Modifiers::none());
    cx.simulate_mouse_up(cell(0., 1.), MouseButton::Left, Modifiers::none());
    backend
        .answers
        .lock()
        .unwrap()
        .push(HistoryAnswer::Text("shifted".into()));
    cx.update(|window, cx| view.update(cx, |view, cx| view.poll(window, cx)));
    // Output into the full history reaches the worker, but not yet the
    // view, before the quote is asked for.
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
    cx.update(|_, cx| view.update(cx, |view, _| assert!(view.quote_selection())));
    let quote = until(|| {
        cx.update(|window, cx| {
            view.update(cx, |view, cx| {
                view.poll(window, cx);
                view.take_quote()
            })
        })
    });
    assert!(matches!(quote, Err(crate::core_terminals::Error::Moved)));
}

#[gpui::test]
fn copy_modes_marked_cells_are_quoted(cx: &mut gpui::TestAppContext) {
    let (view, cx, backend) = shown(cx);
    cx.run_until_parked();
    let bounds = cx.update(|_, cx| view.read(cx).bounds()).unwrap();
    cx.update(|window, cx| view.update(cx, |view, cx| view.enter_copy_mode(window, cx)));
    // A cursor alone marks nothing.
    cx.update(|_, cx| view.update(cx, |view, _| assert!(!view.quote_selection())));
    // From the start of the last row, history row 9, three cells.
    cx.simulate_keystrokes("v l l");
    cx.update(|_, cx| {
        view.update(cx, |view, _| {
            let anchor = view.selection_anchor().unwrap();
            assert_eq!(anchor.origin, bounds.origin + at(px(0.), px(34.)));
            assert_eq!(anchor.size.width, px(8.5 * 3.));
            assert!(view.quote_selection());
        })
    });
    assert_eq!(
        requests(&backend, 1),
        vec![History::Read {
            anchor: point(9, 0),
            cursor: point(9, 2),
        }]
    );
}
