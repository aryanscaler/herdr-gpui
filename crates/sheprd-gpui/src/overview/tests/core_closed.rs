//! Herdr 2 agents that closed: noted from the window's herdr-core tabs, and
//! started again as a new herdr-core tab's program.

use super::*;
use crate::{
    browser::TabId,
    overview::{
        closed::{Closed, Listed},
        core::ENDPOINT,
        reopen,
    },
    teleport::AgentKind,
};
use sheprd_backend::TerminalId;

fn core_row(terminal: &str, identity: &str) -> Row {
    let mut row = row(terminal, State::Working);
    row.key = Key::Agent {
        endpoint: ENDPOINT.into(),
        checkout: "/src/api".into(),
        agent: identity.into(),
    };
    row.identity = Some(identity.into());
    row.open = Open::Core {
        tab: TabId::test(3),
        terminal: TerminalId::new(terminal).unwrap(),
    };
    row
}

fn listed(row: &Row) -> Listed {
    Listed {
        row: row.clone(),
        cwd: Some("/src/api".into()),
        workspace: Some("kept-1".into()),
    }
}

#[test]
fn a_herdr_2_agent_gone_from_the_tabs_is_closed_with_its_workspace() {
    let mut closed = Closed::default();
    let claude = core_row("t1", "claude");
    let codex = core_row("t2", "codex");
    closed.observe_core(vec![listed(&claude), listed(&codex)], at(10), |_| false);
    assert!(closed.is_empty());
    closed.observe_core(vec![listed(&codex)], at(20), |_| false);
    let agent = closed.agents().next().unwrap();
    assert_eq!(closed.len(), 1);
    assert_eq!(agent.host, ENDPOINT);
    assert_eq!(agent.row.identity.as_deref(), Some("claude"));
    assert_eq!(agent.workspace.as_deref(), Some("kept-1"));
    assert_eq!(agent.cwd.as_deref(), Some("/src/api"));
    assert_eq!(reopen::kind(agent), Some(AgentKind::Claude));
}

#[test]
fn a_herdr_2_agent_still_running_elsewhere_did_not_close() {
    let mut closed = Closed::default();
    let claude = core_row("t1", "claude");
    closed.observe_core(vec![listed(&claude)], at(10), |_| false);
    // Its tab moved to another window, which still drives the terminal.
    closed.observe_core(Vec::new(), at(20), |row| {
        row.identity.as_deref() == Some("claude")
    });
    assert!(closed.is_empty());
}

#[test]
fn a_herdr_2_agent_that_exits_to_its_shell_is_closed() {
    let mut closed = Closed::default();
    let claude = core_row("t1", "claude");
    closed.observe_core(vec![listed(&claude)], at(10), |_| false);
    let mut shell = core_row("t1", "codex");
    shell.identity = Some("codex".into());
    closed.observe_core(vec![listed(&shell)], at(20), |_| false);
    assert_eq!(closed.len(), 1);
    // Herdr 1's hosts keep their own records.
    closed.retain_hosts(|host| host == ENDPOINT);
    assert_eq!(closed.len(), 1);
    closed.retain_hosts(|host| host == "local");
    assert!(closed.is_empty());
}

#[test]
fn a_reopened_herdr_2_agent_returning_takes_no_twin_off_the_list() {
    let mut closed = Closed::default();
    let first = core_row("t1", "claude");
    let second = core_row("t2", "claude");
    closed.observe_core(vec![listed(&first), listed(&second)], at(10), |_| false);
    closed.observe_core(Vec::new(), at(20), |_| false);
    assert_eq!(closed.len(), 2);
    let id = closed.agents().next().unwrap().id;
    closed.reopened(id);
    assert_eq!(closed.len(), 1);
    // The reopened agent starts in a new terminal.
    let again = core_row("t3", "claude");
    closed.observe_core(vec![listed(&again)], at(30), |_| false);
    assert_eq!(closed.len(), 1);
}

#[test]
fn a_herdr_2_agent_starts_again_as_its_program() {
    assert_eq!(reopen::argv(AgentKind::Claude), ["claude", "--continue"]);
    assert_eq!(
        reopen::argv(AgentKind::Codex),
        ["codex", "resume", "--last"]
    );
    assert_eq!(
        reopen::command_line(AgentKind::Codex),
        "codex resume --last"
    );
}

#[test]
fn a_herdr_2_agent_being_reopened_stays_listed_until_it_is_back() {
    let mut closed = Closed::default();
    let first = core_row("t1", "claude");
    let second = core_row("t2", "claude");
    closed.observe_core(vec![listed(&first), listed(&second)], at(10), |_| false);
    closed.observe_core(Vec::new(), at(20), |_| false);
    // The older of the two is reopened.
    let id = closed.agents().last().unwrap().id;
    let asked = std::time::Instant::now();
    closed.reopening(id, asked);
    // Another Reopen waits for this start, but not forever.
    assert!(closed.is_reopening(id, asked));
    assert!(!closed.is_reopening(id, asked + Duration::from_secs(60)));
    // Its start failed: nothing turns up, and it can be tried again.
    closed.observe_core(Vec::new(), at(30), |_| false);
    assert!(closed.get(id).is_some());
    // It is back: it, not its twin, leaves the list.
    let again = core_row("t3", "claude");
    closed.observe_core(vec![listed(&again)], at(40), |_| false);
    assert!(closed.get(id).is_none());
    assert_eq!(closed.len(), 1);
}
