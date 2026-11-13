//! A replacement's two durable steps, claim and child registration, lose to
//! any concurrent writer and to a close, and never assume a reply arrived.
use super::*;

fn conflict(peer: &mut crate::tests::MockCore, request: &serde_json::Value) {
    let message = json!({"jsonrpc": "2.0", "id": request["id"],
        "error": {"code": -32000, "message": "version changed", "data": {"kind": "conflict"}}});
    peer.send(
        0,
        crate::frame::Kind::Data,
        &serde_json::to_vec(&message).unwrap(),
    );
}

fn launching() -> Record {
    let mut record = record();
    record.state = State::Launching;
    record.generation = 1;
    record
}

fn pending() -> serde_json::Value {
    let mut replacement = terminal("t2");
    replacement["name"] = json!(launch_name(&ID.parse().unwrap(), 1));
    replacement["labels"] = json!({PENDING_LABEL: ID});
    replacement
}

#[test]
fn a_claim_that_loses_its_cas_race_never_publishes_a_mapping() {
    let (core, mut peer) = connected();
    let worker =
        std::thread::spawn(move || claim(&core, &ID.parse().unwrap(), "t2", 1, |_| Ok(true)));
    let get = peer.request();
    peer.answer(&get, json!({"value": launching(), "version": 4}));
    let list = peer.request();
    peer.answer(&list, json!({"terminals": [terminal("t1"), pending()]}));
    let set = peer.request();
    assert_eq!(set["params"]["version"], 4);
    conflict(&mut peer, &set);
    assert!(matches!(
        worker.join().unwrap(),
        Err(crate::Error::Refused {
            method: crate::Method::DocSet,
            ..
        })
    ));
}

#[test]
fn a_claim_whose_reply_is_lost_reports_failure_rather_than_success() {
    let (core, mut peer) = connected();
    let worker =
        std::thread::spawn(move || claim(&core, &ID.parse().unwrap(), "t2", 1, |_| Ok(true)));
    let get = peer.request();
    peer.answer(&get, json!({"value": launching(), "version": 4}));
    let list = peer.request();
    peer.answer(&list, json!({"terminals": [terminal("t1"), pending()]}));
    let _set = peer.request();
    drop(peer);
    assert!(worker.join().unwrap().is_err());
}

#[test]
fn a_claim_is_refused_once_the_original_terminal_is_gone() {
    let (core, mut peer) = connected();
    let worker =
        std::thread::spawn(move || claim(&core, &ID.parse().unwrap(), "t2", 1, |_| Ok(true)));
    let get = peer.request();
    peer.answer(&get, json!({"value": launching(), "version": 4}));
    let list = peer.request();
    peer.answer(&list, json!({"terminals": [pending()]}));
    assert!(matches!(
        worker.join().unwrap(),
        Err(crate::Error::ChatRecovery(Error::Moved))
    ));
}

#[test]
fn a_probe_failure_refuses_the_claim_instead_of_assuming_the_child_gone() {
    let (core, mut peer) = connected();
    let worker = std::thread::spawn(move || {
        claim(&core, &ID.parse().unwrap(), "t2", 1, |_| {
            Err(crate::sessions::Error::Identity.into())
        })
    });
    let get = peer.request();
    peer.answer(&get, json!({"value": launching(), "version": 4}));
    assert!(matches!(
        worker.join().unwrap(),
        Err(crate::Error::Sessions(crate::sessions::Error::Identity))
    ));
}

#[test]
fn a_child_registration_that_loses_its_cas_race_stays_starting() {
    let (core, mut peer) = connected();
    let mut child = record().child.unwrap();
    child.terminal = "t2".into();
    child.pid = 60;
    let worker = std::thread::spawn(move || child_started(&core, &ID.parse().unwrap(), 1, child));
    let mut record = launching();
    record.state = State::Starting;
    record.terminal = "t2".into();
    let get = peer.request();
    peer.answer(&get, json!({"value": record, "version": 5}));
    let set = peer.request();
    assert_eq!(set["params"]["version"], 5);
    assert_eq!(set["params"]["value"]["state"], "live");
    assert_eq!(set["params"]["value"]["child"]["pid"], 60);
    conflict(&mut peer, &set);
    assert!(matches!(
        worker.join().unwrap(),
        Err(crate::Error::Refused {
            method: crate::Method::DocSet,
            ..
        })
    ));
}

