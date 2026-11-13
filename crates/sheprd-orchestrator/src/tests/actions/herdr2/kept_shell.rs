//! An agent that exited leaves `--keep`'s shell in its labelled terminal:
//! nothing is typed into that shell, but restart and remove still end it.
use super::*;

/// Replaces the agent in front of `t9` with the shell `--keep` leaves.
fn agent_exited(dir: &Path) {
    let path = dir.join("home/core-inventory");
    let mut inventory: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    inventory["terminals"][0]["foreground"]["name"] = "zsh".into();
    inventory["terminals"][0]["foreground"]["argv"] = serde_json::json!(["-zsh"]);
    std::fs::write(path, inventory.to_string()).unwrap();
}

fn dispatched(dir: &Path) -> (Site, String) {
    let site = site(dir, "");
    fake_core(dir, "idle");
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
    agent_exited(dir);
    (site, run.id)
}

#[test]
fn a_message_is_never_typed_into_the_shell_an_exited_agent_left() {
    let dir = tempfile::tempdir().unwrap();
    let (site, run) = dispatched(dir.path());
    let message = Action::Send {
        run,
        text: "hello".into(),
    };
    assert!(perform(&site, &message, &AtomicBool::new(false)).is_err());
    assert!(
        !core_calls(dir.path())
            .iter()
            .any(|call| call.starts_with("send ") && call.contains("hello"))
    );
}

#[test]
fn an_exited_agent_can_be_restarted_from_its_kept_shell() {
    let dir = tempfile::tempdir().unwrap();
    let (site, run) = dispatched(dir.path());
    perform(
        &site,
        &Action::Restart { run: run.clone() },
        &AtomicBool::new(false),
    )
    .unwrap();
    let store = Store::open(&site.database).unwrap();
    assert_eq!(store.runs().unwrap()[0].state, RunState::Running);
    let calls = core_calls(dir.path());
    assert!(
        calls.iter().any(|call| call.starts_with("kill ")),
        "{calls:?}"
    );
    assert_eq!(
        calls.iter().filter(|call| call.starts_with("run ")).count(),
        2
    );
}

#[test]
fn an_exited_agent_and_its_checkout_can_be_removed() {
    let dir = tempfile::tempdir().unwrap();
    let (site, run) = dispatched(dir.path());
    perform(&site, &Action::Remove { run }, &AtomicBool::new(false)).unwrap();
    assert!(!checkout(dir.path()).exists());
    assert!(
        core_calls(dir.path())
            .iter()
            .any(|call| call.starts_with("kill "))
    );
}
