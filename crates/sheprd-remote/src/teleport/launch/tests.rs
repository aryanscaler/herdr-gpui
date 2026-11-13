use super::super::snapshot::{Process, SessionKind};
use super::*;

mod identity;

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| (*s).to_owned()).collect()
}

fn session(agent: &str, kind: SessionKind, value: &str) -> AgentSession {
    AgentSession {
        agent: agent.into(),
        kind,
        source: format!("herdr:{agent}"),
        value: value.into(),
    }
}

fn pane(agent: Option<&str>, session: Option<AgentSession>) -> Pane {
    Pane {
        pane_id: "p".into(),
        tab_id: "t".into(),
        cwd: Some("/w".into()),
        foreground_cwd: None,
        agent: agent.map(Into::into),
        agent_session: session,
        status: super::super::snapshot::PaneStatus::Idle,
    }
}

fn running(argv: &[&str]) -> ProcessInfo {
    ProcessInfo {
        foreground_process_group_id: Some(9),
        shell_pid: Some(1),
        foreground_processes: vec![Process {
            pid: 9,
            argv: strings(argv),
            cwd: None,
        }],
    }
}

#[test]
fn classification_prefers_sessions_then_agents_then_commands() {
    let claude = pane(
        Some("claude"),
        Some(session("claude", SessionKind::Id, "abc")),
    );
    assert_eq!(
        Work::classify(
            &claude,
            &running(&[
                "/Users/me/.local/bin/claude",
                "--dangerously-skip-permissions",
                "--model",
                "opus",
                "fix it",
                "--resume",
                "old"
            ])
        ),
        Work::Session {
            agent: AgentKind::Claude,
            session: session("claude", SessionKind::Id, "abc"),
            flags: strings(&["--dangerously-skip-permissions", "--model", "opus"]),
        }
    );
    // A session reported by anything but Herdr's integration is not trusted.
    let mut forged = session("claude", SessionKind::Id, "abc");
    forged.source = "plugin:x".into();
    assert!(matches!(
        Work::classify(&pane(Some("claude"), Some(forged)), &running(&["claude"])),
        Work::Agent { .. }
    ));
    // Claude sessions are ids; a path cannot be resumed.
    assert!(matches!(
        Work::classify(
            &pane(
                Some("claude"),
                Some(session("claude", SessionKind::Path, "/x.jsonl"))
            ),
            &running(&["claude"])
        ),
        Work::Agent { .. }
    ));
    assert_eq!(
        Work::classify(&pane(Some("gemini"), None), &ProcessInfo::default()),
        Work::Agent {
            name: "gemini".into(),
            argv: strings(&["gemini"])
        }
    );
    assert_eq!(
        Work::classify(&pane(None, None), &running(&["npm", "run", "dev"])),
        Work::Command(strings(&["npm", "run", "dev"]))
    );
    assert_eq!(
        Work::classify(&pane(None, None), &ProcessInfo::default()),
        Work::Shell
    );
}

#[test]
fn only_carried_agents_resume_on_another_host() {
    let copilot = pane(
        Some("copilot"),
        Some(session("copilot", SessionKind::Id, "c-1")),
    );
    assert_eq!(
        Work::classify(&copilot, &running(&["copilot", "--yolo"])),
        Work::Session {
            agent: AgentKind::Copilot,
            session: session("copilot", SessionKind::Id, "c-1"),
            flags: strings(&["--yolo"]),
        }
    );
    assert!(matches!(
        Work::classify(
            &pane(
                Some("letta"),
                Some(session("letta", SessionKind::Id, "conv-1"))
            ),
            &running(&["letta"])
        ),
        Work::Session {
            agent: AgentKind::Letta,
            ..
        }
    ));
    // Droid's session storage is unconfirmed, so its session stays put.
    assert_eq!(
        Work::classify(
            &pane(
                Some("droid"),
                Some(session("droid", SessionKind::Id, "d-1"))
            ),
            &running(&["droid", "--auto", "high"])
        ),
        Work::Agent {
            name: "droid".into(),
            argv: strings(&["droid", "--auto", "high"])
        }
    );
}

