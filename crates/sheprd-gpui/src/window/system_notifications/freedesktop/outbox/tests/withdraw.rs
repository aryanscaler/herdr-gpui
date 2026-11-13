//! Clear All Notifications on the Linux bus: what it closes, and replies
//! that arrive after it.

use super::*;
use crate::window::system_notifications::{REGISTRY_LIMIT, WITHDRAW_LIMIT, withdraw_all};

#[gpui::test]
fn withdrawing_closes_shown_banners_and_drops_waiting_posts(cx: &mut TestAppContext) {
    let server = fixture(cx);
    server.post("a", "shown", cx);
    server.reply(0, Ok(7), cx);
    // Keep the delivery task busy, so the closes go through this server.
    server.post("b", "in flight", cx);
    server.post("c", "waiting", cx);
    cx.update(withdraw_all);
    server.reply(1, Ok(8), cx);
    // The shown banner closes, then the one answered after the clear; the
    // waiting post lost its route and is never sent.
    assert_eq!(server.closed(), [(7, 2), (8, 2)]);
    assert_eq!(server.sent().len(), 2);
    cx.update(|cx| {
        let registry = cx.global::<Registry>();
        assert!(registry.posted.is_empty());
        assert!(registry.bus_ids.is_empty());
        assert!(registry.find_bus_id(8).is_none());
        assert!(!registry.outbox.running);
    });
}

/// The same pane posts again before the old Notify is answered: the old id
/// closes first and never reaches the new post, even when the new post fails.
#[gpui::test]
fn a_reply_from_before_a_clear_never_reaches_the_tags_new_post(cx: &mut TestAppContext) {
    let server = fixture(cx);
    server.post("a", "before", cx);
    cx.update(withdraw_all);
    server.post("a", "after", cx);
    server.reply(0, Ok(7), cx);
    assert_eq!(server.closed(), [(7, 1)], "closed before the next post");
    assert_eq!(server.sent().last(), Some(&("a".into(), "after".into(), 0)));
    server.reply(
        1,
        Err(super::super::super::zbus::Error::Failure("rejected".into()).into()),
        cx,
    );
    cx.update(|cx| {
        let registry = cx.global::<Registry>();
        assert!(registry.find_bus_id(7).is_none());
        assert_eq!(registry.find("a").and_then(|posted| posted.bus_id), None);
    });
    // The next post for the pane is a new banner, not the cleared one.
    server.post("a", "retry", cx);
    assert_eq!(server.sent().last(), Some(&("a".into(), "retry".into(), 0)));
    server.reply(2, Ok(9), cx);
    cx.update(|cx| {
        let registry = cx.global::<Registry>();
        assert_eq!(registry.find_bus_id(9).map(|p| p.tag.as_ref()), Some("a"));
    });
    assert_eq!(server.closed(), [(7, 1)]);
}

/// Banners whose click routes were evicted still close.
#[gpui::test]
fn banners_past_the_click_routes_still_close(cx: &mut TestAppContext) {
    let server = fixture(cx);
    let count = REGISTRY_LIMIT + 6;
    for index in 0..count {
        server.post(&index.to_string(), "shown", cx);
        server.reply(index, Ok(100 + index as u32), cx);
    }
    cx.update(|cx| {
        let registry = cx.global::<Registry>();
        assert_eq!(registry.posted.len(), REGISTRY_LIMIT);
        assert!(registry.find_bus_id(100).is_none(), "its route was evicted");
    });
    server.post("busy", "in flight", cx);
    cx.update(withdraw_all);
    server.reply(count, Ok(1000), cx);
    let closed: Vec<u32> = server.closed().into_iter().map(|(id, _)| id).collect();
    let expected: Vec<u32> = (0..count as u32)
        .map(|index| 100 + index)
        .chain([1000])
        .collect();
    assert_eq!(closed, expected);
}

/// A full withdrawal waits behind a replacement in flight; its reply's id
/// is `late`. Returns the ids closed, in order.
fn full_withdrawal_with_late_reply(late: u32, cx: &mut TestAppContext) -> Vec<u32> {
    let server = fixture(cx);
    for index in 0..WITHDRAW_LIMIT {
        server.post(&index.to_string(), "shown", cx);
        server.reply(index, Ok(index as u32 + 1), cx);
    }
    // The newest banner is replaced; its Notify is still unanswered.
    let newest = (WITHDRAW_LIMIT - 1).to_string();
    server.post(&newest, "replacement", cx);
    assert_eq!(
        server.sent().last(),
        Some(&(newest, "replacement".into(), WITHDRAW_LIMIT as u32))
    );
    cx.update(withdraw_all);
    server.reply(WITHDRAW_LIMIT, Ok(late), cx);
    server.closed().into_iter().map(|(id, _)| id).collect()
}

#[gpui::test]
fn a_late_reply_naming_a_queued_banner_closes_it_once(cx: &mut TestAppContext) {
    // A replacement keeps its id, so the reply names a banner already queued.
    let closed = full_withdrawal_with_late_reply(WITHDRAW_LIMIT as u32, cx);
    let expected: Vec<u32> = (1..=WITHDRAW_LIMIT as u32).collect();
    assert_eq!(closed, expected);
}

#[gpui::test]
fn a_late_reply_with_a_new_id_drops_no_queued_banner(cx: &mut TestAppContext) {
    let closed = full_withdrawal_with_late_reply(9999, cx);
    let expected: Vec<u32> = (1..=WITHDRAW_LIMIT as u32).chain([9999]).collect();
    assert_eq!(closed, expected);
}
