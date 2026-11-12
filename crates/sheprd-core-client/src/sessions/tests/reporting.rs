use super::*;
use crate::{frame::Kind, tests::connected};

#[test]
fn native_records_without_matching_birth_or_with_excess_bytes_are_refused() {
    let home = tempfile::tempdir().unwrap();
    let pid = std::process::id();
    let path = crate::convert::claude_record(home.path(), pid);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    for value in [
        json!({"pid":pid,"sessionId":"session"}),
        json!({"pid":pid,"sessionId":"session","procStart":"previous lifetime"}),
        json!({"pid":pid+1,"sessionId":"session","procStart":"previous lifetime"}),
    ] {
        std::fs::write(&path, value.to_string()).unwrap();
        assert!(claude_record(pid, home.path()).is_err());
    }
    std::fs::write(&path, vec![b' '; sessions::MAX_RECORD + 1]).unwrap();
    assert!(matches!(
        claude_record(pid, home.path()),
        Err(crate::Error::Sessions(Error::Evidence(
            sessions::Error::TooLarge
        )))
    ));
}

#[test]
fn native_claude_fallback_ignores_old_processes_but_not_current_clears() {
    let home = tempfile::tempdir().unwrap();
    let pid = std::process::id();
    let output = sheprd_client::new_command("ps")
        .args(["-o", "lstart=", "-p", &pid.to_string()])
        .env("LC_ALL", "C")
        .env("TZ", "UTC")
        .output()
        .unwrap();
    assert!(output.status.success());
    let stamp = String::from_utf8(output.stdout).unwrap();
    let path = crate::convert::claude_record(home.path(), pid);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        path,
        json!({"pid":pid,"sessionId":"native-exact","procStart":stamp.trim()}).to_string(),
    )
    .unwrap();
    for mode in ["absent", "old-process", "clear"] {
        let (core, mut peer) = connected();
        let identity = Identity {
            core: "c0ffee".into(),
            terminal: "t1".into(),
            pid,
            started: process::read(pid).unwrap().started,
        };
        let value = if mode == "absent" {
            serde_json::Value::Null
        } else {
            let mut binding = Binding {
                version: 1,
                sequence: 1,
                identity,
                agent: "claude".into(),
                session: None,
                environment: BTreeMap::new(),
            };
            if mode == "old-process" {
                binding.identity.started = "previous-birth".into();
            }
            serde_json::to_value(binding).unwrap()
        };
        let server = std::thread::spawn(move || {
            let mut lists = 0;
            loop {
                let request = peer.request();
                match request["method"].as_str().unwrap() {
                    "terminal.list" => {
                        lists += 1;
                        peer.answer(
                            &request,
                            json!({"terminals":[{"address":"t1","state":"live","pid":100,
                            "foreground":{"pid":pid,"name":"claude","argv":["claude"]}}]}),
                        );
                        if lists == 2 {
                            break;
                        }
                    }
                    "doc.get" => peer.answer(&request, json!({"value":value,"version":1})),
                    method => panic!("unexpected {method}"),
                }
            }
        });
        let result = discover_in(&core, "t1", AgentKind::Claude, Some(home.path()));
        if mode == "clear" {
            assert!(matches!(
                result,
                Err(crate::Error::Sessions(Error::Cleared))
            ));
        } else {
            let binding = result.unwrap();
            assert_eq!(binding.selected().unwrap().value, "native-exact");
            assert_eq!(
                binding.environment["CLAUDE_CONFIG_DIR"],
                home.path().join(".claude").to_str().unwrap()
            );
        }
        server.join().unwrap();
    }
}

#[test]
fn a_racing_unordered_session_start_clears_the_winners_identity() {
    let (core, mut peer) = connected();
    let pid = std::process::id();
    let started = process::read(pid).unwrap().started;
    let server = std::thread::spawn(move || {
        let mut raced = false;
        loop {
            let request = peer.request();
            match request["method"].as_str().unwrap() {
                "terminal.list" => peer.answer(
                    &request,
                    json!({"terminals":[{
                        "address":"t1","state":"live","pid":100,
                        "foreground":{"pid":pid,"name":"copilot","argv":["copilot"]}
                    }]}),
                ),
                "doc.get" => {
                    let value = if raced {
                        serde_json::to_value(Binding {
                            version: 1,
                            sequence: 0,
                            identity: Identity {
                                core: "c0ffee".into(),
                                terminal: "t1".into(),
                                pid,
                                started: started.clone(),
                            },
                            agent: "copilot".into(),
                            session: Some(
                                sessions::reference(
                                    AgentKind::Copilot,
                                    SessionKind::Id,
                                    "other-session".into(),
                                )
                                .unwrap(),
                            ),
                            environment: BTreeMap::new(),
                        })
                        .unwrap()
                    } else {
                        serde_json::Value::Null
                    };
                    peer.answer(&request, json!({"value":value,"version":u64::from(raced)}));
                }
                "doc.set" if !raced => {
                    raced = true;
                    let error = json!({"jsonrpc":"2.0","id":request["id"],"error":{"code":-32000,"message":"conflict","data":{"kind":"conflict"}}});
                    peer.send(0, Kind::Data, &serde_json::to_vec(&error).unwrap());
                }
                "doc.set" => {
                    assert_eq!(
                        request["params"]["value"]["session"],
                        serde_json::Value::Null
                    );
                    peer.answer(&request, json!({"version":2}));
                    break;
                }
                method => panic!("unexpected {method}"),
            }
        }
    });
    report(
        &core,
        "t1",
        AgentKind::Copilot,
        br#"{"hook_event_name":"SessionStart","session_id":"our-session"}"#,
    )
    .unwrap();
    server.join().unwrap();
}
