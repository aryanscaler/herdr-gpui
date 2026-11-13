use super::*;

#[test]
fn an_agent_process_or_a_different_conversation_is_not_arrival() {
    for agent in AgentKind::ALL.into_iter().filter(|agent| agent.carries()) {
        let reference = if matches!(agent, AgentKind::Pi | AgentKind::Omp) {
            "/destination/session.jsonl"
        } else {
            "session-1"
        };
        let expected = Resume::new(agent, reference.into());
        assert!(!expected.matches(None));
        let session = AgentSession {
            agent: agent.name().into(),
            kind: expected.kind,
            source: format!("herdr:{}", agent.name()),
            value: reference.into(),
        };
        assert!(expected.matches(Some(&session)));
        let mut wrong = session.clone();
        wrong.value.push_str("-other");
        assert!(!expected.matches(Some(&wrong)));
        wrong = session;
        wrong.source = "plugin:unverified".into();
        assert!(!expected.matches(Some(&wrong)));
    }
}
