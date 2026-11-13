//! A request that times out or loses its reply may still have taken effect.
//! The next pass works from what the core then shows, never from a guess.
use super::*;

const ID: &str = "0123456789abcdef0123456789abcdef";

fn launching() -> Record {
    let mut record = super::super::super::tests::record();
    record.state = State::Launching;
    record.generation = 1;
    record.previous = vec!["t1".into()];
    record
}

#[test]
fn a_lost_claim_reply_relaunches_the_same_generation_exactly_once() {
    let (core, mut peer) = connected();
    let worker = std::thread::spawn(move || recover(&core, &ID.parse().unwrap(), &[shell()]));
    let get = peer.request();
    peer.answer(
        &get,
        json!({"value": super::super::super::tests::record(), "version": 7}),
    );
    let _set = peer.request();
    drop(peer);
    assert!(worker.join().unwrap().is_err());

    // The write had landed: the next pass must not bump the generation again.
    let (core, mut peer) = connected();
    let worker = std::thread::spawn(move || recover(&core, &ID.parse().unwrap(), &[shell()]));
    let get = peer.request();
    peer.answer(&get, json!({"value": launching(), "version": 8}));
    let check = peer.request();
    assert_eq!(check["method"], "doc.get");
    peer.answer(&check, json!({"value": launching(), "version": 8}));
    let run = peer.request();
    assert_eq!(run["method"], "terminal.run");
    assert_eq!(run["params"]["env"][GENERATION_ENV], "1");
    assert_eq!(run["params"]["name"], launch_name(&ID.parse().unwrap(), 1));
    assert_eq!(run["params"]["labels"], json!({PENDING_LABEL: ID}));
    peer.answer(&run, terminal("t2"));
    worker.join().unwrap().unwrap();
}

#[test]
fn a_launch_whose_run_reply_was_lost_is_not_started_again() {
    let (core, mut peer) = connected();
    let id: EndpointId = ID.parse().unwrap();
    let mut started = shell();
    started.address = "t2".into();
    started.name = Some(launch_name(&id, 1));
    started.labels = BTreeMap::from([(PENDING_LABEL.into(), ID.into())]);
    let worker = std::thread::spawn(move || recover(&core, &id, &[shell(), started]));
    let get = peer.request();
    peer.answer(&get, json!({"value": launching(), "version": 8}));
    // No second doc.get and no terminal.run: the pending launch owns it.
    worker.join().unwrap().unwrap();
}

#[test]
fn a_tombstone_ends_a_pending_replacement_but_never_the_retired_shell() {
    let (core, mut peer) = connected();
    let id: EndpointId = ID.parse().unwrap();
    let mut started = shell();
    started.address = "t2".into();
    started.argv = vec!["/bin/sheprd-acp".into()];
    started.name = Some(launch_name(&id, 1));
    started.labels = BTreeMap::from([(PENDING_LABEL.into(), ID.into())]);
    let worker = std::thread::spawn(move || recover(&core, &id, &[shell(), started]));
    let mut record = launching();
    record.state = State::Closed;
    record.previous.clear();
    let get = peer.request();
    peer.answer(&get, json!({"value": record, "version": 9}));
    let kill = peer.request();
    assert_eq!(kill["method"], "terminal.kill");
    assert_eq!(kill["params"]["terminal"], "t2");
    peer.answer(&kill, terminal("t2"));
    worker.join().unwrap().unwrap();
}
