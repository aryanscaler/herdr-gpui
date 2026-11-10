//! The model an agent starts with: `--model NAME` on its own command line,
//! after Herdr 1's `--`, and left out when it could become another option.

use super::*;

const CORE: &str = r#"case "$1" in
run) echo '{"address":"t4"}' ;;
send) echo '{}' ;;
esac"#;

fn herdr2_agent(model: Option<&str>) -> Vec<String> {
    let fixture = Fixture::new();
    fixture.program("herdr-core", CORE);
    let worktree = Worktree {
        path: "/w".into(),
        opened: Opened::Herdr2,
    };
    let agent = Agent {
        kind: AgentKind::Cursor,
        name: "cursor-1",
        model,
        prompt: None,
    };
    start_agent(&fixture.host(), &worktree, &agent, &never()).unwrap();
    fixture.calls()
}

#[test]
fn herdr_2_types_the_model_after_the_agent_command() {
    assert_eq!(
        herdr2_agent(Some("gpt-5.1/high")),
        [
            "run --bg --json --no-parent --cwd /w",
            "send --json -- t4 'cursor-agent' '--model' 'gpt-5.1/high'",
        ]
    );
}

#[test]
fn a_model_that_could_be_an_option_is_left_out() {
    for model in ["--yolo", "two words", ""] {
        assert_eq!(
            herdr2_agent(Some(model))[1],
            "send --json -- t4 'cursor-agent'",
            "{model:?}"
        );
    }
}

#[test]
fn herdr_1_passes_the_model_to_the_agent_after_its_own_options() {
    let fixture = Fixture::new();
    fixture.program("herdr", "echo '{}'");
    let worktree = Worktree {
        path: "/x/lane".into(),
        opened: Opened::Herdr1 {
            workspace_id: "w7".into(),
            pane_id: "w7-1".into(),
        },
    };
    let agent = Agent {
        kind: AgentKind::Claude,
        name: "claude-3",
        model: Some("opus"),
        prompt: None,
    };
    start_agent(&fixture.host(), &worktree, &agent, &never()).unwrap();
    assert_eq!(
        fixture.calls(),
        ["agent start claude-3 --kind claude --pane w7-1 --timeout 90000 -- --model opus"]
    );
}
