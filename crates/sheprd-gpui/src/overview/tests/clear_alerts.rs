//! The Recent alerts shelf's Clear, which runs Clear All Notifications.

use super::{
    alert_shelf::{alert, first_alert},
    page::{bounds, click, fixture},
};
use crate::sidebar::ActivityPanel;

#[gpui::test]
fn no_alerts_offer_no_clear(cx: &mut gpui::TestAppContext) {
    let (_, cx) = fixture(cx);
    assert!(cx.debug_bounds("overview-alerts-clear").is_none());
}

#[gpui::test]
fn clear_empties_the_shelf_without_folding_it(cx: &mut gpui::TestAppContext) {
    let (view, cx) = fixture(cx);
    alert(&view, cx);
    assert!(cx.debug_bounds("overview-alerts-clear").is_some());
    click(cx, "overview-alerts-clear");
    cx.update(|_, cx| {
        let view = view.read(cx);
        assert!(view.overview_alerts().is_empty());
        assert!(view.alert_infos().is_empty());
        // The click cleared; it did not also unfold the shelf.
        assert!(!view.overview.alerts_open);
        assert!(view.endpoints.iter().all(|e| e.toasts.entries.is_empty()));
    });
    assert!(cx.debug_bounds("overview-alerts").is_none());
    assert!(cx.debug_bounds("overview-alerts-clear").is_none());
}

#[gpui::test]
fn an_alert_after_a_clear_shows_and_opens_its_pane(cx: &mut gpui::TestAppContext) {
    let (view, cx) = fixture(cx);
    alert(&view, cx);
    let old = first_alert(&view, cx);
    click(cx, "overview-alerts-clear");
    alert(&view, cx);
    let new = first_alert(&view, cx);
    assert_ne!(old, new, "a new alert never takes a cleared one's id");
    click(cx, "overview-alerts");
    assert!(bounds(cx, old).is_none());
    click(cx, new);
    let panel = cx.update(|_, cx| view.read(cx).activity.panel);
    assert_eq!(panel, ActivityPanel::Workspaces);
}
