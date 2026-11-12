use super::*;

#[test]
fn lost_launch_reply_recovers_owned_terminal_without_replaying_prompt() {
    let dir = tempfile::tempdir().unwrap();
    let site = site(dir.path(), "");
    fake_core(dir.path(), "idle");
    let marker = dir.path().join("home/lose-run-reply");
    std::fs::write(&marker, "").unwrap();
    let cancelled = AtomicBool::new(false);
    assert!(
        perform(
            &site,
            &Action::Dispatch(Box::new(herdr2_request())),
            &cancelled
        )
        .is_err()
    );
    let database = Store::open(&site.database).unwrap();
    let run = database.runs().unwrap().remove(0);
    assert_eq!(database.sessions().unwrap()[0].pane, Pane::Pending);
    assert!(checkout(dir.path()).exists());
    perform(
        &site,
        &Action::Recover {
            run: run.id.clone(),
        },
        &cancelled,
    )
    .unwrap();
    assert!(matches!(
        database.sessions().unwrap()[0].pane,
        Pane::Herdr2 { .. }
    ));
    assert!(
        !core_calls(dir.path())
            .iter()
            .any(|call| call.starts_with("send "))
    );
    assert_eq!(
        core_calls(dir.path())
            .iter()
            .filter(|call| call.starts_with("run "))
            .count(),
        1
    );

    // Only an explicit restart sends the original prompt again. It keeps
    // files and the model, but uses a new, durable ownership token.
    std::fs::remove_file(marker).unwrap();
    let before = database.sessions().unwrap()[0].agent_name.clone();
    std::fs::write(checkout(dir.path()).join("keep.txt"), "user changes").unwrap();
    perform(
        &site,
        &Action::Restart {
            run: run.id.clone(),
        },
        &cancelled,
    )
    .unwrap();
    assert_ne!(database.sessions().unwrap()[0].agent_name, before);
    assert_eq!(database.runs().unwrap()[0].id, run.id);
    assert_eq!(database.runs().unwrap()[0].state, RunState::Running);
    assert!(checkout(dir.path()).join("keep.txt").exists());
    let calls = core_calls(dir.path());
    assert_eq!(
        calls
            .iter()
            .filter(|call| call.contains("Fix the icons"))
            .count(),
        1
    );
    assert_eq!(
        calls
            .iter()
            .filter(|call| call.starts_with("run ") && call.ends_with("claude --model opus"))
            .count(),
        2
    );
}

#[test]
fn competing_restart_cannot_claim_the_same_receipt_twice() {
    let dir = tempfile::tempdir().unwrap();
    let site = site(dir.path(), "");
    fake_core(dir.path(), "idle");
    perform(
        &site,
        &Action::Dispatch(Box::new(herdr2_request())),
        &AtomicBool::new(false),
    )
    .unwrap();
    let first = Store::open(&site.database).unwrap();
    let second = Store::open(&site.database).unwrap();
    let session = first.sessions().unwrap().remove(0);
    first
        .claim_restart(&session.run_id, &session.agent_name, "next-owner")
        .unwrap();
    assert!(matches!(
        second.claim_restart(&session.run_id, &session.agent_name, "other-owner"),
        Err(Error::Busy)
    ));
}
