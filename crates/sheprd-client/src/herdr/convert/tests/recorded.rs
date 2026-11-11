//! A Claude session found in Claude Code's own record, for a pane Herdr's
//! integration reports none for: moving an agent is Sheprd's feature.

use super::*;
use std::path::Path;

fn no_integration() -> Pane {
    let mut pane = pane(serde_json::json!({}));
    pane.agent_session = None;
    pane
}

fn record(pid: u32, session: &str) -> impl FnOnce(&Path) -> io::Result<String> {
    let text = format!(r#"{{"pid":{pid},"sessionId":"{session}","cwd":"/work"}}"#);
    move |path| {
        assert_eq!(path, claude_record(Path::new("/home/me"), 20));
        Ok(text)
    }
}

#[test]
fn without_the_integration_claudes_own_record_names_the_session() {
    let running = info(&["claude", "--model", "opus"], Some("/work"));
    let resume = resume(&no_integration(), &running, Agent::Claude, |pid| {
        recorded_session(
            Agent::Claude,
            pid,
            Path::new("/home/me"),
            record(pid, SESSION),
        )
    })
    .unwrap();
    assert_eq!(resume.session, SESSION);
    assert_eq!(resume.cwd, "/work");
}

#[test]
fn the_integrations_session_comes_first() {
    let running = info(&["claude"], Some("/work"));
    let resume = resume(
        &pane(serde_json::json!({})),
        &running,
        Agent::Claude,
        |_| panic!("the record is not read when Herdr reports the session"),
    )
    .unwrap();
    assert_eq!(resume.session, SESSION);
}

#[test]
fn with_neither_the_integration_is_still_suggested() {
    let running = info(&["claude"], Some("/work"));
    assert!(matches!(
        resume(&no_integration(), &running, Agent::Claude, |_| None),
        Err(Error::NoSession(Agent::Claude))
    ));
}

#[test]
fn a_record_of_another_process_or_agent_or_session_is_none() {
    let home = Path::new("/home/me");
    assert_eq!(
        recorded_session(Agent::Claude, 20, home, record(21, SESSION)),
        None
    );
    assert_eq!(
        recorded_session(Agent::Claude, 20, home, record(20, "--evil")),
        None
    );
    assert_eq!(
        recorded_session(Agent::Codex, 20, home, |_| Ok(String::new())),
        None
    );
    assert_eq!(
        recorded_session(Agent::Claude, 20, home, |_| Err(
            io::ErrorKind::NotFound.into()
        )),
        None
    );
}
