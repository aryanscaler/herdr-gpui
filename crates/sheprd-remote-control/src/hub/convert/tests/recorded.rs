//! A Claude session Herdr's integration does not report: Claude Code's own
//! record names it, as moving an agent is Sheprd's feature.

use super::*;

#[tokio::test(flavor = "multi_thread")]
async fn without_the_integration_claudes_own_record_names_the_session() {
    let dir = tempfile::tempdir().unwrap();
    let requests = serve_in(dir.path(), "idle", "/tmp", false);
    let home = dir.path().join("home");
    let record = convert::claude_record(&home, 20);
    std::fs::create_dir_all(record.parent().unwrap()).unwrap();
    std::fs::write(
        &record,
        format!(r#"{{"pid":20,"sessionId":"{SESSION}","cwd":"/tmp"}}"#),
    )
    .unwrap();
    let mut hub = Hub::offline_in(
        layout(Agent::Claude),
        None,
        Some(dir.path().join("h-client.sock")),
    );
    hub.program = Some(PathBuf::from("/app/sheprd-acp"));
    hub.home = Some(home);
    let hub = Arc::new(hub);
    hub.convert_to_chat("t2-p1", Agent::Claude).await.unwrap();
    let typed = requests
        .try_iter()
        .find(|(method, _)| method == "pane.send_input")
        .unwrap()
        .1;
    assert!(
        typed["text"]
            .as_str()
            .unwrap()
            .contains(&format!("'--resume' '{SESSION}'")),
        "{typed}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn with_neither_the_agent_keeps_running() {
    let dir = tempfile::tempdir().unwrap();
    let requests = serve_in(dir.path(), "idle", "/tmp", false);
    let mut hub = Hub::offline_in(
        layout(Agent::Claude),
        None,
        Some(dir.path().join("h-client.sock")),
    );
    hub.program = Some(PathBuf::from("/app/sheprd-acp"));
    hub.home = Some(dir.path().join("empty-home"));
    let hub = Arc::new(hub);
    let error = hub
        .convert_to_chat("t2-p1", Agent::Claude)
        .await
        .unwrap_err();
    assert!(
        matches!(
            error,
            Error::Refused(convert::Error::NoSession(Agent::Claude))
        ),
        "{error:?}"
    );
    assert!(
        !requests
            .try_iter()
            .any(|(method, _)| method == "pane.send_keys")
    );
}