#[test]
fn a_child_registration_for_another_generation_or_terminal_is_refused() {
    for (generation, terminal) in [(2, "t2"), (1, "t3")] {
        let (core, mut peer) = connected();
        let mut child = record().child.unwrap();
        child.terminal = terminal.into();
        let worker = std::thread::spawn(move || {
            child_started(&core, &ID.parse().unwrap(), generation, child)
        });
        let mut record = launching();
        record.state = State::Starting;
        record.terminal = "t2".into();
        let get = peer.request();
        peer.answer(&get, json!({"value": record, "version": 5}));
        assert!(matches!(
            worker.join().unwrap(),
            Err(crate::Error::ChatRecovery(Error::Moved))
        ));
    }
}

#[test]
fn a_close_rereads_after_a_conflict_so_its_tombstone_lands_before_any_kill() {
    let (core, mut peer) = connected();
    let worker = std::thread::spawn(move || closed(&core, &ID.parse().unwrap()));
    let get = peer.request();
    peer.answer(&get, json!({"value": launching(), "version": 4}));
    let set = peer.request();
    assert_eq!(set["params"]["version"], 4);
    conflict(&mut peer, &set);
    // A claim won the race: the close builds on what it wrote.
    let mut claimed = launching();
    claimed.state = State::Starting;
    claimed.terminal = "t2".into();
    let get = peer.request();
    assert_eq!(get["method"], "doc.get");
    peer.answer(&get, json!({"value": claimed, "version": 5}));
    let set = peer.request();
    assert_eq!(set["params"]["version"], 5);
    assert_eq!(set["params"]["value"]["state"], "closed");
    assert_eq!(set["params"]["value"]["terminal"], "t2");
    peer.answer(&set, json!({"version": 6}));
    worker.join().unwrap().unwrap();
}

#[test]
fn a_close_does_not_retry_a_refusal_that_is_not_a_conflict() {
    let (core, mut peer) = connected();
    let worker = std::thread::spawn(move || closed(&core, &ID.parse().unwrap()));
    let get = peer.request();
    peer.answer(&get, json!({"value": launching(), "version": 4}));
    let set = peer.request();
    let message = json!({"jsonrpc": "2.0", "id": set["id"],
        "error": {"code": -32000, "message": "too large", "data": {"kind": "limit"}}});
    peer.send(
        0,
        crate::frame::Kind::Data,
        &serde_json::to_vec(&message).unwrap(),
    );
    assert!(matches!(
        worker.join().unwrap(),
        Err(crate::Error::Refused { kind: Some(kind), .. }) if kind == "limit"
    ));
}

#[test]
fn a_failed_relabel_after_a_durable_claim_still_lets_the_replacement_start() {
    let (core, mut peer) = connected();
    let worker =
        std::thread::spawn(move || claim(&core, &ID.parse().unwrap(), "t2", 1, |_| Ok(true)));
    let mut record = launching();
    record.previous = vec!["t1".into()];
    let get = peer.request();
    peer.answer(&get, json!({"value": record, "version": 4}));
    let list = peer.request();
    peer.answer(&list, json!({"terminals": [terminal("t1"), pending()]}));
    let set = peer.request();
    peer.answer(&set, json!({"version": 5}));
    let label = peer.request();
    assert_eq!(label["method"], "terminal.label");
    let message = json!({"jsonrpc": "2.0", "id": label["id"],
        "error": {"code": -32000, "message": "no such terminal", "data": {"kind": "not_found"}}});
    peer.send(
        0,
        crate::frame::Kind::Data,
        &serde_json::to_vec(&message).unwrap(),
    );
    worker.join().unwrap().unwrap();
}

#[test]
fn a_replacement_pending_its_claim_or_labels_keeps_workers_polling() {
    let labels = |pairs: &[(&str, &str)]| -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect()
    };
    assert!(tracked(&labels(&[(TOKEN, ID)])));
    assert!(tracked(&labels(&[(PENDING_LABEL, ID)])));
    // A claimed replacement clears its pending label to "".
    assert!(!tracked(&labels(&[(PENDING_LABEL, "")])));
    assert!(!tracked(&labels(&[])));
}
