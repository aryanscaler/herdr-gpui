use super::*;
use crate::{sidebar::layout_tests, state::StatusRecency};
use sheprd_client::ClientEvent;
use sheprd_session::{Keeper, Tab, Workspace};
use std::sync::Arc;

/// A connected host that just reported `agents`, its later ones changed last.
fn host(agents: &[(&str, AgentStatus)]) -> LiveState {
    let mut snapshot = layout_tests::snapshot(1);
    let template = snapshot.agents[0].clone();
    snapshot.agents = (1..)
        .zip(agents)
        .map(|(sequence, &(pane, agent_status))| ClientShellAgent {
            pane_id: pane.into(),
            agent_status,
            state_change_seq: sequence,
            ..template.clone()
        })
        .collect();
    let mut live = LiveState::default();
    live.apply(ClientEvent::Snapshot(Arc::new(snapshot)));
    live
}

fn agent(terminal: &str, status: AgentStatus, tab: &str) -> Agent {
    Agent {
        focused: false,
        identity: Some("claude".into()),
        name: "claude".into(),
        pane: terminal.into(),
        status,
        tab: tab.into(),
        workspace: "kept".into(),
    }
}

fn session(agents: Vec<Agent>, tabs: &[&str]) -> Session {
    Session {
        agents,
        focused: Some("kept".into()),
        workspaces: vec![Workspace {
            ahead_behind: None,
            branch: None,
            cwd: "/src/api".into(),
            custom_label: false,
            id: "kept".into(),
            keeper: Keeper::App,
            label: "api".into(),
            status: AgentStatus::Unknown,
            tabs: tabs
                .iter()
                .map(|tab| Tab {
                    focused: false,
                    id: (*tab).into(),
                    label: format!("tab {tab}"),
                    status: AgentStatus::Unknown,
                })
                .collect(),
            tokens: Vec::new(),
            worktree: None,
        }],
    }
}

fn core<'a>(
    index: usize,
    session: &'a Session,
    recency: &'a StatusRecency,
    answers: bool,
) -> CoreAgents<'a> {
    CoreAgents {
        index,
        answers,
        agents: session
            .agents
            .iter()
            .map(|agent| CoreAgent {
                agent,
                session,
                snapshot: None,
                info: None,
                focused: false,
            })
            .collect(),
        recency: Some(recency),
    }
}

