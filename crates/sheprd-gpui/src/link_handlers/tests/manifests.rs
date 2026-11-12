#![allow(clippy::unwrap_used)]
use super::*;

fn plugin(directory: String) -> Plugin {
    Plugin {
        host: "local".into(),
        directory,
        enabled: true,
        config_directory: None,
        state_directory: None,
    }
}

const MANIFEST: &str = r#"
id = "fixture.notes"
platforms = ["linux", "macos"]
[[actions]]
id = "open"
command = ["unused"]
[[actions]]
id = "windows"
platforms = ["windows"]
command = ["unused"]
[[link_handlers]]
id = "first"
pattern = '^file://'
action = "open"
[[link_handlers]]
id = "second"
pattern = '^file://'
action = "open"
platforms = ["windows"]
[[link_handlers]]
id = "third"
pattern = '^file://'
action = "windows"
platforms = ["windows"]
[[startup]]
command = ["never-import"]
"#;

#[test]
fn manifest_order_and_each_items_platform_override_match_herdr() {
    let loaded = manifest(&plugin("/trusted".into()), MANIFEST).unwrap();
    assert_eq!(
        loaded
            .iter()
            .map(|handler| handler.id.as_str())
            .collect::<Vec<_>>(),
        ["first", "second", "third"]
    );
    assert!(loaded[0].supports(Platform::Linux));
    assert!(!loaded[0].supports(Platform::Windows));
    assert!(
        !loaded[1].supports(Platform::Windows),
        "action still inherits plugin platforms"
    );
    assert!(
        loaded[2].supports(Platform::Windows),
        "both items override plugin platforms"
    );
    assert_eq!(loaded[0].action.as_deref(), Some("open"));
    assert!(loaded.iter().all(|handler| handler.command == ["unused"]));
}

#[test]
fn missing_action_and_invalid_regex_are_typed_errors() {
    assert!(matches!(
        manifest(
            &plugin("/trusted".into()),
            &MANIFEST.replace("action = \"open\"", "action = \"absent\"")
        ),
        Err(Error::Invalid(_))
    ));
    let loaded = manifest(
        &plugin("/trusted".into()),
        &MANIFEST.replace("^file://", "["),
    )
    .unwrap();
    assert!(matches!(Handlers::compile(loaded), Err(Error::Pattern(_))));
}

fn click() -> crate::core_terminals::LinkClick {
    crate::core_terminals::LinkClick {
        handler_url: Some("file:///remote/notes.md".into()),
        target: crate::terminal::RowTarget::Malformed,
        terminal: sheprd_backend::TerminalId::new("t1").unwrap(),
        cwd: None,
        alt: false,
    }
}

#[test]
fn disabled_or_other_host_plugin_is_not_even_read() {
    let mut entry = plugin("/does-not-exist".into());
    entry.enabled = false;
    let stop = AtomicBool::new(false);
    assert!(
        !activate(
            &Handlers::default(),
            std::slice::from_ref(&entry),
            "local",
            &ConnectTarget::Local,
            &click(),
            &stop
        )
        .unwrap()
    );
    entry.enabled = true;
    entry.host = "ssh:elsewhere".into();
    assert!(
        !activate(
            &Handlers::default(),
            &[entry],
            "local",
            &ConnectTarget::Local,
            &click(),
            &stop
        )
        .unwrap()
    );
}

#[cfg(unix)]
#[test]
fn manifest_click_handles_and_unsupported_platform_falls_back_without_running_hooks() {
    let dir = tempfile::tempdir().unwrap();
    let entry = plugin(dir.path().to_str().unwrap().into());
    let manifest = MANIFEST.replace(
        "[\"unused\"]",
        "[\"/bin/sh\", \"-c\", \"printf %s handled > handled\"]",
    );
    std::fs::write(dir.path().join("herdr-plugin.toml"), &manifest).unwrap();
    let stop = AtomicBool::new(false);
    assert!(
        activate(
            &Handlers::default(),
            std::slice::from_ref(&entry),
            "local",
            &ConnectTarget::Local,
            &click(),
            &stop
        )
        .unwrap()
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("handled")).unwrap(),
        "handled"
    );
    std::fs::remove_file(dir.path().join("handled")).unwrap();
    std::fs::write(
        dir.path().join("herdr-plugin.toml"),
        manifest.replace("[\"linux\", \"macos\"]", "[\"windows\"]"),
    )
    .unwrap();
    assert!(
        !activate(
            &Handlers::default(),
            &[entry],
            "local",
            &ConnectTarget::Local,
            &click(),
            &stop
        )
        .unwrap()
    );
    assert!(!dir.path().join("handled").exists());
}
