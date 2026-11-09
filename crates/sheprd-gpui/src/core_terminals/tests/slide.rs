//! Smooth scrolling in a herdr-core view: a move through the history slides
//! into place; output and long jumps do not.

use super::{
    history::{frame, shown},
    worker::until,
};
use gpui::{point as at, px};
use sheprd_backend::{ScrollMetrics, SurfaceUpdate};
use std::time::Duration;

fn scrolled(offset: usize, revision: u64) -> Vec<SurfaceUpdate> {
    vec![
        SurfaceUpdate::Frame(frame(10, 3)),
        SurfaceUpdate::Scroll {
            metrics: ScrollMetrics {
                offset,
                history: 7,
                rows: 3,
                full: false,
            },
            revision,
        },
    ]
}

#[gpui::test]
fn a_scroll_through_the_history_slides_into_place(cx: &mut gpui::TestAppContext) {
    let (view, cx, backend) = shown(cx);
    backend.push(scrolled(2, 4));
    let sliding = until(|| {
        cx.update(|window, cx| {
            view.update(cx, |view, cx| {
                view.poll(window, cx);
                view.sliding(crate::motion::now())
            })
        })
    });
    // It starts where the frame before it was drawn: two rows up, with
    // that frame two rows below it.
    assert!(sliding.lift > 1.5 && sliding.lift <= 2., "{}", sliding.lift);
    assert_eq!(sliding.behind_at, 2.);
    let landed = crate::motion::now() + Duration::from_secs(1);
    cx.update(|_, cx| view.update(cx, |view, _| assert!(view.sliding(landed).is_none())));
}

#[gpui::test]
fn new_output_and_long_jumps_do_not_slide(cx: &mut gpui::TestAppContext) {
    let (view, cx, backend) = shown(cx);
    // New text at the same time: drawn where it is.
    backend.push(scrolled(1, 5));
    // Further than the screen is tall: a jump.
    backend.push(scrolled(6, 5));
    until(|| {
        cx.update(|window, cx| {
            view.update(cx, |view, cx| {
                view.poll(window, cx);
                assert!(view.sliding(crate::motion::now()).is_none());
                (view.top() == 1).then_some(())
            })
        })
    });
}

#[gpui::test]
fn a_scroll_during_a_slide_goes_no_further_than_the_frame_behind(cx: &mut gpui::TestAppContext) {
    let (view, cx, backend) = shown(cx);
    let lift = |cx: &mut gpui::VisualTestContext, offset| {
        backend.push(scrolled(offset, 4));
        until(|| {
            cx.update(|window, cx| {
                view.update(cx, |view, cx| {
                    view.poll(window, cx);
                    (view.top() == 7 - offset as u32)
                        .then(|| view.sliding(crate::motion::now()))
                        .flatten()
                })
            })
        })
    };
    lift(cx, 1);
    // A row more while the first is still sliding: the frame behind is a
    // row off, so the slide starts at most a row up, leaving no rows blank.
    let sliding = lift(cx, 2);
    assert_eq!(sliding.behind_at, 1.);
    assert!(sliding.lift <= 1., "{}", sliding.lift);
}

#[gpui::test]
fn a_press_during_a_slide_lands_on_the_row_drawn_under_it(cx: &mut gpui::TestAppContext) {
    let (view, cx, backend) = shown(cx);
    cx.run_until_parked();
    backend.push(scrolled(2, 4));
    let now = crate::motion::now();
    let sliding = until(|| {
        cx.update(|window, cx| {
            view.update(cx, |view, cx| {
                view.poll(window, cx);
                view.sliding(now)
            })
        })
    });
    // The frame is drawn about two rows up, so its third row is painted
    // at the top of the view.
    assert!(sliding.lift > 1.5, "{}", sliding.lift);
    let bounds = cx.update(|_, cx| view.read(cx).bounds()).unwrap();
    let top = bounds.origin + at(px(1.), px(1.));
    cx.update(|_, cx| assert_eq!(view.read(cx).row_at(top, now), Some(2)));
}
