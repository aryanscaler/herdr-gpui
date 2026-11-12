use super::*;
use crate::store::faults;

#[test]
fn first_session_write_failure_is_recovered_without_replaying_launch_or_prompt() {
    let dir = tempfile::tempdir().unwrap();
    let site = site(dir.path(), "");
    fake_core(dir.path(), "idle");
    let _failure = faults::session_updates_after(1, 1);
    let error = perform(
        &site,
        &Action::Dispatch(Box::new(herdr2_request())),
        &AtomicBool::new(false),
    )
    .unwrap_err();
    // The run's receipt names its pending checkout before anything exists,
    // so the write that fails is the one recording the started terminal.
    let Error::Retained { source } = &error else {
        panic!("a retained agent: {error:?}");
    };
    assert!(
        matches!(&**source, Error::Database(rusqlite::Error::SqliteFailure(code, _)) if code.code == rusqlite::ErrorCode::DatabaseBusy)
    );
    assert!(std::error::Error::source(&error).is_some());
    let store = Store::open(&site.database).unwrap();
    let run = store.runs().unwrap().remove(0);
    assert_eq!(run.state, RunState::Failed);
    assert!(run.workspace.is_some());
    assert_eq!(
        store.sessions().unwrap()[0].pane,
        Pane::Herdr2 {
            terminal: "t9".into()
        }
    );
    assert_eq!(
        core_calls(dir.path())
            .iter()
            .filter(|call| call.starts_with("run "))
            .count(),
        1
    );
    assert!(
        !core_calls(dir.path())
            .iter()
            .any(|call| call.contains("Fix the icons") || call.starts_with("kill "))
    );
    assert!(checkout(dir.path()).exists());
    let cancelled = AtomicBool::new(false);
    perform(
        &site,
        &Action::Send {
            run: run.id.clone(),
            text: "inspect retained work".into(),
        },
        &cancelled,
    )
    .unwrap();
    perform(
        &site,
        &Action::Stop {
            run: run.id.clone(),
        },
        &cancelled,
    )
    .unwrap();
    perform(&site, &Action::Remove { run: run.id }, &cancelled).unwrap();
    let calls = core_calls(dir.path());
    assert!(
        calls
            .iter()
            .any(|call| call == "send --json -- t9 inspect retained work")
    );
    assert!(
        calls
            .iter()
            .any(|call| call == "keys --json -- t9 esc ctrl+c")
    );
    assert_eq!(
        calls
            .iter()
            .filter(|call| call.starts_with("kill "))
            .count(),
        1
    );
    assert!(!checkout(dir.path()).exists());
}

#[test]
fn recovery_write_is_bounded_and_keeps_the_original_database_cause() {
    let dir = tempfile::tempdir().unwrap();
    let site = site(dir.path(), "");
    fake_core(dir.path(), "idle");
    let _failure = faults::session_updates_after(1, 3);
    let error = perform(
        &site,
        &Action::Dispatch(Box::new(herdr2_request())),
        &AtomicBool::new(false),
    )
    .unwrap_err();
    let Error::Retained { source } = error else {
        panic!("a retained agent: {error:?}");
    };
    assert!(
        matches!(*source, Error::Database(rusqlite::Error::SqliteFailure(_, Some(message))) if message == "injected session write failure 3")
    );
    assert_eq!(
        faults::remaining(),
        1,
        "only the original write and one recovery write"
    );
    assert!(checkout(dir.path()).exists());
    assert!(
        !core_calls(dir.path())
            .iter()
            .any(|call| call.starts_with("kill ") || call.contains("Fix the icons"))
    );
}
