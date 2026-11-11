use super::*;
use gpui::{TestAppContext, VisualTestContext};

fn hosted(cx: &mut TestAppContext) -> (gpui::Entity<SheprdWindow>, &mut VisualTestContext) {
    let (view, cx) = cx.add_window_view(crate::sidebar::layout_tests::fixture_window);
    view.update(cx, |view, _| view.hosted = true);
    (view, cx)
}

#[gpui::test]
fn sizes_follow_what_the_window_shows(cx: &mut TestAppContext) {
    let (view, cx) = hosted(cx);
    let sizes = |view: &gpui::Entity<SheprdWindow>, cx: &mut VisualTestContext| {
        cx.update(|window, cx| view.read(cx).region_sizes(window))
    };
    view.update(cx, |view, _| view.status_bar_visible = true);
    assert!(sizes(&view, cx).status_bar > 0.);
    view.update(cx, |view, _| view.status_bar_visible = false);
    assert_eq!(sizes(&view, cx).status_bar, 0.);
}

#[gpui::test]
fn a_hosted_window_leaves_its_regions_to_the_host(cx: &mut TestAppContext) {
    let (_view, cx) = hosted(cx);
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(cx.debug_bounds("activity").is_none());
    assert!(cx.debug_bounds("window-body").is_some());
}