fn ids(rows: &[(usize, Listed<'_>)]) -> Vec<(usize, String)> {
    rows.iter()
        .map(|(index, agent)| (*index, agent.id().to_owned()))
        .collect()
}

#[test]
fn grouped_lists_herdr_2_agents_after_their_hosts_own() {
    let local = host(&[("p1", AgentStatus::Idle)]);
    let remote = host(&[("r1", AgentStatus::Blocked)]);
    let session = session(vec![agent("t1", AgentStatus::Working, "1")], &["1"]);
    let recency = StatusRecency::default();
    // The remote host is listed first in the sidebar.
    let rows = listed_agents(
        vec![(1, &remote), (0, &local)],
        Some(core(0, &session, &recency, true)),
        AgentSort::Grouped,
    );
    assert_eq!(
        ids(&rows),
        [(1, "r1".into()), (0, "p1".into()), (0, "t1".into())]
    );
    let rows = listed_agents(
        vec![(0, &local), (1, &remote)],
        Some(core(0, &session, &recency, true)),
        AgentSort::Grouped,
    );
    assert_eq!(
        ids(&rows),
        [(0, "p1".into()), (0, "t1".into()), (1, "r1".into())]
    );
    assert!(rows[1].1.is_core());
}

#[test]
fn priority_merges_herdr_2_agents_by_attention() {
    let local = host(&[("p1", AgentStatus::Idle), ("p2", AgentStatus::Working)]);
    let session = session(vec![agent("t1", AgentStatus::Blocked, "1")], &["1"]);
    let mut recency = StatusRecency::default();
    recency.observe([("t1", AgentStatus::Blocked)]);
    let rows = listed_agents(
        vec![(0, &local)],
        Some(core(0, &session, &recency, true)),
        AgentSort::Priority,
    );
    assert_eq!(
        ids(&rows),
        [(0, "t1".into()), (0, "p2".into()), (0, "p1".into())]
    );
    // A core that does not answer lists its agents after connected hosts'.
    let rows = listed_agents(
        vec![(0, &local)],
        Some(core(0, &session, &recency, false)),
        AgentSort::Priority,
    );
    assert_eq!(rows.last().map(|(_, agent)| agent.id()), Some("t1"));
}

#[test]
fn herdr_2_agents_are_listed_only_with_their_host() {
    let remote = host(&[("r1", AgentStatus::Idle)]);
    let session = session(vec![agent("t1", AgentStatus::Working, "1")], &["1"]);
    let recency = StatusRecency::default();
    let rows = listed_agents(
        vec![(1, &remote)],
        Some(core(0, &session, &recency, true)),
        AgentSort::Grouped,
    );
    assert_eq!(ids(&rows), [(1, "r1".into())]);
    // With no Herdr 1 at all, the core's agents still list.
    let none = LiveState::default();
    let rows = listed_agents(
        vec![(0, &none)],
        Some(core(0, &session, &recency, true)),
        AgentSort::Priority,
    );
    assert_eq!(ids(&rows), [(0, "t1".into())]);
}

#[test]
fn a_herdr_2_agent_is_placed_in_its_workspace_and_tab() {
    let one = session(vec![agent("t1", AgentStatus::Working, "1")], &["1"]);
    let two = session(vec![agent("t1", AgentStatus::Working, "2")], &["1", "2"]);
    let recency = StatusRecency::default();
    let place = |session: &Session| {
        let core = core(0, session, &recency, true);
        core.agents[0]
            .place()
            .map(|(w, t)| (w.to_owned(), t.map(str::to_owned)))
    };
    assert_eq!(place(&one), Some(("api".into(), None)));
    assert_eq!(place(&two), Some(("api".into(), Some("tab 2".into()))));
    let target = Listed::Core(core(0, &two, &recency, true).agents[0]).target();
    assert_eq!(
        target,
        Target::Core {
            workspace: "kept".into(),
            tab: "2".into(),
            terminal: "t1".into(),
        }
    );
}

#[test]
fn plugin_values_leave_out_the_labels_sheprd_sets() {
    let session = session(vec![agent("t1", AgentStatus::Working, "1")], &["1"]);
    let info = TerminalInfo {
        agent: AgentStatus::Working,
        cwd: None,
        foreground: None,
        id: sheprd_backend::TerminalId::new("t1").unwrap(),
        labels: [
            ("acp", "abcd"),
            ("sheprd.workspace", "kept"),
            ("ticket", "ENG-42"),
        ]
        .into_iter()
        .map(|(name, value)| (name.to_owned(), value.to_owned()))
        .collect(),
        name: None,
        reporter: None,
        state: sheprd_backend::RunState::Running,
        title: Some("Fix the login".into()),
    };
    let recency = StatusRecency::default();
    let mut listed = core(0, &session, &recency, true);
    listed.agents[0].info = Some(&info);
    let agent = listed.agents[0];
    assert_eq!(
        agent.plugin_values().collect::<Vec<_>>(),
        [("ticket", "ENG-42")]
    );
    assert_eq!(agent.pane().as_deref(), Some("Fix the login"));
    let report = crate::plugin_values::host_report_with_core("Local", None, &listed.agents);
    assert_eq!(report.reported.len(), 1);
    assert_eq!(report.reported[0].name, "ticket");
}

#[test]
fn a_herdr_2_agent_changes_on_the_shared_clock() {
    let mut recency = StatusRecency::default();
    recency.observe([("t1", AgentStatus::Working), ("t2", AgentStatus::Idle)]);
    let (t1, t2) = (recency.of("t1"), recency.of("t2"));
    assert!(t1 > 0 && t2 > t1);
    recency.observe([("t1", AgentStatus::Working), ("t2", AgentStatus::Idle)]);
    assert_eq!((recency.of("t1"), recency.of("t2")), (t1, t2));
    recency.observe([("t1", AgentStatus::Done)]);
    assert!(recency.of("t1") > t2);
    assert_eq!(recency.of("t2"), 0);
}

#[test]
fn every_hosts_herdr_2_agents_are_listed_with_their_host() {
    let local = host(&[("p1", AgentStatus::Idle)]);
    let remote = host(&[("r1", AgentStatus::Idle)]);
    let here = session(vec![agent("t1", AgentStatus::Working, "1")], &["1"]);
    // A saved host's core numbers its terminals on its own: IDs repeat.
    let there = session(vec![agent("t1", AgentStatus::Blocked, "1")], &["1"]);
    let (near, far) = (StatusRecency::default(), StatusRecency::default());
    let rows = listed_agents(
        vec![(0, &local), (1, &remote)],
        vec![core(0, &here, &near, true), core(1, &there, &far, true)],
        AgentSort::Grouped,
    );
    assert_eq!(
        ids(&rows),
        [
            (0, "p1".into()),
            (0, "t1".into()),
            (1, "r1".into()),
            (1, "t1".into())
        ]
    );
    let rows = listed_agents(
        vec![(0, &local), (1, &remote)],
        vec![core(0, &here, &near, true), core(1, &there, &far, true)],
        AgentSort::Priority,
    );
    // The remote host's blocked agent leads.
    assert_eq!(rows[0].0, 1);
    assert_eq!(rows[0].1.status(), AgentStatus::Blocked);
}
