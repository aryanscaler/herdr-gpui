//! A dispatch journals its pre-start recovery identity before Git or the
//! core is touched, so a failed first session write leaves nothing to undo;
//! a failed write after the launch keeps everything the agent may be using.
use super::*;
use crate::store::faults;

#[test]
fn a_failed_prestart_record_creates_no_checkout_and_starts_no_agent() {
    let dir = tempfile::tempdir().unwrap();
    let site = site(dir.path(), "");
    fake_core(dir.path(), "idle");
    let _failure = faults::session_updates_after(0, 1);
    let error = perform(
        &site,
        &Action::Dispatch(Box::new(herdr2_request())),
        &AtomicBool::new(false),
    )
    .unwrap_err();
    assert!(
        matches!(&error, Error::Database(rusqlite::Error::SqliteFailure(code, _)) if code.code == rusqlite::ErrorCode::DatabaseBusy),
        "{error:?}"
    );
    assert!(std::error::Error::source(&error).is_some());
    let store = Store::open(&site.database).unwrap();
    let run = store.runs().unwrap().remove(0);
    assert_eq!(run.state, RunState::Failed);
    assert!(run.workspace.is_none());
    assert!(!checkout(dir.path()).exists());
    assert!(
        !core_calls(dir.path())
            .iter()
            .any(|call| call.starts_with("run ") || call.contains("Fix the icons"))
    );
}

#[test]
fn a_failed_terminal_record_is_retried_once_and_keeps_its_database_cause() {
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
