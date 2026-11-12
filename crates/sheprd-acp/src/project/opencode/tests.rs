#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::*;
use serde_json::json;
use sheprd_chat_protocol::{EntryKind, MAX_ACTIVITIES};

fn projector() -> Projector {
    Projector {
        opencode_children: true,
        ..Projector::default()
    }
}

fn event(child: &str, payload: Value) -> Value {
    let mut value = json!({
        "rootSessionId": "root", "childSessionId": child,
        "parentSessionId": "root", "depth": 1, "title": format!("Agent {child}")
    });
    value
        .as_object_mut()
        .unwrap()
        .extend(payload.as_object().unwrap().clone());
    value
}

fn send(p: &mut Projector, t: &mut Transcript, value: Value, now: u64) {
    for change in p.opencode_child(t, "root", value, now) {
        t.apply(&change).unwrap();
    }
}

fn status(p: &mut Projector, t: &mut Transcript, child: &str, status: &str, now: u64) {
    send(
        p,
        t,
        event(child, json!({"type":"status", "status":status})),
        now,
    );
}

#[test]
fn three_background_children_outlive_parent_turn_and_finish_independently() {
    let (mut p, mut t) = (projector(), Transcript::default());
    for id in ["a", "b", "c"] {
        status(&mut p, &mut t, id, "created", 10);
        status(&mut p, &mut t, id, "running", 11);
    }
    // Launch completion describes dispatch, never the child execution.
    super::super::tests::apply(
        &mut p,
        &mut t,
        json!({
            "sessionUpdate":"tool_call", "toolCallId":"launch", "title":"subagent",
            "rawInput":{"agent":"general", "prompt":"Look", "background":true},
            "status":"completed"
        }),
    );
    assert_eq!(t.activities.len(), 3);
    assert!(p.direct_turn_ended(&t, 20).is_empty());
    assert!(t.activities.iter().all(|a| a.is_running()));
    status(&mut p, &mut t, "b", "completed", 30);
    send(
        &mut p,
        &mut t,
        event(
            "a",
            json!({"type":"status", "status":"failed",
        "error":{"type":"provider.error", "message":"Provider failed"}}),
        ),
        40,
    );
    assert!(t.activity("opencode:c").unwrap().is_running());
    status(&mut p, &mut t, "c", "interrupted", 50);
    for (id, state, end) in [
        ("a", ActivityState::Failed, 40),
        ("b", ActivityState::Finished, 30),
        ("c", ActivityState::Killed, 50),
    ] {
        let activity = t.activity(&activity_id(id)).unwrap();
        assert_eq!(activity.state, state);
        assert_eq!(activity.started_ms, 10);
        assert_eq!(activity.finished_ms, Some(end));
        assert!(!activity.stoppable);
    }
    assert_eq!(
        t.activity("opencode:a").unwrap().detail.as_deref(),
        Some("Provider failed")
    );
    status(&mut p, &mut t, "b", "created", 60);
    status(&mut p, &mut t, "b", "completed", 70);
    assert_eq!(t.activity("opencode:b").unwrap().finished_ms, Some(30));
}

#[test]
fn concurrent_children_use_session_identity_even_for_identical_tool_ids() {
    let (mut p, mut t) = (projector(), Transcript::default());
    for child in ["a", "b"] {
        status(&mut p, &mut t, child, "created", 1);
        send(
            &mut p,
            &mut t,
            event(
                child,
                json!({"type":"update", "update": {
                    "sessionUpdate":"tool_call", "toolCallId":format!("{child}:read"),
                    "title":format!("Read {child}"), "status":"in_progress"
                }}),
            ),
            2,
        );
    }
    assert!(p.direct_turn_ended(&t, 3).is_empty());
    status(&mut p, &mut t, "a", "completed", 4);
    send(
        &mut p,
        &mut t,
        event(
            "b",
            json!({"type":"update", "update": {
                "sessionUpdate":"tool_call_update", "toolCallId":"b:read", "status":"completed"
            }}),
        ),
        5,
    );
    for entry in &t.entries {
        let EntryKind::Tool(tool) = &entry.kind else {
            panic!("expected tool")
        };
        if tool.kind == ToolKind::Subagent {
            continue;
        }
        assert_eq!(
            tool.parent,
            Some(activity_id(tool.call_id.split(':').next().unwrap()))
        );
    }
    assert_eq!(t.entries.len(), 4);
    assert!(t.activity("opencode:b").unwrap().is_running());
    // A mismatched call namespace cannot overwrite another child's step.
    let before = t.entries.clone();
    send(
        &mut p,
        &mut t,
        event(
            "b",
            json!({"type":"update", "update": {
                "sessionUpdate":"tool_call_update", "toolCallId":"a:read", "title":"wrong"
            }}),
        ),
        6,
    );
    assert_eq!(t.entries, before);
}

