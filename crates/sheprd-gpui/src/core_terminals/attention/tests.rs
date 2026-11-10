use super::{Heard, News, in_front};
use gpui::WindowId;
use sheprd_backend::{AgentStatus, TerminalId};
use sheprd_client::herdr::AgentKind;
use std::{
    collections::HashSet,
    time::{Duration, Instant},
};

const DELAY: Duration = Duration::from_secs(1);

fn id(name: &str) -> TerminalId {
    TerminalId::new(name).unwrap()
}

fn after(start: Instant, millis: u64) -> Instant {
    start + Duration::from_millis(millis)
}

fn names<T>(news: &[(TerminalId, News, T)]) -> Vec<(&str, News)> {
    news.iter().map(|(t, n, _)| (t.as_str(), *n)).collect()
}

fn heard_names(news: &[(TerminalId, News)]) -> Vec<(&str, News)> {
    news.iter().map(|(t, n)| (t.as_str(), *n)).collect()
}

/// `heard` with `t1` known as `before`, then as `now`, the news queued at
/// `start`.
fn heard_change(start: Instant, before: AgentStatus, now: AgentStatus) -> Heard {
    let mut heard = Heard::default();
    let t1 = id("t1");
    heard.hear(start, [(&t1, before, None)], |_| true);
    for (terminal, news) in heard.hear(after(start, 10), [(&t1, now, None)], |_| true) {
        heard.push(start, terminal, news);
    }
    heard
}

#[test]
fn only_a_change_into_blocked_or_out_of_a_turn_is_news() {
    use AgentStatus::*;
    assert_eq!(News::of(None, Blocked), None, "first heard of");
    assert_eq!(News::of(Some(Working), Blocked), Some(News::NeedsYou));
    assert_eq!(News::of(Some(Idle), Blocked), Some(News::NeedsYou));
    assert_eq!(News::of(Some(Blocked), Blocked), None);
    assert_eq!(News::of(Some(Working), Done), Some(News::Finished));
    assert_eq!(News::of(Some(Working), Idle), Some(News::Finished));
    assert_eq!(News::of(Some(Idle), Done), None);
    assert_eq!(News::of(Some(Blocked), Working), None);
    assert_eq!(News::of(Some(Working), Unknown), None);
}

#[test]
fn news_must_still_hold_after_the_delay() {
    assert!(News::NeedsYou.holds(AgentStatus::Blocked));
    assert!(!News::NeedsYou.holds(AgentStatus::Working));
    assert!(News::Finished.holds(AgentStatus::Idle));
    assert!(!News::Finished.holds(AgentStatus::Working));
}

#[test]
fn a_change_is_news_once_across_windows() {
    let start = Instant::now();
    let mut heard = Heard::default();
    let t1 = id("t1");
    let known = |_: &TerminalId| true;
    assert!(
        heard
            .hear(start, [(&t1, AgentStatus::Working, None)], known)
            .is_empty()
    );
    assert_eq!(
        heard_names(&heard.hear(after(start, 10), [(&t1, AgentStatus::Blocked, None)], known)),
        [("t1", News::NeedsYou)]
    );
    // A second window lists the same core a moment later.
    assert!(
        heard
            .hear(after(start, 20), [(&t1, AgentStatus::Blocked, None)], known)
            .is_empty()
    );
}

#[test]
fn a_list_older_than_one_taken_is_not_news() {
    let start = Instant::now();
    let mut heard = Heard::default();
    let t1 = id("t1");
    let known = |_: &TerminalId| true;
    heard.hear(start, [(&t1, AgentStatus::Working, None)], known);
    heard.hear(after(start, 20), [(&t1, AgentStatus::Blocked, None)], known);
    // Another window's slower read, asked for before the newest, arrives
    // late: it neither undoes the block nor makes the next one news again.
    assert!(
        heard
            .hear(after(start, 10), [(&t1, AgentStatus::Working, None)], known)
            .is_empty()
    );
    assert!(
        heard
            .hear(after(start, 30), [(&t1, AgentStatus::Blocked, None)], known)
            .is_empty()
    );
}

#[test]
fn terminals_the_core_dropped_are_forgotten() {
    let start = Instant::now();
    let mut heard = Heard::default();
    let t1 = id("t1");
    heard.hear(start, [(&t1, AgentStatus::Working, None)], |_| true);
    heard.hear(after(start, 10), [], |_| false);
    // Heard of afresh, so a blocked agent reusing the address is quiet.
    assert!(
        heard
            .hear(
                after(start, 20),
                [(&t1, AgentStatus::Blocked, None)],
                |_| true
            )
            .is_empty()
    );
}

