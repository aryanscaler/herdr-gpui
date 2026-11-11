//! What the agent continues as, and the line that starts it.

use super::*;

fn resumed(argv: &[&str], cwd: Option<&str>, agent: Agent) -> Resume {
    let session = serde_json::json!({"agent_session": {
        "agent": agent.name(),
        "kind": "id",
        "source": format!("herdr:{}", agent.name()),
        "value": SESSION,
    }, "agent": agent.name()});
    resume(&pane(session), &info(argv, cwd), agent, |_| None).unwrap()
}

#[test]
fn the_session_continues_in_the_agents_own_folder() {
    let resume = resumed(&["claude"], Some("/work"), Agent::Claude);
    assert_eq!(resume.cwd, "/work");
    assert_eq!(resume.session, SESSION);
    assert_eq!(resumed(&["claude"], None, Agent::Claude).cwd, "/foreground");
}

#[test]
fn claude_keeps_its_flags_and_drops_what_named_the_old_session() {
    let argv = [
        "claude",
        "--model",
        "opus",
        "--resume",
        "old",
        "fix the bug",
    ];
    let resume = resumed(&argv, Some("/work"), Agent::Claude);
    assert_eq!(resume.flags, ["--model", "opus"]);
    assert_eq!(
        resume.words(),
        [
            "--resume", SESSION, "--cwd", "/work", "claude", "--", "--model", "opus"
        ]
    );
}

#[test]
fn other_agents_leave_their_flags_behind() {
    let argv = ["codex", "--model", "gpt"];
    let resume = resumed(&argv, Some("/work"), Agent::Codex);
    assert!(resume.flags.is_empty());
    assert_eq!(
        resume.words(),
        ["--resume", SESSION, "--cwd", "/work", "codex"]
    );
}

#[test]
fn the_local_line_runs_this_apps_program_through_sh() {
    let resume = resumed(&["claude"], Some("/my work"), Agent::Claude);
    assert_eq!(
        resume
            .local_line(Path::new("/Applications/Herdr.app/sheprd-acp"))
            .unwrap(),
        format!(
            " sh -c 'exec \"$0\" \"$@\"' '/Applications/Herdr.app/sheprd-acp' \
             '--resume' '{SESSION}' '--cwd' '/my work' 'claude'"
        )
    );
}

#[test]
fn a_folder_no_line_can_carry_is_refused_before_anything_stops() {
    for cwd in ["/it's", "/line\nbreak"] {
        let resume = resumed(&["claude"], Some(cwd), Agent::Claude);
        assert!(matches!(
            resume.local_line(Path::new("/app/sheprd-acp")),
            Err(Error::Line)
        ));
    }
}
