//! Clearing a herdr-core view drops the history lines its rows are counted
//! from, so what was selected, marked, or being copied names other text.

use super::{
    history::{frame, requests, shown},
    worker::until,
};
use gpui::{Modifiers, MouseButton, point as at, px};
use sheprd_backend::{History, HistoryAnswer, ScrollMetrics, SurfaceUpdate};

/// The view's screen after a clear: the same grid, no history, revision 5.
fn cleared() -> Vec<SurfaceUpdate> {
    vec![
        SurfaceUpdate::Frame(frame(10, 3)),
        SurfaceUpdate::Scroll {
            metrics: ScrollMetrics {
                offset: 0,
                history: 0,
                rows: 3,
                full: false,
            },
            revision: 5,
        },
    ]
}

#[gpui::test]
fn clearing_forgets_copy_modes_mark(cx: &mut gpui::TestAppContext) {
    let (view, cx, backend) = shown(cx);
    cx.update(|window, cx| view.update(cx, |view, cx| view.enter_copy_mode(window, cx)));
    cx.simulate_keystrokes("v l");
    cx.update(|_, cx| view.update(cx, |view, cx| view.clear_history(cx)));
    // With nothing marked, `y` leaves without reading anything.
    cx.simulate_keystrokes("y");
    cx.update(|_, cx| assert!(!view.read(cx).copy_mode_active()));
    let sent = requests(&backend, 2);
    assert_eq!(sent[0], History::Clear);
    assert!(
        sent.iter()
            .all(|request| !matches!(request, History::Read { .. })),
        "{sent:?}"
    );
}

#[gpui::test]
fn a_copy_asked_for_before_a_clear_copies_nothing(cx: &mut gpui::TestAppContext) {
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
        .push(HistoryAnswer::Text("other".into()));
    cx.update(|window, cx| view.update(cx, |view, cx| view.poll(window, cx)));
    // A clear the worker applied reaches it, but not yet the view, before
    // the copy.
    backend.push(cleared());
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
    // The rows selected before it no longer hold the selected text.
    cx.update(|_, cx| view.update(cx, |view, _| assert!(!view.copy_selection())));
}
