use super::*;

/// An integration's own label for `unknown` is the state text of an agent
/// that reports no status, as before the rows took Herdr 2 agents too.
#[test]
fn an_unknown_status_keeps_its_integrations_label() {
    let mut snapshot = layout_tests::snapshot(1);
    snapshot.agents[0].agent_status = AgentStatus::Unknown;
    snapshot.agents[0].state_labels = vec![
        ("unknown".into(), "resting".into()),
        ("idle".into(), "waiting".into()),
    ];
    let agents = agent_layout(r#"rows = [["state_text"]]"#);
    let rows = agent_rows(&agents, &snapshot.agents[0], &snapshot, None).unwrap();
    assert_eq!(texts(&rows), [["resting"]]);
}
