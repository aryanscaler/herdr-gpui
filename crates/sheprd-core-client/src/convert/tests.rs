use super::*;
use sheprd_backend::RunState;
use std::collections::BTreeMap;

fn process(pid: u32, argv: &[&str]) -> Process {
    Process {
        argv: argv.iter().map(|arg| (*arg).to_owned()).collect(),
        name: argv[0].rsplit('/').next().unwrap().to_owned(),
        pid,
    }
}

fn info(status: AgentStatus, foreground: Option<Process>) -> TerminalInfo {
    TerminalInfo {
        agent: status,
        cwd: Some("/work/app".into()),
        foreground,
        id: TerminalId::new("t1").unwrap(),
        labels: BTreeMap::new(),
        name: None,
        reporter: None,
        state: RunState::Running,
        title: None,
    }
}

fn claude(status: AgentStatus) -> TerminalInfo {
    info(
        status,
        Some(process(
            42,
            &["/usr/local/bin/claude", "--model", "opus", "fix it"],
        )),
    )
}

fn record(pid: u32, session: &str) -> String {
    format!(r#"{{"pid":{pid},"sessionId":"{session}","cwd":"/work/app","status":"idle"}}"#)
}

#[test]
fn only_an_idle_known_agent_moves() {
    assert_eq!(check(&claude(AgentStatus::Idle)).unwrap(), Agent::Claude);
    assert_eq!(check(&claude(AgentStatus::Done)).unwrap(), Agent::Claude);
    assert!(matches!(
        check(&claude(AgentStatus::Working)),
        Err(convert::Error::Busy(Agent::Claude))
    ));
    assert!(matches!(
        check(&info(AgentStatus::Idle, Some(process(9, &["zsh"])))),
        Err(convert::Error::NoAgent)
    ));
}

const ENDPOINT: &str = "0123456789abcdef0123456789abcdef";

fn labelled(mut info: TerminalInfo) -> TerminalInfo {
    info.labels.insert(TOKEN.to_owned(), ENDPOINT.into());
    info
}

#[test]
fn a_chat_does_not_move_again() {
    let chat = labelled(info(
        AgentStatus::Idle,
        Some(process(7, &["/opt/sheprd/sheprd-acp", "--resume", "s"])),
    ));
    assert!(matches!(check(&chat), Err(convert::Error::AlreadyChat)));
}

#[test]
fn a_label_is_a_chat_only_while_a_chat_program_is_in_front() {
    let endpoint = Some(ENDPOINT.parse().unwrap());
    // Started by a core that reports no process.
    assert_eq!(
        chat_endpoint(&labelled(info(AgentStatus::Idle, None))),
        endpoint
    );
    for program in [
        "/opt/sheprd/sheprd-acp",
        "/opt/herdr/herdr-acp",
        "herdr-acp.exe",
    ] {
        let chat = labelled(info(AgentStatus::Idle, Some(process(7, &[program]))));
        assert_eq!(chat_endpoint(&chat), endpoint, "{program}");
    }
    // The chat ended and the shell it was typed into is back, or the agent
    // was started there again: the label is left over.
    let shell = labelled(info(AgentStatus::Idle, Some(process(9, &["zsh"]))));
    assert_eq!(chat_endpoint(&shell), None);
    let again = labelled(claude(AgentStatus::Idle));
    assert_eq!(chat_endpoint(&again), None);
    assert_eq!(check(&again).unwrap(), Agent::Claude);
    // No label, or one that names no endpoint.
    assert_eq!(chat_endpoint(&info(AgentStatus::Idle, None)), None);
    let mut odd = info(AgentStatus::Idle, None);
    odd.labels.insert(TOKEN.to_owned(), "not an id".into());
    assert_eq!(chat_endpoint(&odd), None);
}

#[test]
fn the_chat_line_names_its_endpoint_and_no_herdr_pane() {
    let resume = Resume {
        agent: Agent::Claude,
        cwd: "/work/app".into(),
        flags: vec!["--model".into(), "opus".into()],
        session: "abc-123".into(),
    };
    let endpoint: EndpointId = ENDPOINT.parse().unwrap();
    let line = chat_line(&resume, Path::new("/opt/sheprd/sheprd-acp"), &endpoint).unwrap();
    let words = format!(
        "'env' 'HERDR_PANE_ID=' 'HERDR_ACP_ENDPOINT={ENDPOINT}' '/opt/sheprd/sheprd-acp' '--resume' 'abc-123'"
    );
    assert!(line.starts_with(" sh -c "), "{line}");
    assert!(line.contains(&words), "{line}");
    assert!(line.contains("'--model' 'opus'"), "{line}");
    assert!(matches!(
        chat_line(&resume, Path::new("/it's/sheprd-acp"), &endpoint),
        Err(convert::Error::Line)
    ));
}

#[test]
fn claudes_session_comes_from_its_own_record() {
    let home = Path::new("/home/me");
    let resume = resume(&claude(AgentStatus::Idle), Agent::Claude, home, |path| {
        assert_eq!(path, Path::new("/home/me/.claude/sessions/42.json"));
        Ok(record(42, "e6355726-1f61-42c3-bf05-5c1d1cba3262"))
    })
    .unwrap();
    assert_eq!(resume.session, "e6355726-1f61-42c3-bf05-5c1d1cba3262");
    assert_eq!(resume.cwd, "/work/app");
    assert_eq!(
        resume.flags,
        ["--model", "opus"],
        "the prompt is not replayed"
    );
}

#[test]
fn a_record_of_another_process_or_an_odd_session_is_none() {
    let home = Path::new("/h");
    let found = claude(AgentStatus::Idle);
    for text in [
        record(43, "abc"),
        record(42, "-rf"),
        record(42, "a b"),
        "not json".to_owned(),
    ] {
        assert!(matches!(
            resume(&found, Agent::Claude, home, |_| Ok(text.clone())),
            Err(convert::Error::NoCoreSession(Agent::Claude))
        ));
    }
    assert!(matches!(
        resume(&found, Agent::Claude, home, |_| Err(
            io::ErrorKind::NotFound.into()
        )),
        Err(convert::Error::NoCoreSession(Agent::Claude))
    ));
}

#[test]
fn other_agents_have_no_session_to_read_yet() {
    let codex = info(AgentStatus::Idle, Some(process(7, &["codex"])));
    assert!(matches!(
        resume(&codex, Agent::Codex, Path::new("/h"), |_| unreachable!()),
        Err(convert::Error::NoCoreSession(Agent::Codex))
    ));
}

#[test]
fn only_the_terminals_own_shell_in_front_takes_the_line() {
    // Terminal pid 100 started `program`; `front` is in front.
    let record = |program: &str, front: Option<(i32, &str)>| -> Terminal {
        let mut value = crate::tests::terminal("t1");
        value["argv"] = serde_json::json!([program]);
        value["pid"] = serde_json::json!(100);
        if let Some((pid, name)) = front {
            value["foreground"] =
                serde_json::json!({"pid": pid, "name": name, "argv": [name], "cwd": ""});
        }
        serde_json::from_value(value).unwrap()
    };
    // The shell the terminal started is back.
    assert!(shell_back(
        &record("zsh", Some((100, "zsh"))),
        Agent::Claude
    ));
    // Claude, or `less` run after it on the same line, is in front.
    assert!(!shell_back(
        &record("zsh", Some((200, "claude"))),
        Agent::Claude
    ));
    assert!(!shell_back(
        &record("zsh", Some((200, "less"))),
        Agent::Claude
    ));
    // Nothing in front tells nothing.
    assert!(!shell_back(&record("zsh", None), Agent::Claude));
    // Claude is the terminal's own program: no shell will come back.
    let own = record("claude", Some((100, "claude")));
    assert!(!shell_back(&own, Agent::Claude));
    assert!(agent_is_program(&own, Agent::Claude));
    assert!(!agent_is_program(
        &record("zsh", Some((200, "claude"))),
        Agent::Claude
    ));
}
