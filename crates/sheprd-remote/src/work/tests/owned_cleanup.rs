use super::*;
use crate::work::owned_agent::{self, Owner};

#[test]
fn keep_is_disabled_before_kill_and_a_live_replacement_is_not_success() {
    let fixture = Fixture::new();
    fixture.program(
        "herdr-core",
        r#"case "$1" in
ls) echo '{"terminals":[{"address":"t9","state":"live","argv":["sh"]}]}' ;;
esac"#,
    );
    assert!(matches!(
        owned_agent::end_verified(&fixture.host(), "t9", &never()),
        Err(owned_agent::Error::Uncertain)
    ));
    assert_eq!(
        fixture.calls(),
        [
            "keep --off --json -- t9",
            "kill --json -- t9",
            "ls --all --json"
        ]
    );
}

#[test]
fn finished_owned_terminal_record_is_removed_but_an_unreadable_inventory_is_not_success() {
    let fixture = Fixture::new();
    fixture.program(
        "herdr-core",
        r#"case "$1" in
ls) echo '{"terminals":[{"address":"t9","state":"finished"}]}' ;;
esac"#,
    );
    owned_agent::end_verified(&fixture.host(), "t9", &never()).unwrap();
    assert_eq!(fixture.calls().last().unwrap(), "rm --json -- t9");
    fixture.program("herdr-core", "if [ \"$1\" = ls ]; then exit 1; fi");
    assert!(owned_agent::end_verified(&fixture.host(), "t9", &never()).is_err());
}

#[test]
fn unrelated_program_in_the_owned_checkout_prevents_any_kill() {
    let fixture = Fixture::new();
    fixture.program("herdr-core", r#"echo '{"terminals":[
{"address":"t9","state":"live","cwd":"/work/lane","argv":["claude"],"labels":{"sheprd.fanout.lane":"a"},"foreground":{"name":"claude","argv":["claude"],"pid":9}},
{"address":"t10","state":"live","cwd":"/work/lane/other","argv":["sh"]}]}'"#);
    let tree = Worktree {
        path: "/work/lane".into(),
        opened: Opened::Herdr2,
    };
    let agent = Agent {
        kind: AgentKind::Claude,
        name: "a",
        model: None,
        prompt: None,
    };
    assert!(matches!(
        owned_agent::end(
            &fixture.host(),
            &tree,
            Owner {
                key: "sheprd.fanout.lane",
                value: "a"
            },
            &agent,
            Some("t9"),
            &never()
        ),
        Err(owned_agent::Error::Uncertain)
    ));
    assert_eq!(fixture.calls(), ["ls --all --json"]);
}

#[test]
fn absence_finishes_known_terminals_but_never_guesses_an_unacknowledged_launch() {
    let fixture = Fixture::new();
    fixture.program("herdr-core", "echo '{\"terminals\":[]}'");
    let tree = Worktree {
        path: "/work/lane".into(),
        opened: Opened::Herdr2,
    };
    let agent = Agent {
        kind: AgentKind::Claude,
        name: "a",
        model: None,
        prompt: None,
    };
    let owner = Owner {
        key: "sheprd.fanout.lane",
        value: "a",
    };
    assert!(matches!(
        owned_agent::end(&fixture.host(), &tree, owner, &agent, None, &never()),
        Err(owned_agent::Error::Uncertain)
    ));
    owned_agent::end(&fixture.host(), &tree, owner, &agent, Some("t9"), &never()).unwrap();
    assert!(fixture.calls().iter().all(|call| call == "ls --all --json"));
}
