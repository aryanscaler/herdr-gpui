use super::*;
use crate::tests::{connected, terminal};
use serde_json::json;

mod races;

const ID: &str = "0123456789abcdef0123456789abcdef";

#[test]
fn a_chat_launch_records_only_explicit_overrides_and_clears_inherited_generation() {
    let (core, mut peer) = connected();
    let worker = std::thread::spawn(move || {
        run(
            &core,
            &Run {
                argv: &["/bin/sheprd-acp".into(), "codex".into()],
                cwd: Some("/work"),
                env: BTreeMap::from([
                    ("CODEX_HOME", "/profile"),
                    (ENDPOINT_ENV, ID),
                    (GENERATION_ENV, "99"),
                ]),
                labels: BTreeMap::from([(TOKEN, ID)]),
                keep: true,
                ..Run::default()
            },
        )
    });
    let request = peer.request();
    assert_eq!(request["method"], "terminal.run");
    let params = &request["params"];
    assert_eq!(params["env"][GENERATION_ENV], "");
    let overrides: serde_json::Value =
        serde_json::from_str(params["env"][OVERRIDES_ENV].as_str().unwrap()).unwrap();
    assert_eq!(overrides, json!({"CODEX_HOME": "/profile"}));
    peer.answer(&request, terminal("t1"));
    worker.join().unwrap().unwrap();
}

#[test]
fn recovery_validation_accepts_host_absolute_paths_but_not_relative_paths_or_nul_arguments() {
    let mut saved = record();
    for path in ["/work/tree", r"C:\work\tree", r"\\host\share\tree"] {
        saved.cwd = path.into();
        validate(&saved).unwrap();
    }
    saved.cwd = "relative".into();
    assert!(matches!(
        validate(&saved),
        Err(crate::Error::ChatRecovery(Error::Invalid))
    ));
    saved.cwd = "/work".into();
    saved.args.push("bad\0argument".into());
    assert!(matches!(
        validate(&saved),
        Err(crate::Error::ChatRecovery(Error::Invalid))
    ));
}

pub(super) fn record() -> Record {
    Record {
        adapter: false,
        args: vec!["--model".into(), "chosen".into()],
        agent: "codex".into(),
        child: Some(Identity {
            core: "c0ffee".into(),
            terminal: "t1".into(),
            pid: 50,
            started: "birth:50".into(),
        }),
        cwd: "/work/tree".into(),
        env: BTreeMap::from([("CODEX_HOME".into(), "/private/profile".into())]),
        generation: 0,
        labels: BTreeMap::from([(TOKEN.into(), ID.into())]),
        previous: Vec::new(),
        program: "/bin/sheprd-acp".into(),
        root_argv: vec!["/bin/sheprd-acp".into(), "codex".into()],
        root_pid: Some(10),
        session: "exact-session".into(),
        state: State::Live,
        terminal: "t1".into(),
        version: 1,
    }
}

#[test]
fn closing_before_the_session_opens_leaves_a_durable_tombstone() {
    let (core, mut peer) = connected();
    let worker = std::thread::spawn(move || closed(&core, &ID.parse().unwrap()));
    let get = peer.request();
    assert_eq!(get["method"], "doc.get");
    peer.answer(&get, json!({"value": null, "version": 0}));
    let set = peer.request();
    assert_eq!(set["method"], "doc.set");
    assert_eq!(set["params"]["version"], 0);
    assert_eq!(set["params"]["value"]["state"], "closed");
    peer.answer(&set, json!({"version": 1}));
    worker.join().unwrap().unwrap();
}

#[test]
fn a_late_start_cannot_claim_a_closed_or_newer_generation() {
    for (state, generation) in [(State::Closed, 1), (State::Launching, 2)] {
        let (core, mut peer) = connected();
        let worker =
            std::thread::spawn(move || claim(&core, &ID.parse().unwrap(), "t2", 1, |_| Ok(true)));
        let mut record = record();
        record.state = state;
        record.generation = generation;
        let get = peer.request();
        peer.answer(&get, json!({"value": record, "version": 4}));
        assert!(matches!(
            worker.join().unwrap(),
            Err(crate::Error::ChatRecovery(Error::Closed | Error::Moved))
        ));
    }
}

