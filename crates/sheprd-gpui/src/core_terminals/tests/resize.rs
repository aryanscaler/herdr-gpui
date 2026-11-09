//! A resize rewraps a herdr-core view's text, so the history points a
//! selection or a read named before it hold other text after.

use super::{
    history::{frame, shown},
    worker::until,
};
use gpui::{Modifiers, MouseButton, point as at, px};
use sheprd_backend::{HistoryAnswer, ScrollMetrics, SurfaceUpdate};

/// The view's screen, rewrapped twelve columns wide at text revision 5.
fn rewrapped() -> Vec<SurfaceUpdate> {
    vec![
        SurfaceUpdate::Frame(frame(12, 3)),
        SurfaceUpdate::Scroll {
            metrics: ScrollMetrics {
                offset: 0,
                history: 6,
                rows: 3,
                full: false,
            },
            revision: 5,
        },
    ]
}

#[gpui::test]
fn a_resize_drops_the_selection_and_a_copy_asked_for_before_it(cx: &mut gpui::TestAppContext) {
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
        .push(HistoryAnswer::Text("rewrapped".into()));
    cx.update(|window, cx| view.update(cx, |view, cx| view.poll(window, cx)));
    // The resize reaches the worker, but not yet the view, before the copy.
    backend.push(rewrapped());
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
