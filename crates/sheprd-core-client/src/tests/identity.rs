//! What a terminal's record says about the program in it: the name its status
//! reports give, and the process in front.

use super::terminal;
use crate::{backend::info, wire::Terminal};
use serde_json::json;
use sheprd_backend::Process;
use sheprd_client::herdr::AgentKind;

fn record(value: serde_json::Value) -> Terminal {
    serde_json::from_value(value).unwrap()
}

#[test]
fn the_reported_app_and_the_foreground_process_carry_over() {
    let mut value = terminal("t1");
    value["status"] = json!({"state": "idle", "app": " claude-code "});
    value["foreground"] =
        json!({"pid": 42, "name": "node", "argv": ["node", "/bin/codex"], "cwd": "/w"});
    let info = info(&record(value)).unwrap();
    assert_eq!(info.reporter.as_deref(), Some("claude-code"));
    assert_eq!(
        info.foreground,
        Some(Process {
            argv: vec!["node".into(), "/bin/codex".into()],
            name: "node".into(),
            pid: 42,
        })
    );
}

#[test]
fn a_blank_app_and_an_impossible_pid_are_left_out() {
    let mut value = terminal("t1");
    value["status"] = json!({"state": "working", "app": "  "});
    value["foreground"] = json!({"pid": -1, "name": "sh", "argv": [], "cwd": ""});
    let info = info(&record(value)).unwrap();
    assert_eq!((info.reporter, info.foreground), (None, None));
}

#[test]
fn a_program_that_never_reported_has_no_reporter() {
    let info = info(&record(terminal("t1"))).unwrap();
    assert_eq!((info.reporter, info.foreground), (None, None));
}

#[test]
fn the_agent_is_named_by_its_report_else_its_process() {
    let mut value = terminal("t1");
    value["foreground"] =
        json!({"pid": 7, "name": "node", "argv": ["node", "/x/codex"], "cwd": ""});
    let by_process = info(&record(value.clone())).unwrap();
    assert_eq!(crate::agent_kind(&by_process), Some(AgentKind::Codex));
    value["status"] = json!({"state": "idle", "app": "claude-code"});
    let by_report = info(&record(value)).unwrap();
    assert_eq!(crate::agent_kind(&by_report), Some(AgentKind::Claude));
    assert_eq!(
        crate::agent_kind(&info(&record(terminal("t2"))).unwrap()),
        None
    );
}

#[test]
fn an_unknown_program_that_reports_a_status_is_named_by_its_report() {
    let mut value = terminal("t1");
    value["status"] = json!({"state": "idle", "app": "my\u{202e}bot"});
    assert_eq!(
        crate::agent_name(&info(&record(value.clone())).unwrap()).as_deref(),
        Some("mybot")
    );
    value["status"] = json!({"state": "idle", "app": "claude-code"});
    assert_eq!(
        crate::agent_name(&info(&record(value)).unwrap()).as_deref(),
        Some("claude")
    );
    // A shell that never reported is no agent.
    assert_eq!(
        crate::agent_name(&info(&record(terminal("t2"))).unwrap()),
        None
    );
}

#[test]
fn a_program_that_is_gone_is_no_agent() {
    let mut value = terminal("t1");
    value["status"] = json!({"state": "idle", "app": "claude-code"});
    let running = info(&record(value)).unwrap();
    assert_eq!(crate::agent_kind(&running), Some(AgentKind::Claude));
    // The core keeps the last report once the program is lost or exits.
    for state in [
        sheprd_backend::RunState::Lost,
        sheprd_backend::RunState::Exited(sheprd_backend::Exit::default()),
    ] {
        let gone = sheprd_backend::TerminalInfo {
            state,
            ..running.clone()
        };
        assert_eq!(crate::agent_kind(&gone), None, "{state:?}");
        assert_eq!(crate::agent_name(&gone), None, "{state:?}");
    }
}

#[test]
fn an_agent_that_quit_to_its_shell_is_no_agent() {
    let mut value = terminal("t1");
    value["argv"] = json!(["zsh"]);
    value["pid"] = json!(100);
    value["status"] = json!({"state": "idle", "app": "claude-code"});
    value["foreground"] = json!({"pid": 200, "name": "claude", "argv": ["claude"], "cwd": ""});
    let running = info(&record(value.clone())).unwrap();
    assert_eq!(crate::agent_kind(&running), Some(AgentKind::Claude));
    // Claude quit: the shell is back in front, holding its last report.
    value["foreground"] = json!({"pid": 100, "name": "zsh", "argv": ["zsh"], "cwd": ""});
    let quit = info(&record(value.clone())).unwrap();
    assert_eq!(crate::agent_name(&quit), None);
    assert_eq!(quit.agent, sheprd_backend::AgentStatus::Unknown);
    // A terminal started as the agent itself keeps it.
    value["argv"] = json!(["claude"]);
    value["foreground"] = json!({"pid": 100, "name": "claude", "argv": ["claude"], "cwd": ""});
    let own = info(&record(value.clone())).unwrap();
    assert_eq!(crate::agent_kind(&own), Some(AgentKind::Claude));
    // The shell became the agent (`exec claude`): same pid, the agent in front.
    value["argv"] = json!(["zsh"]);
    value["foreground"] = json!({"pid": 100, "name": "claude", "argv": ["claude"], "cwd": ""});
    let exec = info(&record(value.clone())).unwrap();
    assert_eq!(crate::agent_kind(&exec), Some(AgentKind::Claude));
    assert_eq!(exec.agent, sheprd_backend::AgentStatus::Idle);
    // Claude quit and the shell became Codex, which has not reported yet:
    // Claude's report is stale, so Codex is named by its process.
    value["foreground"] = json!({"pid": 100, "name": "codex", "argv": ["codex"], "cwd": ""});
    let codex = info(&record(value.clone())).unwrap();
    assert_eq!(crate::agent_kind(&codex), Some(AgentKind::Codex));
    assert_eq!(codex.agent, sheprd_backend::AgentStatus::Unknown);
    // A report from a program Sheprd does not know stands.
    value["argv"] = json!(["zsh"]);
    value["foreground"] = json!({"pid": 100, "name": "zsh", "argv": ["zsh"], "cwd": ""});
    value["status"] = json!({"state": "done", "app": "zsh-integration"});
    let shell = info(&record(value)).unwrap();
    assert_eq!(shell.agent, sheprd_backend::AgentStatus::Done);
}
