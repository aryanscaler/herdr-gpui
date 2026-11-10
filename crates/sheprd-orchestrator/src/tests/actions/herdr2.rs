//! Dispatching on Herdr 2: Git makes the worktree from the main checkout and
//! the agent is typed into a herdr-core shell there, against a fake
//! `herdr-core` that logs its calls. Herdr 1's `herdr` is never called.
use super::*;
use crate::test_executable;

/// A `herdr-core` whose agent in `t9` reports `status` once started.
fn fake_core(dir: &Path, status: &str) {
    let script = format!(
        r#"#!/bin/sh
printf '%s\n' "$*" >> "$HOME/core-calls"
case "$1" in
run) echo '{{"address":"t9"}}' ;;
wait) echo '{{"ended":true,"exit_code":null,"reason":"status","status":"{status}","turn":0}}' ;;
*) echo '{{}}' ;;
esac
"#
    );
    test_executable::write(dir.join("home/.local/bin/herdr-core"), &script, 0o755).unwrap();
}

fn core_calls(dir: &Path) -> Vec<String> {
    std::fs::read_to_string(dir.join("home/core-calls"))
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect()
}

fn herdr2_request() -> DispatchRequest {
    DispatchRequest {
        model: Some("opus".into()),
        terminals: Terminals::Herdr2,
        ..request()
    }
}

/// Where Herdr 1 would have filed the dispatch's worktree.
fn checkout(dir: &Path) -> PathBuf {
    dir.join("home/.herdr/worktrees/app/377-icons")
}

#[test]
fn a_herdr_2_dispatch_types_the_agent_into_a_core_shell_and_records_its_terminal() {
    let dir = tempfile::tempdir().unwrap();
    let site = site(dir.path(), "");
    fake_core(dir.path(), "idle");
    let request = Action::Dispatch(Box::new(herdr2_request()));
    let done = perform(&site, &request, &AtomicBool::new(false)).unwrap();
    assert_eq!(done, Done::Started);

    assert!(calls(dir.path()).is_empty(), "{:?}", calls(dir.path()));
    let checkout = checkout(dir.path());
    assert!(checkout.join(".git").exists());
    assert_eq!(
        core_calls(dir.path()),
        [
            format!("run --bg --json --no-parent --cwd {}", checkout.display()),
            "send --json -- t9 'claude' '--model' 'opus'".to_owned(),
            "wait --json --timeout 5 t9".to_owned(),
            "send --json -- t9 Fix the icons; don't break 'quotes'.".to_owned(),
        ]
    );
    let store = Store::open(&site.database).unwrap();
    let run = store.runs().unwrap().remove(0);
    assert_eq!(run.state, RunState::Running);
    assert_eq!(run.model.as_deref(), Some("opus"));
    let workspace = run.workspace.unwrap();
    assert_eq!(workspace.backend, Backend::Herdr);
    assert_eq!(workspace.id, "");
    assert_eq!(workspace.path.as_deref(), Some(checkout.as_path()));
    let session = store.sessions().unwrap().remove(0);
    assert_eq!(
        session.pane,
        Pane::Herdr2 {
            terminal: "t9".into()
        }
    );
}

#[test]
fn a_herdr_2_run_is_stopped_messaged_and_removed_through_its_terminal() {
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
    let id = Store::open(&site.database)
        .unwrap()
        .runs()
        .unwrap()
        .remove(0)
        .id;
    let before = core_calls(dir.path()).len();

    let text = "--and the tests".to_owned();
    perform(
        &site,
        &Action::Send {
            run: id.clone(),
            text,
        },
        &cancelled,
    )
    .unwrap();
    perform(&site, &Action::Stop { run: id.clone() }, &cancelled).unwrap();
    perform(&site, &Action::Remove { run: id }, &cancelled).unwrap();

    assert_eq!(
        core_calls(dir.path())[before..],
        [
            "send --json -- t9 --and the tests",
            "keys --json -- t9 esc ctrl+c",
            "kill --json -- t9",
        ]
    );
    assert!(calls(dir.path()).is_empty());
    assert!(!checkout(dir.path()).exists());
    assert!(
        Store::open(&site.database)
            .unwrap()
            .runs()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn a_herdr_2_agent_that_never_gets_ready_is_ended_and_its_worktree_removed() {
    let dir = tempfile::tempdir().unwrap();
    let site = site(dir.path(), "");
    fake_core(dir.path(), "blocked");
    let request = Action::Dispatch(Box::new(herdr2_request()));
    let error = perform(&site, &request, &AtomicBool::new(false)).unwrap_err();
    assert!(matches!(error, Error::Work(_)), "{error:?}");

    let core = core_calls(dir.path());
    assert_eq!(core.last().map(String::as_str), Some("kill --json -- t9"));
    assert!(!core.iter().any(|call| call.contains("Fix the icons")));
    assert!(!checkout(dir.path()).exists());
    let store = Store::open(&site.database).unwrap();
    let run = store.runs().unwrap().remove(0);
    assert_eq!(run.state, RunState::Failed);
    assert!(run.workspace.is_none());
    assert!(store.sessions().unwrap().is_empty());
}
