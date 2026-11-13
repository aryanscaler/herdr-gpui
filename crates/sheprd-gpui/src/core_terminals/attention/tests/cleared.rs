//! Clear All Notifications and the Herdr 2 news still waiting out a delay.

use super::*;

#[test]
fn a_clear_drops_waiting_banners_but_keeps_sounds_and_status() {
    let start = Instant::now();
    let cx = gpui::TestAppContext::single();
    cx.update(|cx| {
        cx.set_global(heard_change(
            start,
            AgentStatus::Working,
            AgentStatus::Blocked,
        ));
        super::super::forget_pending_posts(cx);
        let heard = cx.global_mut::<Heard>();
        // No banner arrives after the clear.
        assert!(heard.take_posts(after(start, 1000), DELAY).is_empty());
        // Its sound still plays, and the agent still waits on the user.
        assert_eq!(
            names(&heard.take_due(after(start, 1000), DELAY)),
            [("t1", News::NeedsYou)]
        );
        assert_eq!(
            heard.agents.get(&id("t1")).map(|agent| agent.status),
            Some(AgentStatus::Blocked)
        );
    });
}

#[test]
fn a_clear_before_any_news_is_heard_does_nothing() {
    let cx = gpui::TestAppContext::single();
    cx.update(|cx| {
        super::super::forget_pending_posts(cx);
        assert!(!cx.has_global::<Heard>());
    });
}