#[test]
fn a_newer_change_replaces_the_one_waiting() {
    let mut heard = Heard::default();
    let now = Instant::now();
    heard.push(now, id("t1"), News::NeedsYou);
    heard.push(now, id("t1"), News::Finished);
    heard.push(now, id("t2"), News::Finished);
    let due: Vec<_> = heard.due.iter().map(|(_, t, n)| (t.as_str(), *n)).collect();
    assert_eq!(due, [("t1", News::Finished), ("t2", News::Finished)]);
}

#[test]
fn news_waits_out_the_delay_and_must_still_hold() {
    let start = Instant::now();
    let mut heard = heard_change(start, AgentStatus::Working, AgentStatus::Blocked);
    assert!(
        heard.take_due(after(start, 500), DELAY).is_empty(),
        "too soon"
    );
    assert_eq!(
        names(&heard.take_due(after(start, 1_000), DELAY)),
        [("t1", News::NeedsYou)]
    );
    assert!(heard.due.is_empty());
    // Answered before the delay was over.
    let mut heard = heard_change(start, AgentStatus::Working, AgentStatus::Blocked);
    heard.hear(
        after(start, 500),
        [(&id("t1"), AgentStatus::Working, None)],
        |_| true,
    );
    assert!(heard.take_due(after(start, 1_000), DELAY).is_empty());
}

#[test]
fn news_no_window_could_play_in_time_is_dropped() {
    let start = Instant::now();
    let mut heard = heard_change(start, AgentStatus::Working, AgentStatus::Blocked);
    assert!(heard.take_due(after(start, 10_000), DELAY).is_empty());
    assert!(heard.due.is_empty());
}

#[test]
fn news_waits_app_wide_for_whichever_window_plays_it() {
    // Heard in one window, which then closed: the news is not the window's.
    let start = Instant::now();
    let mut heard = heard_change(start, AgentStatus::Working, AgentStatus::Done);
    heard.show(WindowId::from(1), HashSet::new(), |_| false);
    assert_eq!(
        names(&heard.take_due(after(start, 1_000), DELAY)),
        [("t1", News::Finished)]
    );
}

#[test]
fn a_finished_turn_shown_in_the_active_window_is_quiet() {
    let start = Instant::now();
    let shown = HashSet::from([id("t1")]);
    let mut heard = heard_change(start, AgentStatus::Working, AgentStatus::Done);
    heard.show(WindowId::from(1), shown.clone(), |_| true);
    assert!(heard.take_due(after(start, 1_000), DELAY).is_empty());
    // A request still sounds.
    let mut heard = heard_change(start, AgentStatus::Working, AgentStatus::Blocked);
    heard.show(WindowId::from(1), shown.clone(), |_| true);
    assert_eq!(heard.take_due(after(start, 1_000), DELAY).len(), 1);
    // The window that showed it closed, or no longer shows it.
    let mut heard = heard_change(start, AgentStatus::Working, AgentStatus::Done);
    heard.show(WindowId::from(1), shown, |_| true);
    heard.show(WindowId::from(2), HashSet::new(), |window| {
        window != WindowId::from(1)
    });
    assert_eq!(heard.take_due(after(start, 1_000), DELAY).len(), 1);
}

#[test]
fn another_windows_newer_list_does_not_hide_this_ones_terminals() {
    // Two windows show different tabs: each lists only its own terminals.
    let start = Instant::now();
    let mut heard = Heard::default();
    let (t1, t2) = (id("t1"), id("t2"));
    let known = |_: &TerminalId| true;
    heard.hear(start, [(&t1, AgentStatus::Working, None)], known);
    heard.hear(after(start, 30), [(&t2, AgentStatus::Working, None)], known);
    // The first window's list, asked for before the second's, still says
    // what happened to its own terminal.
    assert_eq!(
        heard_names(&heard.hear(after(start, 20), [(&t1, AgentStatus::Blocked, None)], known)),
        [("t1", News::NeedsYou)]
    );
    // But not against what a newer list said of the same terminal.
    heard.hear(after(start, 40), [(&t1, AgentStatus::Working, None)], known);
    assert!(
        heard
            .hear(after(start, 35), [(&t1, AgentStatus::Blocked, None)], known)
            .is_empty()
    );
}

