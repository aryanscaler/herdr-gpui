//! A restart interrupted after it ended the previous agent, or before it
//! launched the next, can be retried: the core's inventory proves nothing of
//! the run is left to end. Without that proof the retry still refuses.
use super::*;

fn dispatched(dir: &Path) -> (Site, Store, String) {
    let site = site(dir, "");
    fake_core(dir, "idle");
    perform(
        &site,
        &Action::Dispatch(Box::new(herdr2_request())),
        &AtomicBool::new(false),
    )
    .unwrap();
    let store = Store::open(&site.database).unwrap();
    let id = store.runs().unwrap()[0].id.clone();
    (site, store, id)
}

fn runs(dir: &Path) -> usize {
    core_calls(dir)
        .iter()
        .filter(|call| call.starts_with("run "))
        .count()
}

fn restart(site: &Site, id: &str) -> Result<Done> {
    perform(
        site,
        &Action::Restart { run: id.into() },
        &AtomicBool::new(false),
    )
}

#[test]
fn a_restart_stopped_after_ending_the_previous_agent_continues() {
    let dir = tempfile::tempdir().unwrap();
    let (site, store, id) = dispatched(dir.path());
    // Ended, but the app stopped before recording the next agent.
    std::fs::write(
        dir.path().join("home/core-inventory"),
        r#"{"terminals":[]}"#,
    )
    .unwrap();
    restart(&site, &id).unwrap();
    assert_eq!(runs(dir.path()), 2);
    assert_eq!(store.runs().unwrap()[0].state, RunState::Running);
    assert!(matches!(
        store.sessions().unwrap()[0].pane,
        Pane::Herdr2 { .. }
    ));
    assert!(
        !core_calls(dir.path())
            .iter()
            .any(|call| call.starts_with("kill "))
    );
}

#[test]
fn a_restart_stopped_before_launching_the_next_agent_continues() {
    let dir = tempfile::tempdir().unwrap();
    let (site, store, id) = dispatched(dir.path());
    let session = store.sessions().unwrap().remove(0);
    store
        .claim_restart(&id, &session.agent_name, "next-never-ran")
        .unwrap();
    std::fs::write(
        dir.path().join("home/core-inventory"),
        r#"{"terminals":[]}"#,
    )
    .unwrap();
    restart(&site, &id).unwrap();
    assert_eq!(runs(dir.path()), 2);
    let session = store.sessions().unwrap().remove(0);
    assert_ne!(session.agent_name, "next-never-ran");
    assert!(matches!(session.pane, Pane::Herdr2 { .. }));
}

#[test]
fn an_unproven_absence_still_refuses_the_restart() {
    let dir = tempfile::tempdir().unwrap();
    let (site, store, id) = dispatched(dir.path());
    let session = store.sessions().unwrap().remove(0);
    store
        .claim_restart(&id, &session.agent_name, "next-never-ran")
        .unwrap();
    // Something else now works in the checkout: never launch beside it.
    let shell = serde_json::json!({"terminals": [{
        "address": "t4",
        "cwd": checkout(dir.path()),
        "argv": ["zsh"],
        "state": "live",
    }]});
    std::fs::write(dir.path().join("home/core-inventory"), shell.to_string()).unwrap();
    assert!(restart(&site, &id).is_err());
    assert_eq!(runs(dir.path()), 1);
    assert!(checkout(dir.path()).exists());
}
