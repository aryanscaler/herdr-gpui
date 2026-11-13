use super::*;

/// The record names `t2`, a replacement another worker created and claimed
/// after this pass listed terminals, so the pass's own list has only `t1`.
fn claimed_elsewhere() -> Record {
    let mut record = super::super::super::tests::record();
    record.terminal = "t2".into();
    record.previous = vec!["t1".into()];
    record.generation = 1;
    record.state = State::Starting;
    record
}

#[test]
fn a_replacement_newer_than_the_list_is_never_closed() {
    let (core, mut peer) = connected();
    let id: EndpointId = "0123456789abcdef0123456789abcdef".parse().unwrap();
    let worker = std::thread::spawn(move || recover(&core, &id, &[shell()]));
    let get = peer.request();
    peer.answer(&get, json!({"value": claimed_elsewhere(), "version": 9}));
    let list = peer.request();
    assert_eq!(list["method"], "terminal.list");
    let mut t2 = terminal("t2");
    t2["keep"] = json!(true);
    peer.answer(&list, json!({"terminals": [terminal("t1"), t2]}));
    // No doc.set follows: the pass defers rather than writing a tombstone.
    assert!(matches!(
        worker.join().unwrap(),
        Err(crate::Error::ChatRecovery(Error::Moved))
    ));
}

#[test]
fn a_terminal_gone_from_a_fresh_list_is_closed_at_the_version_read() {
    let (core, mut peer) = connected();
    let id: EndpointId = "0123456789abcdef0123456789abcdef".parse().unwrap();
    let worker = std::thread::spawn(move || recover(&core, &id, &[shell()]));
    let get = peer.request();
    peer.answer(&get, json!({"value": claimed_elsewhere(), "version": 9}));
    let list = peer.request();
    assert_eq!(list["method"], "terminal.list");
    peer.answer(&list, json!({"terminals": [terminal("t1")]}));
    let set = peer.request();
    assert_eq!(set["method"], "doc.set");
    // Fenced on the version read: a claim or close since then refuses it.
    assert_eq!(set["params"]["version"], 9);
    assert_eq!(set["params"]["value"]["state"], "closed");
    assert_eq!(set["params"]["value"]["terminal"], "t2");
    peer.answer(&set, json!({"version": 10}));
    worker.join().unwrap().unwrap();
}
