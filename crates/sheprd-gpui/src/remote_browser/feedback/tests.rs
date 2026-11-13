#![allow(clippy::unwrap_used)]
use super::*;
use crate::browser::Batch;

fn pending(scope: &str, now: Instant) -> Pending {
    let terminal = TerminalId::new("t3").unwrap();
    let scope = Scope::endpoint(scope);
    Pending {
        window: 1,
        target: FeedbackKey {
            pane_id: crate::chat::core_pane::key_on(
                &crate::core_terminals::Host::Remote(scope.clone()),
                &terminal,
            ),
            scope,
        },
        terminal,
        request: "herdr-gpui-a1".into(),
        reply: "sheprd-reply-a1".into(),
        until: now + Duration::from_secs(600),
        phase: Phase::Waiting,
        notes: None,
    }
}

fn keep(feedback: &mut Feedback, target: &FeedbackKey, text: &str) {
    feedback.keep(
        Batch {
            target: target.clone(),
            text: text.into(),
        },
        None,
    );
}

fn offered(pending: &mut Pending, feedback: &mut Feedback, now: Instant) -> String {
    let mut wire = String::new();
    let (send, recv) = std::sync::mpsc::sync_channel(1);
    assert!(
        pending.advance(Some("feedback-600"), now, feedback, |text| {
            wire = text;
            Some(recv)
        })
    );
    assert!(!feedback.has(&pending.target));
    send.send(Ok(())).unwrap();
    assert!(
        pending.advance(Some("feedback-600"), now, feedback, |_| panic!(
            "second write"
        ))
    );
    wire
}

#[test]
fn notes_return_to_the_exact_host_and_terminal_only_after_ack() {
    let now = Instant::now();
    let mut request = pending("ssh:one", now);
    let other = pending("ssh:two", now);
    let mut feedback = Feedback::default();
    keep(&mut feedback, &other.target, "other host");
    assert!(
        request.advance(Some("feedback-600"), now, &mut feedback, |_| panic!(
            "no notes here"
        ))
    );
    keep(&mut feedback, &request.target, "café\nline two");
    let reply: serde_json::Value =
        serde_json::from_str(&offered(&mut request, &mut feedback, now)).unwrap();
    assert_eq!(reply["notes"], "café\nline two");
    assert!(
        request.advance(Some("feedback-600"), now, &mut feedback, |_| panic!(
            "no replay"
        ))
    );
    assert!(!request.advance(Some("ack"), now, &mut feedback, |_| panic!("no replay")));
    assert!(!feedback.has(&request.target));
    assert_eq!(feedback.take(&other.target).as_deref(), Some("other host"));
}

#[test]
fn cancelled_disconnected_or_unacknowledged_notes_return_before_newer_notes() {
    for (value, elapsed) in [(None, Duration::ZERO), (Some("feedback-600"), ACK_WAIT)] {
        let now = Instant::now();
        let mut request = pending("cloud:one", now);
        let mut feedback = Feedback::default();
        keep(&mut feedback, &request.target, "old");
        offered(&mut request, &mut feedback, now);
        keep(&mut feedback, &request.target, "new");
        assert!(!request.advance(value, now + elapsed, &mut feedback, |_| panic!("no replay")));
        assert_eq!(feedback.take(&request.target).as_deref(), Some("old\nnew"));
    }
}

#[test]
fn a_failed_writer_or_early_ack_never_consumes_notes() {
    let now = Instant::now();
    for early_ack in [false, true] {
        let mut request = pending("wsl:Ubuntu", now);
        let mut feedback = Feedback::default();
        keep(&mut feedback, &request.target, "keep me");
        let (send, recv) = std::sync::mpsc::sync_channel(1);
        assert!(request.advance(Some("feedback-600"), now, &mut feedback, |_| Some(recv)));
        drop(send);
        assert!(!request.advance(
            if early_ack { Some("ack") } else { None },
            now,
            &mut feedback,
            |_| panic!("no retry")
        ));
        assert_eq!(feedback.take(&request.target).as_deref(), Some("keep me"));
    }
}

#[test]
fn expired_wait_returns_empty_and_cancelled_wait_never_takes_notes() {
    let now = Instant::now();
    let mut request = pending("ssh:one", now);
    let mut feedback = Feedback::default();
    let reply: serde_json::Value = serde_json::from_str(&offered(
        &mut request,
        &mut feedback,
        now + Duration::from_secs(600),
    ))
    .unwrap();
    assert!(reply["notes"].is_null());
    let mut request = pending("ssh:one", now);
    keep(&mut feedback, &request.target, "stay");
    assert!(!request.advance(None, now, &mut feedback, |_| panic!("cancelled")));
    assert!(feedback.has(&request.target));
}

#[test]
fn an_unacknowledged_reply_retains_and_restores_each_pages_pending_marker() {
    use std::sync::Arc;
    let now = Instant::now();
    let mut request = pending("ssh:one", now);
    let mut feedback = Feedback::default();
    let marker = Arc::new(());
    let weak = Arc::downgrade(&marker);
    feedback.keep(
        Batch {
            target: request.target.clone(),
            text: "withdraw me".into(),
        },
        Some(marker),
    );
    offered(&mut request, &mut feedback, now);
    assert_eq!(
        weak.strong_count(),
        1,
        "the in-flight reply retains the pending send"
    );
    assert!(!request.advance(None, now, &mut feedback, |_| panic!("cancelled")));
    assert!(feedback.holds(&weak));
    assert!(feedback.withdraw(&weak));
    assert!(!feedback.has(&request.target));
    assert_eq!(weak.strong_count(), 0);
}

mod retire;
