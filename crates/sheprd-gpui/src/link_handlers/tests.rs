use super::*;

fn handler(plugin: &str, id: &str) -> Handler {
    Handler {
        platforms: None,
        action: None,
        config_directory: None,
        state_directory: None,
        plugin: plugin.into(),
        id: id.into(),
        host: "local".into(),
        pattern: "^file://".into(),
        command: vec!["unused".into()],
        directory: "/".into(),
        enabled: true,
    }
}

#[test]
fn first_matching_plugin_then_declaration_order_and_host_isolation() {
    let handlers = Handlers::compile(vec![
        handler("z", "first"),
        handler("a", "second"),
        handler("a", "first"),
    ])
    .unwrap();
    assert_eq!(
        handlers.matching("local", "file:///notes.md").unwrap().id,
        "second"
    );
    assert!(handlers.matching("ssh:other", "file:///notes.md").is_none());
    assert!(handlers.matching("local", "https://example.com").is_none());
    assert!(handlers.matching("local", "file:///x\0").is_none());
    assert!(handlers.matching("local", &"x".repeat(32769)).is_none());
}

#[test]
fn disabled_and_unmatched_handlers_decline() {
    let mut disabled = handler("a", "disabled");
    disabled.enabled = false;
    let handlers = Handlers::compile(vec![disabled, handler("z", "enabled")]).unwrap();
    assert_eq!(
        handlers.matching("local", "file:///x").unwrap().id,
        "enabled"
    );
    assert!(Handlers::default().matching("local", "file:///x").is_none());
}

#[test]
fn invalid_configuration_fails_before_any_click() {
    assert!(matches!(
        Handlers::compile(vec![handler("a", "one"), handler("a", "one")]),
        Err(Error::Invalid(_))
    ));
    let mut invalid = handler("a", "one");
    invalid.pattern = "[".into();
    let error = Handlers::compile(vec![invalid]).unwrap_err();
    assert!(std::error::Error::source(&error).is_some());
    assert!(matches!(error, Error::Pattern(_)));
    let mut invalid = handler("a", "one");
    invalid.command.clear();
    assert!(matches!(
        Handlers::compile(vec![invalid]),
        Err(Error::Invalid(_))
    ));
}

#[cfg(unix)]
#[test]
fn command_receives_literal_url_and_failure_never_invokes_another_handler() {
    let folder = tempfile::tempdir().unwrap();
    let mut configured = handler("a", "capture");
    configured.directory = folder.path().to_str().unwrap().into();
    configured.command = vec![
        "/bin/sh".into(),
        "-c".into(),
        "printf %s \"$HERDR_PLUGIN_CLICKED_URL\" > clicked; test -z \"${HERDR_PANE_ID-}\"; exit 23"
            .into(),
    ];
    let handlers = Handlers::compile(vec![configured, handler("z", "never")]).unwrap();
    let url = "file:///$(touch injected)'notes.md";
    let chosen = handlers.matching("local", url).unwrap();
    let error = run(
        chosen,
        &sheprd_client::ConnectTarget::Local,
        url,
        "term_test",
        None,
        &std::sync::atomic::AtomicBool::new(false),
    )
    .unwrap_err();
    assert!(matches!(error, Error::Run(_)));
    assert_eq!(
        std::fs::read_to_string(folder.path().join("clicked")).unwrap(),
        url
    );
    assert!(!folder.path().join("injected").exists());
}

#[cfg(unix)]
#[test]
fn handled_command_and_cancelled_command() {
    let folder = tempfile::tempdir().unwrap();
    let mut configured = handler("a", "capture");
    configured.directory = folder.path().to_str().unwrap().into();
    configured.command = vec![
        "/bin/sh".into(),
        "-c".into(),
        "printf %s ok > handled".into(),
    ];
    run(
        &configured,
        &sheprd_client::ConnectTarget::Local,
        "file:///x",
        "t",
        None,
        &std::sync::atomic::AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(
        std::fs::read_to_string(folder.path().join("handled")).unwrap(),
        "ok"
    );
    std::fs::remove_file(folder.path().join("handled")).unwrap();
    assert!(
        run(
            &configured,
            &sheprd_client::ConnectTarget::Local,
            "file:///x",
            "t",
            None,
            &std::sync::atomic::AtomicBool::new(true)
        )
        .is_err()
    );
    assert!(!folder.path().join("handled").exists());
}