#[test]
fn a_replacement_claim_binds_its_terminal_and_publishes_mapping_after_cas() {
    let (core, mut peer) = connected();
    let worker =
        std::thread::spawn(move || claim(&core, &ID.parse().unwrap(), "t2", 1, |_| Ok(true)));
    let mut record = record();
    record.state = State::Launching;
    record.generation = 1;
    record.previous.push("t1".into());
    let get = peer.request();
    peer.answer(&get, json!({"value": record, "version": 4}));
    let list = peer.request();
    let mut replacement = terminal("t2");
    replacement["name"] = json!(launch_name(&ID.parse().unwrap(), 1));
    replacement["labels"] = json!({PENDING_LABEL: ID});
    peer.answer(&list, json!({"terminals": [terminal("t1"), replacement]}));
    let set = peer.request();
    assert_eq!(set["params"]["version"], 4);
    assert_eq!(set["params"]["value"]["terminal"], "t2");
    assert_eq!(set["params"]["value"]["state"], "starting");
    peer.answer(&set, json!({"version": 5}));
    let label = peer.request();
    assert_eq!(label["method"], "terminal.label");
    assert_eq!(label["params"]["terminal"], "t2");
    peer.answer(&label, replacement);
    let retired = peer.request();
    assert_eq!(retired["method"], "terminal.label");
    assert_eq!(retired["params"]["terminal"], "t1");
    peer.answer(&retired, terminal("t1"));
    worker.join().unwrap().unwrap();
}

#[test]
fn replacement_claims_refuse_a_live_child_or_missing_child_identity() {
    for missing in [false, true] {
        let (core, mut peer) = connected();
        let worker =
            std::thread::spawn(move || claim(&core, &ID.parse().unwrap(), "t2", 1, |_| Ok(false)));
        let mut record = record();
        record.state = State::Launching;
        record.generation = 1;
        if missing {
            record.child = None;
        }
        let get = peer.request();
        peer.answer(&get, json!({"value": record, "version": 7}));
        assert!(matches!(
            worker.join().unwrap(),
            Err(crate::Error::ChatRecovery(Error::ChildAlive))
        ));
    }
}

#[test]
fn a_tombstone_wins_over_a_late_child_registration() {
    let (core, mut peer) = connected();
    let child = record().child.unwrap();
    let worker = std::thread::spawn(move || child_started(&core, &ID.parse().unwrap(), 1, child));
    let mut record = record();
    record.state = State::Closed;
    let get = peer.request();
    peer.answer(&get, json!({"value": record, "version": 9}));
    assert!(matches!(
        worker.join().unwrap(),
        Err(crate::Error::ChatRecovery(Error::Closed))
    ));
}

#[test]
fn durable_aliases_collapse_history_and_refuse_other_chat_ownership() {
    let a = BTreeMap::from([(TOKEN.to_owned(), ID.to_owned())]);
    let mut b = a.clone();
    b.insert(REPLACES_LABEL.into(), r#"["t1"]"#.into());
    let mut c = a.clone();
    c.insert(REPLACES_LABEL.into(), r#"["t1","t2"]"#.into());
    let mapped = mapping([("t1", &a), ("t2", &b), ("t3", &c)]);
    assert_eq!(
        mapped
            .get(&sheprd_backend::TerminalId::new("t1").unwrap())
            .unwrap()
            .as_str(),
        "t3"
    );
    assert_eq!(
        mapped
            .get(&sheprd_backend::TerminalId::new("t2").unwrap())
            .unwrap()
            .as_str(),
        "t3"
    );
    let other = BTreeMap::from([(TOKEN.into(), "abcdef0123456789abcdef0123456789".into())]);
    assert!(mapping([("t1", &other), ("t2", &b)]).is_empty());
    assert!(mapping([("t2", &b), ("t3", &b)]).is_empty());
}

#[test]
fn retired_shells_do_not_reappear_when_a_replacement_is_no_longer_live() {
    let mut old = BTreeMap::from([(TOKEN.into(), ID.into())]);
    old.insert(REPLACED_BY_LABEL.into(), "t2".into());
    let mut next = BTreeMap::from([(TOKEN.into(), ID.into())]);
    next.insert(REPLACED_BY_LABEL.into(), "t3".into());
    let map = mapping([("t1", &old), ("t2", &next)]);
    assert_eq!(
        map.get(&sheprd_backend::TerminalId::new("t1").unwrap())
            .unwrap()
            .as_str(),
        "t3"
    );
    next.insert(REPLACED_BY_LABEL.into(), "t1".into());
    assert!(mapping([("t1", &old), ("t2", &next)]).is_empty());
}