#[test]
fn paths_move_only_at_boundaries() {
    assert_eq!(
        remap("/w/feat/src", "/w/feat", "/h/feat").as_deref(),
        Some("/h/feat/src")
    );
    assert_eq!(
        remap("/w/feat", "/w/feat/", "/h/feat").as_deref(),
        Some("/h/feat")
    );
    assert_eq!(remap("/w/feature", "/w/feat", "/h/feat"), None);
    let text = br#"{"cwd":"/w/feat","file":"/w/feat/a.rs","other":"/w/feature","x":"/w/feat.bak"}"#;
    assert_eq!(
        rewrite_paths(text, "/w/feat", "/home/u/feat"),
        br#"{"cwd":"/home/u/feat","file":"/home/u/feat/a.rs","other":"/w/feature","x":"/w/feat.bak"}"#
    );
}

#[test]
fn remapped_commands_find_programs_on_the_destination() {
    assert_eq!(
        remap_argv(
            &strings(&[
                "/opt/homebrew/bin/cargo",
                "run",
                "--manifest-path=/w/feat/Cargo.toml"
            ]),
            "/w/feat",
            "/h/f"
        ),
        strings(&["cargo", "run", "--manifest-path=/h/f/Cargo.toml"])
    );
    assert_eq!(
        remap_argv(&strings(&["/w/feat/scripts/dev.sh"]), "/w/feat", "/h/f"),
        strings(&["/h/f/scripts/dev.sh"])
    );
    assert_eq!(
        command_line(&strings(&["git", "commit", "-m", "it's done", ""])),
        "git commit -m 'it'\\''s done' ''"
    );
}

#[test]
fn claude_project_dirs_match_claude_code() {
    assert_eq!(
        claude_project_dir("/Users/penso/.herdr/worktrees/herdr-gpui/worktree-calm-forest-9099"),
        "-Users-penso--herdr-worktrees-herdr-gpui-worktree-calm-forest-9099"
    );
    // Computed by Claude Code's own function under JavaScript: over 200
    // characters, with UTF-16 surrogates each becoming a dash.
    assert_eq!(
        claude_project_dir(
            "/Users/penso/.herdr/worktrees/moltis/agent-1255-bug-agentend-messagesending-and-messagesent-hooks-are-declared-but-never-dispatched-by-the-gateway-runtime-fbd6e3d9-extra-long-suffix/\u{e9}/sub/and/some/more/\u{1f600}/deeper"
        ),
        "-Users-penso--herdr-worktrees-moltis-agent-1255-bug-agentend-messagesending-and-messagesent-hooks-are-declared-but-never-dispatched-by-the-gateway-runtime-fbd6e3d9-extra-long-suffix---sub-and-some-mor-brbx8l"
    );
    assert_eq!(base36(0), "0");
    assert_eq!(base36(35), "z");
    assert_eq!(base36(36), "10");
}

#[test]
fn fresh_starts_carry_the_handoff_prompt() {
    let prompt = handoff_resume_prompt(&handoff_path(0));
    assert_eq!(
        prompt,
        "Read .herdr/teleport/handoff-1.md and continue the work it describes."
    );
    assert_eq!(
        AgentKind::Claude.start_argv(Some("go")),
        strings(&["claude", "go"])
    );
    assert_eq!(
        AgentKind::Opencode.start_argv(Some("go")),
        strings(&["opencode", "--prompt", "go"])
    );
    assert_eq!(AgentKind::Codex.start_argv(None), strings(&["codex"]));
    assert_eq!(
        AgentKind::Copilot.start_argv(Some("go")),
        strings(&["copilot", "-i", "go"])
    );
    // An agent whose prompt flag is unknown starts without the note.
    assert!(!AgentKind::Droid.takes_prompt());
    assert_eq!(AgentKind::Droid.start_argv(Some("go")), strings(&["droid"]));
    assert_eq!(
        AgentKind::Cursor.start_argv(None),
        strings(&["cursor-agent"])
    );
}

#[test]
fn pi_session_dirs_match_pi() {
    assert_eq!(
        pi_session_dir("/Users/penso/.herdr/worktrees/moltis/moltis-ui"),
        "--Users-penso-.herdr-worktrees-moltis-moltis-ui--"
    );
}
