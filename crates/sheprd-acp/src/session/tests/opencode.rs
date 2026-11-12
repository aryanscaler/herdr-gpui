use super::*;
use crate::project;
use sheprd_chat_protocol::ActivityState;

#[test]
fn negotiates_child_stream_and_routes_notifications_while_parent_is_idle() {
    let mut h = harness_with(config());
    h.session.start();
    let request = h.last_request("initialize");
    assert_eq!(
        request["params"]["clientCapabilities"]["_meta"][project::opencode::CAPABILITY],
        true
    );
    h.respond(
        request["id"].as_i64().unwrap(),
        json!({"protocolVersion":1,
        "agentCapabilities":{"_meta":{"opencode/child-session-updates":true}}}),
    );
    let request = h.last_request("session/new");
    h.respond(request["id"].as_i64().unwrap(), json!({"sessionId":"s1"}));
    let turn = prompt(&mut h, 1, "Launch a child");
    for (status, end) in [("created", None), ("completed", Some("end_turn"))] {
        if let Some(end) = end {
            h.respond(turn, json!({"stopReason":end}));
            assert!(
                h.transcript()
                    .activity("opencode:child")
                    .unwrap()
                    .is_running()
            );
        }
        h.input(Input::Agent(Inbound::Notification {
            method: project::opencode::METHOD.into(),
            params: json!({"rootSessionId":"s1", "childSessionId":"child",
                "parentSessionId":"s1", "depth":1, "type":"status", "status":status}),
        }));
    }
    assert_eq!(
        h.session
            .transcript
            .activity("opencode:child")
            .unwrap()
            .state,
        ActivityState::Finished
    );
    let turn = prompt(&mut h, 1, "Continue the child");
    h.input(Input::Agent(Inbound::Notification {
        method: project::opencode::METHOD.into(),
        params: json!({"rootSessionId":"s1", "childSessionId":"child",
            "parentSessionId":"s1", "depth":1, "type":"status", "status":"running"}),
    }));
    h.respond(turn, json!({"stopReason":"cancelled"}));
    assert!(
        h.transcript()
            .activity("opencode:child")
            .unwrap()
            .is_running()
    );
    h.input(Input::AgentExited {
        code: Some(0),
        stderr: Vec::new(),
    });
    assert_eq!(
        h.transcript().activity("opencode:child").unwrap().state,
        ActivityState::Killed
    );
}
