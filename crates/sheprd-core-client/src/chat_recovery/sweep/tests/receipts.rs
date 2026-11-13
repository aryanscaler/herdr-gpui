//! Receipts (`replaces` on a replacement, `replaced_by` on its retired
//! shell) are repaired until both stand, and never block a close's kills.
use super::*;

const ID: &str = "0123456789abcdef0123456789abcdef";

/// A live replacement `t2` of the retired shell `t1`.
fn replaced() -> (Record, Terminal) {
    let mut record = super::super::super::tests::record();
    record.terminal = "t2".into();
    record.previous = vec!["t1".into()];
    record.root_pid = Some(30);
    record.root_argv = vec!["/bin/sheprd-acp".into()];
    let mut replacement = shell();
    replacement.address = "t2".into();
    replacement.pid = Some(30);
    replacement.argv = vec!["/bin/sheprd-acp".into()];
    replacement.labels = BTreeMap::from([
        (TOKEN.into(), ID.into()),
        (REPLACES_LABEL.into(), r#"["t1"]"#.into()),
    ]);
    (record, replacement)
}

#[test]
fn a_retired_shell_missing_its_receipt_is_relabelled() {
    let (record, replacement) = replaced();
    let retired = shell();
    assert!(receipts_missing(
        &record,
        &replacement,
        &[retired.clone(), replacement.clone()]
    ));
    let mut labelled = retired;
    labelled.labels = BTreeMap::from([(REPLACED_BY_LABEL.into(), "t2".into())]);
    assert!(!receipts_missing(
        &record,
        &replacement,
        &[labelled, replacement.clone()]
    ));
    // A retired shell that is gone needs no receipt.
    assert!(!receipts_missing(
        &record,
        &replacement,
        std::slice::from_ref(&replacement)
    ));
}

#[test]
fn a_failed_receipt_never_keeps_a_close_from_ending_its_replacement() {
    let (core, mut peer) = connected();
    let (mut record, replacement) = replaced();
    record.state = State::Closed;
    let worker = std::thread::spawn(move || recover(&core, &ID.parse().unwrap(), &[replacement]));
    let get = peer.request();
    peer.answer(&get, json!({"value": record, "version": 9}));
    let label = peer.request();
    assert_eq!(label["method"], "terminal.label");
    peer.answer(&label, terminal("t2"));
    // The retired shell was removed: its receipt cannot be written.
    let label = peer.request();
    assert_eq!(label["params"]["terminal"], "t1");
    let message = json!({"jsonrpc": "2.0", "id": label["id"],
        "error": {"code": -32000, "message": "no such terminal", "data": {"kind": "not_found"}}});
    peer.send(
        0,
        crate::frame::Kind::Data,
        &serde_json::to_vec(&message).unwrap(),
    );
    let kill = peer.request();
    assert_eq!(kill["method"], "terminal.kill");
    assert_eq!(kill["params"]["terminal"], "t2");
    peer.answer(&kill, terminal("t2"));
    worker.join().unwrap().unwrap();
}
