use super::*;
use crate::tests::{connected, terminal};
use serde_json::json;

mod lost_replies;
mod receipts;
mod stale_list;

fn shell() -> Terminal {
    let mut raw = terminal("t1");
    raw["keep"] = json!(true);
    raw["pid"] = json!(20);
    raw["argv"] = json!(["zsh"]);
    raw["foreground"] = json!({"pid": 20, "name": "zsh", "argv": ["zsh"], "cwd": "/work/tree"});
    serde_json::from_value(raw).unwrap()
}

#[test]
fn lost_requires_a_replaced_root_and_never_an_unknown_or_busy_foreground() {
    let record = super::super::tests::record();
    let mut raw = shell();
    assert!(lost(&record, &raw));
    raw.foreground = None;
    assert!(!lost(&record, &raw));
    raw = shell();
    raw.foreground.as_mut().unwrap().name = "vim".into();
    assert!(!lost(&record, &raw));
    raw = shell();
    raw.state = TerminalState::Finished;
    assert!(!lost(&record, &raw));
}

#[test]
fn restart_launch_preserves_session_env_and_cwd_without_any_shell_input() {
    let (core, mut peer) = connected();
    let id: EndpointId = "0123456789abcdef0123456789abcdef".parse().unwrap();
    let worker = std::thread::spawn(move || recover(&core, &id, &[shell()]));
    let record = super::super::tests::record();
    let get = peer.request();
    peer.answer(&get, json!({"value": record, "version": 7}));
    let set = peer.request();
    assert_eq!(set["method"], "doc.set");
    assert_eq!(set["params"]["version"], 7);
    assert_eq!(set["params"]["value"]["state"], "launching");
    let claimed = set["params"]["value"].clone();
    peer.answer(&set, json!({"version": 8}));
    let check = peer.request();
    assert_eq!(check["method"], "doc.get");
    peer.answer(&check, json!({"value": claimed, "version": 8}));
    let run = peer.request();
    assert_eq!(run["method"], "terminal.run");
    assert_eq!(run["params"]["cwd"], "/work/tree");
    assert_eq!(
        run["params"]["argv"],
        json!([
            "/bin/sheprd-acp",
            "--resume",
            "exact-session",
            "codex",
            "--",
            "--model",
            "chosen"
        ])
    );
    assert_eq!(run["params"]["env"]["CODEX_HOME"], "/private/profile");
    assert_eq!(run["params"]["env"][GENERATION_ENV], "1");
    assert!(run["params"]["labels"].get(REPLACES_LABEL).is_none());
    peer.answer(&run, terminal("t2"));
    worker.join().unwrap().unwrap();
}

#[test]
fn an_ambiguous_finished_launch_is_never_replayed() {
    let (core, mut peer) = connected();
    let id: EndpointId = "0123456789abcdef0123456789abcdef".parse().unwrap();
    let mut attempted = shell();
    attempted.address = "t2".into();
    attempted.name = Some(launch_name(&id, 1));
    attempted.state = TerminalState::Finished;
    let worker = std::thread::spawn(move || recover(&core, &id, &[shell(), attempted]));
    let mut record = super::super::tests::record();
    record.state = State::Launching;
    record.generation = 1;
    let get = peer.request();
    peer.answer(&get, json!({"value": record, "version": 8}));
    worker.join().unwrap().unwrap();
}

#[test]
fn an_interrupted_start_without_a_recorded_child_is_not_replayed() {
    let (core, mut peer) = connected();
    let id = "0123456789abcdef0123456789abcdef".parse().unwrap();
    let worker = std::thread::spawn(move || recover(&core, &id, &[shell()]));
    let mut record = super::super::tests::record();
    record.state = State::Starting;
    let get = peer.request();
    peer.answer(&get, json!({"value": record, "version": 8}));
    worker.join().unwrap().unwrap();
}

#[test]
fn a_competing_claim_refusal_starts_no_terminal() {
    let (core, mut peer) = connected();
    let id = "0123456789abcdef0123456789abcdef".parse().unwrap();
    let worker = std::thread::spawn(move || recover(&core, &id, &[shell()]));
    let get = peer.request();
    peer.answer(
        &get,
        json!({"value": super::super::tests::record(), "version": 7}),
    );
    let set = peer.request();
    assert_eq!(set["method"], "doc.set");
    peer.send(0, crate::frame::Kind::Data, &serde_json::to_vec(&json!({
        "jsonrpc": "2.0", "id": set["id"], "error": {"code": -32000, "message": "version changed", "data": {"kind": "conflict"}}
    })).unwrap());
    assert!(matches!(
        worker.join().unwrap(),
        Err(crate::Error::Refused {
            method: crate::Method::DocSet,
            ..
        })
    ));
}

#[test]
fn a_close_after_claim_cancels_the_pending_launch() {
    let (core, mut peer) = connected();
    let id = "0123456789abcdef0123456789abcdef".parse().unwrap();
    let worker = std::thread::spawn(move || recover(&core, &id, &[shell()]));
    let get = peer.request();
    let mut record = super::super::tests::record();
    peer.answer(&get, json!({"value": record, "version": 7}));
    let set = peer.request();
    peer.answer(&set, json!({"version": 8}));
    let check = peer.request();
    record.state = State::Closed;
    peer.answer(&check, json!({"value": record, "version": 9}));
    assert!(matches!(
        worker.join().unwrap(),
        Err(crate::Error::ChatRecovery(Error::Moved))
    ));
}
