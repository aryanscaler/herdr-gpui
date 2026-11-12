#![allow(clippy::unwrap_used)]
use super::*;

#[cfg(unix)]
#[test]
fn remote_script_preserves_literal_arguments_and_plugin_context() {
    let temporary = tempfile::tempdir().unwrap();
    let directory = temporary.path().join("plugin's directory");
    std::fs::create_dir(&directory).unwrap();
    let handler = Handler {
        platforms: None,
        plugin: "fixture".into(), id: "markdown".into(), host: "ssh:fixture".into(),
        action: Some("open-notes".into()), pattern: ".*".into(), enabled: true,
        directory: directory.to_str().unwrap().into(),
        config_directory: Some(directory.to_str().unwrap().into()),
        state_directory: Some(directory.to_str().unwrap().into()),
        command: vec!["/bin/sh".into(), "-c".into(),
            r#"test -z "${HERDR_SOCKET_PATH-}${HERDR_CLIENT_SOCKET_PATH-}${HERDR_SESSION-}${HC_TERMINAL_SOCKET-}" && test "$HERDR_PLUGIN_ACTION_ID" = open-notes && test "$HERDR_PLUGIN_ROOT" = "$PWD" && printf %s "$HERDR_PLUGIN_CONTEXT_JSON" > context && printf %s "$1" > argument"#.into(),
            "handler".into(), "literal $(touch injected)'argument".into()],
    };
    let url = "file:///$(touch injected)'notes.md";
    let text = format!(
        "export HERDR_SOCKET_PATH=/wrong/daemon HERDR_CLIENT_SOCKET_PATH=/wrong/client HERDR_SESSION=wrong HC_TERMINAL_SOCKET=/wrong/core\n{}",
        script(&handler, url, "term", Some("/remote/work tree"))
    );
    sheprd_client::run_script(
        ScriptHost::Local,
        &text,
        std::io::empty(),
        std::io::sink(),
        LIMITS,
        &AtomicBool::new(false),
    )
    .unwrap();
    let context: serde_json::Value =
        serde_json::from_slice(&std::fs::read(directory.join("context")).unwrap()).unwrap();
    assert_eq!(context["clicked_url"], url);
    assert_eq!(context["focused_pane_cwd"], "/remote/work tree");
    assert_eq!(context["invocation_source"], "link_click");
    assert_eq!(
        std::fs::read_to_string(directory.join("argument")).unwrap(),
        "literal $(touch injected)'argument"
    );
    assert!(!directory.join("injected").exists());
}