#[test]
fn news_keeps_the_agent_it_was_heard_from() {
    // Whichever window plays it, the agent's own sound setting applies.
    let start = Instant::now();
    let mut heard = Heard::default();
    let t1 = id("t1");
    let droid = Some(AgentKind::Droid);
    heard.hear(start, [(&t1, AgentStatus::Working, droid)], |_| true);
    for (terminal, news) in heard.hear(
        after(start, 10),
        [(&t1, AgentStatus::Blocked, droid)],
        |_| true,
    ) {
        heard.push(start, terminal, news);
    }
    let due = heard.take_due(after(start, 1_000), DELAY);
    assert_eq!(due.len(), 1);
    assert_eq!(due[0].2, droid);
}

#[test]
fn the_stand_in_shell_is_in_front_only_while_its_terminal_is_shown() {
    let shell = id("t1");
    let tab = HashSet::from([id("t2"), id("t3")]);
    // The terminal is shown: its shell is in front.
    assert_eq!(
        in_front(true, None, Some(shell.clone())),
        HashSet::from([shell.clone()])
    );
    // A herdr-core tab is shown: its panes are.
    assert_eq!(in_front(true, Some(tab.clone()), None), tab);
    // A browser or code page is shown: nothing is, the shell included.
    assert!(in_front(true, None, None).is_empty());
    // An inactive window shows the user nothing.
    assert!(in_front(false, Some(tab), Some(shell)).is_empty());
}

#[test]
fn a_shell_hidden_behind_a_page_still_sounds_when_it_finishes() {
    let start = Instant::now();
    let mut heard = heard_change(start, AgentStatus::Working, AgentStatus::Done);
    // With Herdr 1 missing, a browser tab covers the stand-in shell `t1`:
    // the active group shows the page, so the shell is not passed in front.
    heard.show(WindowId::from(1), in_front(true, None, None), |_| true);
    assert_eq!(
        names(&heard.take_due(after(start, 1_000), DELAY)),
        [("t1", News::Finished)]
    );
    // Shown, it stays quiet.
    let mut heard = heard_change(start, AgentStatus::Working, AgentStatus::Done);
    heard.show(
        WindowId::from(1),
        in_front(true, None, Some(id("t1"))),
        |_| true,
    );
    assert!(heard.take_due(after(start, 1_000), DELAY).is_empty());
}

#[test]
fn news_waiting_for_a_muted_terminal_never_plays() {
    let start = Instant::now();
    let mut heard = heard_change(start, AgentStatus::Working, AgentStatus::Blocked);
    // Muted while the news waited out its delay, then unmuted before it ends.
    heard.forget_due(|terminal| terminal.as_str() == "t1");
    assert!(heard.take_due(after(start, 1_500), DELAY).is_empty());
    assert!(
        heard.take_posts(after(start, 60_000), DELAY).is_empty(),
        "nor does it post a system notification"
    );
    let mut kept = heard_change(start, AgentStatus::Working, AgentStatus::Blocked);
    kept.forget_due(|terminal| terminal.as_str() == "t9");
    assert_eq!(
        names(&kept.take_due(after(start, 1_500), DELAY)),
        [("t1", News::NeedsYou)]
    );
}

#[test]
fn banners_wait_out_their_own_delay_apart_from_the_sound() {
    let start = Instant::now();
    let mut heard = heard_change(start, AgentStatus::Working, AgentStatus::Blocked);
    // The sound plays after a second; the banner waits a minute.
    let banner = Duration::from_secs(60);
    assert_eq!(
        names(&heard.take_due(after(start, 1_000), DELAY)),
        [("t1", News::NeedsYou)]
    );
    assert!(heard.take_posts(after(start, 1_000), banner).is_empty());
    assert_eq!(
        names(&heard.take_posts(after(start, 60_000), banner)),
        [("t1", News::NeedsYou)]
    );
    // A change answered before the banner's delay posts nothing.
    let mut heard = heard_change(start, AgentStatus::Working, AgentStatus::Blocked);
    heard.hear(
        after(start, 30_000),
        [(&id("t1"), AgentStatus::Working, None)],
        |_| true,
    );
    assert!(heard.take_posts(after(start, 60_000), banner).is_empty());
}
