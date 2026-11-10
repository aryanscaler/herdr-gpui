use super::*;

fn terminal(json: &str) -> Terminal {
    serde_json::from_str(json).unwrap()
}

/// A Claude Code terminal as herdr-core 0.1.17 reports one, and a shell.
fn terminals() -> HashMap<String, Terminal> {
    HashMap::from([
        (
            "t3".to_owned(),
            terminal(
                r#"{"pid":12952,"cwd":"/w",
                    "foreground":{"argv":["claude","--dangerously-skip-permissions"],
                        "cwd":"/w/app","name":"2.1.319","pid":29421},
                    "status":{"app":"claude-code","state":"idle"}}"#,
            ),
        ),
        (
            "t1".to_owned(),
            terminal(
                r#"{"pid":63492,"foreground":{"argv":["/bin/zsh","-l"],"pid":63492},"status":null}"#,
            ),
        ),
    ])
}

const RECORD: &str = r#"{"pid":29421,"sessionId":"6941a392-5074-4ffb-abc7-b28c821b975e",
    "cwd":"/w/app","procStart":"Sat Nov  7 01:17:05 2026","kind":"interactive"}"#;

fn section(id: &str, utc: &str, local: &str, record: &str) -> String {
    format!("\u{1e}{id}\n{utc}\n{local}\n{record}\n")
}

#[test]
fn only_claude_code_terminals_are_asked() {
    assert_eq!(claude_processes(&terminals()), [("t3", 29421)]);
}

#[test]
fn a_record_of_the_running_process_names_its_session() {
    let processes = [("t3", 29421)];
    let output = section(
        "t3",
        "Sat Nov  7 01:17:05 2026    ",
        "Fri Nov  6 17:17:05 2026    ",
        RECORD,
    );
    let found = parse(&output, &processes);
    let session = &found["t3"];
    assert_eq!(session.value, "6941a392-5074-4ffb-abc7-b28c821b975e");
    assert!(AgentKind::Claude.accepts(session));

    // It resumes, keeping its flags.
    let work = work(&terminals()["t3"], Some(session));
    assert_eq!(
        work,
        Work::Session {
            agent: AgentKind::Claude,
            session: session.clone(),
            flags: vec!["--dangerously-skip-permissions".into()],
        }
    );
    // Without a session it is an agent to hand off, as before.
    assert!(matches!(
        super::work(&terminals()["t3"], None),
        Work::Agent { .. }
    ));
}

#[test]
fn a_record_of_another_process_is_never_taken() {
    let processes = [("t3", 29421)];
    // Left by an earlier process given the same ID: it started at another time.
    let stale = section(
        "t3",
        "Mon Nov  9 08:00:00 2026",
        "Mon Nov  9 00:00:00 2026",
        RECORD,
    );
    assert!(parse(&stale, &processes).is_empty());
    // The process is gone, so `ps` says nothing.
    assert!(parse(&section("t3", "", "", RECORD), &processes).is_empty());
    // A record of another process ID, or none at all.
    let other = RECORD.replace("29421", "29422");
    let start = "Sat Nov  7 01:17:05 2026";
    assert!(parse(&section("t3", start, start, &other), &processes).is_empty());
    assert!(parse("\u{1e}t3\n", &processes).is_empty());
    // A terminal not asked about, and a session ID that reads as a flag.
    assert!(parse(&section("t9", start, start, RECORD), &processes).is_empty());
    let flag = RECORD.replace("6941a392", "-6941a392");
    assert!(parse(&section("t3", start, start, &flag), &processes).is_empty());
    // A record without a start time cannot be tied to the process.
    let undated = r#"{"pid":29421,"sessionId":"6941a392"}"#;
    assert!(parse(&section("t3", start, start, undated), &processes).is_empty());
}

#[test]
fn a_move_is_refused_once_an_agent_runs_another_session() {
    let start = "Sat Nov  7 01:17:05 2026";
    let processes = [("t3", 29421)];
    let reviewed = parse(&section("t3", start, start, RECORD), &processes);
    let expected: Vec<(String, AgentSession)> = reviewed.clone().into_iter().collect();
    assert!(unchanged(&expected, &reviewed).is_ok());
    // `/resume` picked another conversation in the same process.
    let resumed = RECORD.replace("6941a392", "7aa96a54");
    let now = parse(&section("t3", start, start, &resumed), &processes);
    assert!(matches!(
        unchanged(&expected, &now),
        Err(Error::SourceSessionChanged)
    ));
    // Claude restarted, or quit: no session of its process is found.
    assert!(matches!(
        unchanged(&expected, &HashMap::new()),
        Err(Error::SourceSessionChanged)
    ));
}