#[test]
fn child_plans_text_and_settings_do_not_change_parent_conversation() {
    let (mut p, mut t) = (projector(), Transcript::default());
    status(&mut p, &mut t, "a", "created", 1);
    let cards = t.entries.clone();
    for update in [
        json!({"sessionUpdate":"plan", "entries":[]}),
        json!({"sessionUpdate":"agent_message_chunk", "content":{"type":"text","text":"private child"}}),
        json!({"sessionUpdate":"session_info_update", "title":"child title"}),
    ] {
        send(
            &mut p,
            &mut t,
            event("a", json!({"type":"update", "update":update})),
            2,
        );
    }
    assert_eq!(t.entries, cards);
    assert!(t.plan.is_empty());
    assert!(!p.planned);
    // The real V2 tool starts with empty input; its structured update supplies tasks.
    super::super::tests::apply(
        &mut p,
        &mut t,
        json!({"sessionUpdate":"tool_call",
        "toolCallId":"todos", "name":"todowrite", "title":"todowrite", "rawInput":{}}),
    );
    super::super::tests::apply(
        &mut p,
        &mut t,
        json!({"sessionUpdate":"tool_call_update",
        "toolCallId":"todos", "rawInput":{"todos":[
            {"content":"Implement", "status":"in_progress"},
            {"content":"Obsolete", "status":"cancelled"}
        ]}}),
    );
    assert_eq!(t.plan.len(), 1);
    assert_eq!(t.plan[0].content, "Implement");
    assert_eq!(t.entries, cards);
}

#[test]
fn unnegotiated_malformed_foreign_and_oversized_events_are_ignored() {
    let (mut p, mut t) = (Projector::default(), Transcript::default());
    status(&mut p, &mut t, "a", "created", 1);
    assert!(t.activities.is_empty());
    p.opencode_children = true;
    let valid = event("a", json!({"type":"status", "status":"created"}));
    for (key, value) in [
        ("rootSessionId", json!("other")),
        ("depth", json!(0)),
        ("childSessionId", json!("x".repeat(MAX_ACTIVITY_TEXT))),
        ("parentSessionId", json!("other")),
        ("status", json!("unknown")),
    ] {
        let mut invalid = valid.clone();
        invalid[key] = value;
        send(&mut p, &mut t, invalid, 1);
    }
    send(&mut p, &mut t, json!({}), 1);
    status(&mut p, &mut t, "unseen", "completed", 1);
    assert!(t.activities.is_empty());
}

#[test]
fn child_tracking_is_bounded_and_connection_exit_settles_running_cards() {
    let (mut p, mut t) = (projector(), Transcript::default());
    for n in 0..MAX_ACTIVITIES + 20 {
        status(&mut p, &mut t, &n.to_string(), "created", 1);
    }
    assert_eq!(t.activities.len(), MAX_ACTIVITIES);
    assert!(p.direct.is_empty());
    assert!(p.children.is_empty());
    for change in activity::kill_all(&t, 10) {
        t.apply(&change).unwrap();
    }
    assert!(
        t.activities
            .iter()
            .all(|a| a.state == ActivityState::Killed)
    );
}

#[test]
fn explicit_new_execution_reopens_child_but_launch_results_do_not() {
    let (mut p, mut t) = (projector(), Transcript::default());
    status(&mut p, &mut t, "a", "created", 1);
    status(&mut p, &mut t, "a", "completed", 2);
    for background in [false, true] {
        super::super::tests::apply(
            &mut p,
            &mut t,
            json!({
                "sessionUpdate":"tool_call", "toolCallId":format!("launch-{background}"),
                "title":"subagent", "status":"in_progress",
                "rawInput":{"agent":"general", "prompt":"continue", "sessionID":"a", "background":background}
            }),
        );
    }
    assert_eq!(t.activities.len(), 1);
    assert_eq!(
        t.activity("opencode:a").unwrap().state,
        ActivityState::Finished
    );
    status(&mut p, &mut t, "a", "running", 10);
    assert!(t.activity("opencode:a").unwrap().is_running());
    assert_eq!(t.activity("opencode:a").unwrap().started_ms, 10);
    status(&mut p, &mut t, "a", "completed", 20);
    assert_eq!(t.activity("opencode:a").unwrap().finished_ms, Some(20));
}

#[test]
fn nested_child_can_end_after_its_parent_left_retained_history() {
    let (mut p, mut t) = (projector(), Transcript::default());
    let mut nested = event("nested", json!({"type":"status", "status":"created"}));
    nested["parentSessionId"] = json!("old-parent");
    nested["depth"] = json!(2);
    send(&mut p, &mut t, nested.clone(), 1);
    nested["status"] = json!("completed");
    send(&mut p, &mut t, nested, 2);
    assert_eq!(
        t.activity("opencode:nested").unwrap().state,
        ActivityState::Finished
    );
}

#[test]
fn excessive_steps_stay_bounded_without_losing_completion() {
    let (mut p, mut t) = (projector(), Transcript::default());
    status(&mut p, &mut t, "a", "created", 1);
    for n in 0..super::super::MAX_STEPS + 10 {
        send(
            &mut p,
            &mut t,
            event(
                "a",
                json!({"type":"update", "update": {
                    "sessionUpdate":"tool_call", "toolCallId":format!("a:{n}"),
                    "title":format!("step {n}"), "status":"in_progress"
                }}),
            ),
            2,
        );
    }
    let steps = t
        .entries
        .iter()
        .filter(|e| {
            matches!(&e.kind,
        EntryKind::Tool(tool) if tool.parent.as_deref() == Some("opencode:a"))
        })
        .count();
    assert_eq!(steps, super::super::MAX_STEPS);
    status(&mut p, &mut t, "a", "completed", 3);
    assert_eq!(
        t.activity("opencode:a").unwrap().state,
        ActivityState::Finished
    );
}
