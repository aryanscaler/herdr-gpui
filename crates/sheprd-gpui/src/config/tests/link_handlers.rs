use super::*;

#[test]
fn link_handler_configuration_is_checked_and_compiled() -> anyhow::Result<()> {
    let text = r#"
[[link_handlers]]
plugin = "fixture"
id = "notes"
host = "local"
pattern = '^file://.*\.md$'
directory = "/trusted/plugin"
command = ["./open-link"]
"#;
    let config = Config::parse_over(text, &Daemon::default())?;
    assert!(config.unknown_keys.is_empty());
    assert!(
        config
            .link_handlers
            .matching("local", "file:///notes.md")
            .is_some()
    );
    assert!(
        config
            .link_handlers
            .matching("ssh:other", "file:///notes.md")
            .is_none()
    );
    assert!(
        Config::parse_over(&text.replace("^file://.*\\.md$", "["), &Daemon::default()).is_err()
    );
    Ok(())
}

#[test]
fn manifest_opt_in_is_validated_without_reading_or_running_it() -> anyhow::Result<()> {
    let config = Config::parse(
        r#"[[link_plugins]]
host = "ssh:fixture"
directory = "/not-installed-on-this-machine"
enabled = false
"#,
    )?;
    assert!(config.unknown_keys.is_empty());
    assert_eq!(config.link_plugins.len(), 1);
    assert!(!config.link_plugins[0].enabled);
    assert!(
        Config::parse(
            r#"[[link_plugins]]
host = "local"
directory = "relative"
"#
        )
        .is_err()
    );
    Ok(())
}
