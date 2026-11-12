use super::*;

#[test]
fn a_send_applied_before_its_ack_is_lost_is_not_replayed_or_killed() {
    check_send_outcome(true);
}

#[test]
fn a_rejected_send_preserves_the_known_shell_for_inspection() {
    check_send_outcome(false);
}

fn check_send_outcome(applied: bool) {
    let fixture = Fixture::new();
    let receipt = fixture.temp.path().join("agent-started");
    let apply = if applied {
        format!(
            "printf started > {}",
            shell_quote(receipt.to_str().unwrap())
        )
    } else {
        ":".to_owned()
    };
    fixture.program(
        "herdr-core",
        &format!(
            r#"case "$1" in
run) echo '{{"address":"t4"}}' ;;
send) {apply}; exit 75 ;;
kill) exit 0 ;;
esac"#
        ),
    );
    let tree = Worktree {
        path: "/work/lane".into(),
        opened: Opened::Herdr2,
    };
    let agent = Agent {
        kind: AgentKind::Claude,
        name: "claude-test",
        model: None,
        prompt: Some("only once"),
    };
    let error = start_agent(&fixture.host(), &tree, &agent, &never()).unwrap_err();
    let Error::Unprompted { started, source } = error else {
        panic!("retain known terminal")
    };
    assert_eq!(
        started,
        Started::Herdr2 {
            terminal: "t4".into()
        }
    );
    assert!(matches!(
        *source,
        Error::Script {
            step: Step::StartAgent,
            ..
        }
    ));
    assert_eq!(receipt.exists(), applied);
    let calls = fixture.calls();
    assert_eq!(
        calls
            .iter()
            .filter(|call| call.starts_with("send "))
            .count(),
        1
    );
    assert!(
        !calls
            .iter()
            .any(|call| call.starts_with("kill ") || call.contains("only once"))
    );
}
