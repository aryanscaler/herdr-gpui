use super::*;
use serde_json::json;

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod conversion;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod reporting;

#[test]
fn session_events_are_explicit_and_not_prompts() {
    for agent in [AgentKind::Claude, AgentKind::Codex, AgentKind::Copilot] {
        let event = json!({"hook_event_name":"SessionStart", "session_id":"exact-123", "transcript_path":"/log"});
        assert_eq!(
            selection(agent, &event).unwrap().unwrap().value,
            "exact-123"
        );
        if agent != AgentKind::Copilot {
            assert!(selection(agent, &json!({"session_id":"guessed"})).is_err());
        }
        assert!(
            selection(
                agent,
                &json!({"hook_event_name":"UserPromptSubmit", "session_id":"other"})
            )
            .is_err()
        );
    }
    assert!(
        selection(
            AgentKind::Codex,
            &json!({"hook_event_name":"SessionStart", "session_id":"x"})
        )
        .is_err()
    );
}

#[test]
fn native_hook_payloads_without_event_names_are_supported_without_prompt_inference() {
    assert_eq!(
        selection(
            AgentKind::Copilot,
            &json!({"session_id":"", "sessionId":"exact"})
        )
        .unwrap()
        .unwrap()
        .value,
        "exact"
    );
    assert!(
        selection(
            AgentKind::Copilot,
            &json!({"sessionId":"exact", "prompt":"not a start"})
        )
        .is_err()
    );
    assert!(
        selection(
            AgentKind::Codex,
            &json!({"session_id":"exact", "transcript_path":"/exact/log"})
        )
        .is_ok()
    );
    assert!(
        selection(
            AgentKind::Codex,
            &json!({"session_id":"exact", "transcript_path":"/exact/log", "tool_name":"tool"})
        )
        .is_err()
    );
}

#[test]
fn home_clears_an_opencode_selection() {
    assert!(
        selection(AgentKind::Opencode, &json!({"session_id":null}))
            .unwrap()
            .is_none()
    );
    assert!(selection(AgentKind::Opencode, &json!({})).is_err());
    assert!(
        selection(
            AgentKind::Claude,
            &json!({"hook_event_name":"SessionStart", "session_id":"child", "agent_id":"nested"})
        )
        .is_err()
    );
}

#[test]
fn carried_sessions_use_explicit_paths_and_qualified_letta_defaults() {
    for agent in [AgentKind::Pi, AgentKind::Omp] {
        let selected = selection(agent, &json!({"session_path":"/sessions/exact.jsonl"}))
            .unwrap()
            .unwrap();
        assert_eq!(selected.kind, SessionKind::Path);
        assert!(selection(agent, &json!({"session_path":"relative.jsonl"})).is_err());
        assert!(
            selection(agent, &json!({"session_path":null}))
                .unwrap()
                .is_none()
        );
    }
    let selected = selection(
        AgentKind::Letta,
        &json!({"conversation_id":"default","agent_id":"agent-123"}),
    )
    .unwrap()
    .unwrap();
    assert_eq!(selected.value, "default:agent-123");
    assert!(selection(AgentKind::Letta, &json!({"conversation_id":"default"})).is_err());
}
