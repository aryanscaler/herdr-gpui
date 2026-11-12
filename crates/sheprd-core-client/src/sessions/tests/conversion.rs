use super::*;
use crate::tests::connected;
use std::{
    path::Path,
    sync::{Arc, Mutex, atomic::AtomicBool},
};

#[test]
fn non_claude_conversion_uses_the_bound_session_and_configuration() {
    for (kind, agent) in [
        (AgentKind::Codex, sheprd_chat_protocol::Agent::Codex),
        (AgentKind::Copilot, sheprd_chat_protocol::Agent::Copilot),
        (AgentKind::Opencode, sheprd_chat_protocol::Agent::Opencode),
    ] {
        let (core, mut peer) = connected();
        let directory = tempfile::tempdir().unwrap();
        let cwd = directory.path().to_str().unwrap().to_owned();
        let pid = std::process::id();
        let mut environment = BTreeMap::new();
        if kind == AgentKind::Codex {
            environment.insert("CODEX_HOME".into(), "/custom/codex".into());
        }
        let binding = Binding {
            version: 1,
            sequence: 1,
            identity: Identity {
                core: "c0ffee".into(),
                terminal: "t1".into(),
                pid,
                started: process::read(pid).unwrap().started,
            },
            agent: kind.name().into(),
            session: Some(
                sessions::reference(kind, SessionKind::Id, "exact-session-123".into()).unwrap(),
            ),
            environment,
        };
        let sent = Arc::new(Mutex::new(Vec::new()));
        let received = sent.clone();
        let server = std::thread::spawn(move || {
            let mut stopped = false;
            loop {
                let request = peer.request();
                let method = request["method"].as_str().unwrap();
                let result = match method {
                    "terminal.list" => {
                        let (pid, name) = if stopped {
                            (100, "zsh")
                        } else {
                            (pid, kind.name())
                        };
                        json!({"terminals":[{"address":"t1", "state":"live", "pid":100,
                            "cwd":cwd, "status":{"app":kind.name(),"state":"idle"},
                            "foreground":{"pid":pid,"name":name,"argv":[name],"cwd":cwd}}]})
                    }
                    "doc.get" => json!({"value":binding,"version":1}),
                    "terminal.keys" => {
                        stopped = true;
                        json!({})
                    }
                    "terminal.send" => {
                        received
                            .lock()
                            .unwrap()
                            .push(request["params"]["text"].as_str().unwrap().to_owned());
                        json!({"turn":1})
                    }
                    "terminal.label" => crate::tests::terminal("t1"),
                    _ => panic!("unexpected {method}"),
                };
                peer.answer(&request, result);
                if method == "terminal.label" {
                    break;
                }
            }
        });
        crate::convert::run(
            &core,
            &sheprd_backend::TerminalId::new("t1").unwrap(),
            agent,
            Path::new("/opt/sheprd-acp"),
            Path::new("/unused-home"),
            &AtomicBool::new(false),
        )
        .unwrap();
        server.join().unwrap();
        let lines = sent.lock().unwrap();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("'--resume' 'exact-session-123'"));
        if kind == AgentKind::Codex {
            assert!(lines[0].contains("'CODEX_HOME=/custom/codex'"));
        }
    }
}
