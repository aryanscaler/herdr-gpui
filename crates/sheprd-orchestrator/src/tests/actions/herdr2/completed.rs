//! A run whose agent finished: `--keep` leaves a shell in its terminal, which
//! still carries the run's label and command. Restart and Remove end it;
//! Send never types into that shell.
use super::*;

/// The agent in the run's terminal exits, leaving the kept shell in front.
fn finish(dir: &Path) {
    in_front(dir, serde_json::json!(["-zsh"]), "zsh");
}

/// `argv`, named `name`, is now the terminal's foreground program.
fn in_front(dir: &Path, argv: serde_json::Value, name: &str) {
    let inventory = dir.join("home/core-inventory");
    let mut list: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&inventory).unwrap()).unwrap();
    list["terminals"][0]["foreground"] =
        serde_json::json!({"argv": argv, "name": name, "cwd": "/", "pid": 10});
    std::fs::write(&inventory, serde_json::to_vec(&list).unwrap()).unwrap();
}

#[test]
fn a_finished_run_restarts_and_is_removed_but_is_never_typed_into() {
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
    let run = Store::open(&site.database)
        .unwrap()
        .runs()
        .unwrap()
        .remove(0);
    finish(dir.path());
    let refused = perform(
        &site,
        &Action::Send {
            run: run.id.clone(),
            text: "not for a shell".into(),
        },
        &cancelled,
    );
    assert!(
        matches!(
            refused,
            Err(Error::OwnedAgent(
                sheprd_remote::work::owned_agent::Error::Uncertain
            ))
        ),
        "{refused:?}"
    );

    perform(
        &site,
        &Action::Restart {
            run: run.id.clone(),
        },
        &cancelled,
    )
    .unwrap();
    let store = Store::open(&site.database).unwrap();
    assert_eq!(store.runs().unwrap()[0].state, RunState::Running);
    let calls = core_calls(dir.path());
    assert_eq!(
        calls.iter().filter(|call| call.starts_with("run ")).count(),
        2
    );
    assert_eq!(
        calls
            .iter()
            .filter(|call| call.contains("Fix the icons"))
            .count(),
        2
    );
    assert!(!calls.iter().any(|call| call.contains("not for a shell")));

    finish(dir.path());
    perform(&site, &Action::Remove { run: run.id }, &cancelled).unwrap();
    assert!(!checkout(dir.path()).exists());
    assert!(
        Store::open(&site.database)
            .unwrap()
            .runs()
            .unwrap()
            .is_empty()
    );
}

/// Once the user runs something in the kept shell, the terminal is their
/// work: Remove ends nothing and keeps the checkout.
#[test]
fn a_program_started_in_the_kept_shell_keeps_the_run() {
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
    let run = Store::open(&site.database)
        .unwrap()
        .runs()
        .unwrap()
        .remove(0);
    in_front(dir.path(), serde_json::json!(["vim", "notes.md"]), "vim");
    let refused = perform(&site, &Action::Remove { run: run.id }, &cancelled);
    assert!(
        matches!(
            refused,
            Err(Error::OwnedAgent(
                sheprd_remote::work::owned_agent::Error::Uncertain
            ))
        ),
        "{refused:?}"
    );
    assert!(checkout(dir.path()).join(".git").exists());
    assert!(
        !core_calls(dir.path())
            .iter()
            .any(|call| call.starts_with("kill "))
    );
}
