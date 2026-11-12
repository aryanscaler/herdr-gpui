use super::*;

#[test]
fn a_prompt_applied_before_its_reply_is_lost_is_never_cleaned_up_or_replayed() {
    let dir = tempfile::tempdir().unwrap();
    let site = site(dir.path(), "");
    let script = r#"#!/bin/sh
printf '%s\n' "$*" >> "$HOME/core-calls"
case "$1" in
run) echo '{"address":"t9"}' ;;
wait) echo '{"ended":true,"exit_code":null,"reason":"status","status":"idle","turn":0}' ;;
send) case "$*" in *'Fix the icons'*) printf applied > "$HOME/prompt-applied"; exit 75;; *) echo '{}';; esac ;;
*) echo '{}' ;;
esac
"#;
    test_executable::write(dir.path().join("home/.local/bin/herdr-core"), script, 0o755).unwrap();
    assert!(
        perform(
            &site,
            &Action::Dispatch(Box::new(herdr2_request())),
            &AtomicBool::new(false)
        )
        .is_err()
    );
    assert!(dir.path().join("home/prompt-applied").exists());
    assert!(checkout(dir.path()).exists());
    let store = Store::open(&site.database).unwrap();
    assert_eq!(
        store.sessions().unwrap()[0].pane,
        Pane::Herdr2 {
            terminal: "t9".into()
        }
    );
    let calls = core_calls(dir.path());
    assert_eq!(
        calls
            .iter()
            .filter(|call| call.contains("Fix the icons"))
            .count(),
        1
    );
    assert!(!calls.iter().any(|call| call.starts_with("kill ")));
}

/// A start that failed and one whose reply was lost look the same to the
/// dispatch. Neither may delete the checkout, kill, or type the prompt: the
/// run keeps its owner receipt, and Recover finds an agent that did start
/// (`restart::lost_launch_reply_recovers_owned_terminal_without_replaying_prompt`).
#[test]
fn rejected_or_lost_start_replies_retain_the_checkout_and_owner_receipt() {
    for (applied, marker) in [(false, "reject-run"), (true, "lose-run-reply")] {
        let dir = tempfile::tempdir().unwrap();
        let site = site(dir.path(), "");
        fake_core(dir.path(), "idle");
        std::fs::write(dir.path().join("home").join(marker), "").unwrap();
        let error = perform(
            &site,
            &Action::Dispatch(Box::new(herdr2_request())),
            &AtomicBool::new(false),
        )
        .unwrap_err();
        assert!(matches!(error, Error::Retained { .. }), "{error:?}");
        assert_eq!(
            dir.path().join("home/core-inventory").exists(),
            applied,
            "only the lost reply leaves a terminal"
        );
        assert!(checkout(dir.path()).exists());
        let store = Store::open(&site.database).unwrap();
        let run = store.runs().unwrap().remove(0);
        assert_eq!(run.state, RunState::Failed);
        assert!(run.workspace.is_some());
        let session = store.sessions().unwrap().remove(0);
        assert_eq!(session.pane, Pane::Pending);
        assert_eq!(session.checkout_token.as_deref(), Some(run.id.as_str()));
        let calls = core_calls(dir.path());
        assert_eq!(
            calls.iter().filter(|call| call.starts_with("run ")).count(),
            1
        );
        assert!(!calls.iter().any(|call| call.starts_with("kill ")
            || call.starts_with("send ")
            || call.contains("Fix the icons")));
    }
}
