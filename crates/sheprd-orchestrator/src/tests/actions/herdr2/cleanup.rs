use super::*;

#[test]
fn a_pending_run_never_deletes_a_checkout_still_used_by_an_unattributed_shell() {
    let dir = tempfile::tempdir().unwrap();
    let site = site(dir.path(), "");
    test_executable::write(
        dir.path().join("home/.local/bin/herdr-core"),
        r#"#!/bin/sh
case "$1" in
run) printf '%s' "$7" > "$HOME/cwd"; exit 1 ;;
ls) printf '{"terminals":[{"address":"t9","cwd":"%s"}]}' "$(cat "$HOME/cwd")" ;;
*) exit 1 ;;
esac
"#,
        0o755,
    )
    .unwrap();
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
    assert!(run.workspace.is_some());
    assert_eq!(database.sessions().unwrap()[0].pane, Pane::Pending);
    assert!(
        perform(
            &site,
            &Action::Remove {
                run: run.id.clone()
            },
            &cancelled
        )
        .is_err()
    );
    assert!(checkout(dir.path()).exists());
    fake_core(dir.path(), "idle");
    perform(&site, &Action::Remove { run: run.id }, &cancelled).unwrap();
    assert!(!checkout(dir.path()).exists());
}

#[test]
fn removal_never_walks_up_from_a_stale_subdirectory_or_removes_another_branch() {
    let dir = tempfile::tempdir().unwrap();
    let site = site(dir.path(), "");
    fake_core(dir.path(), "idle");
    let cancelled = AtomicBool::new(false);
    perform(
        &site,
        &Action::Dispatch(Box::new(herdr2_request())),
        &cancelled,
    )
    .unwrap();
    let mut database = Store::open(&site.database).unwrap();
    let mut run = database.runs().unwrap().remove(0);
    let mut session = database.sessions().unwrap().remove(0);
    session.pane = Pane::Pending;
    let checkout = checkout(dir.path());
    let nested = checkout.join("nested");
    std::fs::create_dir(&nested).unwrap();
    run.workspace.as_mut().unwrap().path = Some(nested);
    database.update_run(&run, Some(&session)).unwrap();
    assert!(
        perform(
            &site,
            &Action::Remove {
                run: run.id.clone()
            },
            &cancelled
        )
        .is_err()
    );
    assert!(checkout.join(".git").is_file());
    run.workspace.as_mut().unwrap().path = Some(checkout.clone());
    run.workspace.as_mut().unwrap().branch = "another-run".into();
    database.update_run(&run, Some(&session)).unwrap();
    assert!(perform(&site, &Action::Remove { run: run.id }, &cancelled).is_err());
    assert!(checkout.join(".git").is_file());
    assert!(!database.runs().unwrap().is_empty());
}

#[test]
fn a_failed_launch_keeps_a_pending_session_until_locked_checkout_can_be_removed() {
    let dir = tempfile::tempdir().unwrap();
    let site = site(dir.path(), "");
    test_executable::write(
        dir.path().join("home/.local/bin/herdr-core"),
        "#!/bin/sh\nif [ \"$1\" = ls ]; then echo '{\"terminals\":[]}'; exit 0; fi\nif [ \"$1\" = run ]; then git -C \"$7\" worktree lock \"$7\"; fi\nexit 1\n",
        0o755,
    )
    .unwrap();
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
    assert_eq!(run.state, RunState::Failed);
    assert!(run.workspace.is_some());
    assert_eq!(database.sessions().unwrap()[0].pane, Pane::Pending);
    assert!(checkout(dir.path()).exists());
    assert!(
        perform(
            &site,
            &Action::Send {
                run: run.id.clone(),
                text: "never send".into()
            },
            &cancelled
        )
        .is_err()
    );
    assert!(
        Command::new("git")
            .args(["-C", &site.main_root, "worktree", "unlock"])
            .arg(checkout(dir.path()))
            .status()
            .unwrap()
            .success()
    );
    perform(&site, &Action::Remove { run: run.id }, &cancelled).unwrap();
    assert!(!checkout(dir.path()).exists());
    assert!(database.runs().unwrap().is_empty());
}

#[test]
fn failed_terminal_cleanup_keeps_the_checkout_and_session_for_retry() {
    let dir = tempfile::tempdir().unwrap();
    let site = site(dir.path(), "");
    test_executable::write(
        dir.path().join("home/.local/bin/herdr-core"),
        r#"#!/bin/sh
case "$1" in
run) echo '{"address":"t9"}' ;;
wait) echo '{"ended":true,"reason":"status","status":"blocked","turn":0}' ;;
kill) exit 1 ;;
ls) echo '{"terminals":[{"address":"t9"}]}' ;;
*) echo '{}' ;;
esac
"#,
        0o755,
    )
    .unwrap();
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
    assert_eq!(run.state, RunState::Failed);
    assert!(run.workspace.is_some());
    assert!(matches!(
        database.sessions().unwrap()[0].pane,
        Pane::Herdr2 { .. }
    ));
    assert!(checkout(dir.path()).exists());
    assert!(
        perform(
            &site,
            &Action::Remove {
                run: run.id.clone()
            },
            &cancelled
        )
        .is_err()
    );
    fake_core(dir.path(), "idle");
    perform(&site, &Action::Remove { run: run.id }, &cancelled).unwrap();
    assert!(!checkout(dir.path()).exists());
}
