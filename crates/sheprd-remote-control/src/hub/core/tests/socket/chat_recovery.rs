use super::*;
use sheprd_chat_protocol::{Agent, ENDPOINT_ENV, TOKEN};
use std::path::Path;

#[test]
fn a_lost_chat_keeps_its_terminal_and_folder_like_a_desktop_chat() {
    let (source, fake) = fake();
    source.poll().unwrap();
    source
        .open_chat(Agent::Claude, Path::new("/bin/sheprd-acp"), "/code", None)
        .unwrap();
    let request = fake.inputs.recv_timeout(Duration::from_secs(2)).unwrap();
    let request: Value = serde_json::from_slice(&request).unwrap();
    assert_eq!(request["method"], "terminal.run");
    let params = &request["params"];
    assert_eq!(params["keep"], true);
    assert_eq!(params["cwd"], "/code");
    assert_eq!(params["env"]["HERDR_PANE_ID"], "");
    assert!(params["labels"][TOKEN].as_str().is_some());
    assert_eq!(params["labels"][TOKEN], params["env"][ENDPOINT_ENV]);

    // Ordinary shells still recover at the same address after program loss.
    source.open_shell("/code", None).unwrap();
    let request = fake.inputs.recv_timeout(Duration::from_secs(2)).unwrap();
    let request: Value = serde_json::from_slice(&request).unwrap();
    assert_eq!(request["params"]["keep"], true);
    drop(source);
    fake.server.join().unwrap();
}

#[test]
fn the_recovery_shell_is_a_terminal_not_a_chat_despite_its_saved_label() {
    let mut terminal = record("t9");
    terminal["labels"] = json!({TOKEN: "0123456789abcdef0123456789abcdef"});
    terminal["keep"] = json!(true);
    terminal["reason"] = json!("lost");
    terminal["pid"] = json!(42);
    terminal["foreground"] = json!({"pid": 42, "name": "zsh", "argv": ["zsh"], "cwd": "/code"});
    let (source, fake) = fake_with_doc(false, Value::Null, vec![terminal]);
    let layout = source.poll().unwrap();
    assert!(layout.chat(&id("t9")).is_none());
    assert!(layout.terminal(&id("t9")).is_some());
    drop(source);
    fake.server.join().unwrap();
}

#[test]
fn a_phone_replays_durable_replacement_labels_into_saved_tabs_after_restart() {
    let endpoint = "0123456789abcdef0123456789abcdef";
    let mut old = record("t1");
    old["labels"] = json!({TOKEN: endpoint});
    old["foreground"] = json!({"pid": 123, "name": "zsh"});
    let mut replacement = record("t9");
    replacement["labels"] =
        json!({TOKEN: endpoint, sheprd_core_client::chat_recovery::REPLACES_LABEL: "[\"t1\"]"});
    replacement["foreground"] = json!({"pid": 124, "name": "sheprd-acp"});
    let (mut source, fake) = fake_with_doc(false, Value::Null, vec![old, replacement]);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("browser-tabs.json");
    std::fs::write(
        &path,
        serde_json::to_vec(&json!({"tabs": [{
            "id": 4, "scope": "local:/test.sock", "workspace_id": "w1", "title": "saved chat",
            "location": {"kind": "core_terminal", "panes": "t1", "names": {"t1": "mine"}}
        }]}))
        .unwrap(),
    )
    .unwrap();
    let state = Arc::get_mut(&mut source).unwrap();
    state.scope = Some("local:/test.sock".into());
    state.tabs = Some(path);
    for _ in 0..2 {
        let layout = source.poll().unwrap();
        let (workspace, chat) = layout.chat(&id("t9")).unwrap();
        assert_eq!(workspace.id, "w1");
        assert_eq!(chat.endpoint.as_str(), endpoint);
        assert!(layout.terminal(&id("t1")).is_none());
        // Simulate a new reader with no RAM aliases. The tabs file still
        // names t1; labels in core are the durable adoption receipt.
        *lock(&source.poller) = Default::default();
    }
    drop(source);
    fake.server.join().unwrap();
}
