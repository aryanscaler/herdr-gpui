use super::*;
use sheprd_client::protocol::AgentStatus;
use sheprd_session::Agent;

fn source(status: AgentStatus) -> Source {
    Source {
        host: Some("build".into()),
        session: Some("work".into()),
        herdr_1: None,
        core: Some(Arc::new(Session {
            agents: vec![Agent {
                focused: false,
                identity: Some("claude".into()),
                name: "Doing tests".into(),
                pane: "t9".into(),
                status,
                tab: "tab".into(),
                workspace: "kept".into(),
            }],
            ..Session::default()
        })),
    }
}

#[test]
fn core_status_changes_and_disconnects_without_a_herdr_1_snapshot() {
    let mut cache = LiveCache::default();
    cache.update(vec![source(AgentStatus::Working)]);
    assert_eq!(cache.agents[0].terminals, Terminals::Herdr2);
    assert!(cache.agents[0].workspace_id.is_empty());
    assert_eq!(cache.agents[0].pane_id, "t9");
    assert_eq!(cache.agents[0].host.as_deref(), Some("build"));
    assert_eq!(cache.agents[0].session.as_deref(), Some("work"));
    cache.update(vec![source(AgentStatus::Blocked)]);
    assert_eq!(cache.agents[0].status, AgentStatus::Blocked);
    let mut disconnected = source(AgentStatus::Working);
    disconnected.core = None;
    cache.update(vec![disconnected]);
    assert!(cache.agents.is_empty());
}

#[test]
fn unchanged_inputs_reuse_the_projection_but_host_changes_do_not() {
    let mut cache = LiveCache::default();
    let first = source(AgentStatus::Working);
    let mut next = source(AgentStatus::Working);
    next.core = first.core.clone();
    cache.update(vec![first]);
    let agents = cache.agents.clone();
    cache.update(vec![next]);
    assert!(Arc::ptr_eq(&agents, &cache.agents));
    let mut changed = source(AgentStatus::Working);
    changed.core = cache.from[0].core.clone();
    changed.host = Some("another".into());
    cache.update(vec![changed]);
    assert!(!Arc::ptr_eq(&agents, &cache.agents));
    assert_eq!(cache.agents[0].host.as_deref(), Some("another"));
}

#[test]
fn mixed_backends_keep_their_statuses_and_core_cannot_impersonate_a_workspace() {
    let mut from = source(AgentStatus::Blocked);
    let snapshot = crate::sidebar::layout_tests::snapshot(2);
    let expected = snapshot.agents[0].pane_id.clone();
    from.herdr_1 = Some(Arc::new(snapshot));
    let mut cache = LiveCache::default();
    cache.update(vec![from]);
    assert!(
        cache
            .agents
            .iter()
            .any(|agent| { agent.pane_id == expected && agent.terminals == Terminals::Herdr1 })
    );
    assert!(cache.agents.iter().any(|agent| {
        agent.terminals == Terminals::Herdr2 && agent.status == AgentStatus::Blocked
    }));
    assert!(
        cache
            .branches
            .iter()
            .all(|branch| branch.workspace_id != "kept")
    );
}
