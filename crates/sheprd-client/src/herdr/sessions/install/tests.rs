#![allow(clippy::unwrap_used)]
use super::*;

#[test]
fn custom_homes_preserve_settings_hooks_and_are_idempotent() {
    for (agent, key, filename) in [
        (AgentKind::Codex, "CODEX_HOME", "hooks.json"),
        (AgentKind::Copilot, "COPILOT_HOME", "settings.json"),
        (AgentKind::Opencode, "XDG_CONFIG_HOME", "cli.json"),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let custom = temp.path().join("custom ' home");
        let directory = if agent == AgentKind::Opencode {
            custom.join("opencode")
        } else {
            custom.clone()
        };
        std::fs::create_dir_all(&directory).unwrap();
        let original = json!({"unrelated": {"enabled": true}, "plugins": ["existing"], "hooks": {"SessionStart": [{"custom": true}], "Stop": ["keep"]}});
        let path = directory.join(filename);
        std::fs::write(&path, original.to_string()).unwrap();
        let toml = directory.join("config.toml");
        std::fs::write(
            &toml,
            "# user comment\nmodel = 'custom'\n[features]\nother = true\n",
        )
        .unwrap();
        let env = |name: &str| (name == key).then(|| custom.as_os_str().to_owned());
        run(agent, temp.path(), env).unwrap();
        let first = std::fs::read_to_string(&path).unwrap();
        let value: Value = serde_json::from_str(&first).unwrap();
        assert_eq!(value["unrelated"], original["unrelated"]);
        assert_eq!(value["hooks"]["Stop"], original["hooks"]["Stop"]);
        assert_eq!(
            value["hooks"]["SessionStart"][0],
            original["hooks"]["SessionStart"][0]
        );
        assert_eq!(value["plugins"][0], "existing");
        if agent == AgentKind::Codex {
            let text = std::fs::read_to_string(&toml).unwrap();
            assert!(text.contains("# user comment"));
            let doc = text.parse::<toml_edit::DocumentMut>().unwrap();
            assert_eq!(doc["features"]["hooks"].as_bool(), Some(true));
            assert_eq!(doc["features"]["other"].as_bool(), Some(true));
        }
        run(agent, temp.path(), env).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), first);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }
}

#[test]
fn comments_and_wrong_shapes_are_refused_before_writing_assets() {
    let temp = tempfile::tempdir().unwrap();
    let directory = temp.path().join(".copilot");
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("settings.json");
    for text in [
        "{ // retain my comment\n}",
        "{\"hooks\": null}",
        "{\"hooks\": {\"SessionStart\": {}}}",
    ] {
        std::fs::write(&path, text).unwrap();
        let error = run(AgentKind::Copilot, temp.path(), |_| None).unwrap_err();
        assert!(matches!(error, Error::Json { .. } | Error::Shape { .. }));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
        assert!(!directory.join("sheprd-session-report").exists());
    }
}

#[test]
fn invalid_toml_prevents_partial_install() {
    let temp = tempfile::tempdir().unwrap();
    let directory = temp.path().join(".codex");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("config.toml"), "[bad").unwrap();
    let error = run(AgentKind::Codex, temp.path(), |_| None).unwrap_err();
    assert!(std::error::Error::source(&error).is_some());
    assert!(matches!(error, Error::Toml { .. }));
    assert!(!directory.join("hooks.json").exists());
    assert!(!directory.join("sheprd-session-report").exists());
}

#[test]
fn moving_the_helper_replaces_only_its_owned_registration() {
    let temp = tempfile::tempdir().unwrap();
    let directory = temp.path().join(".copilot");
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("settings.json");
    let unrelated = json!({"type":"command","bash":"echo keep","timeoutSec":5});
    std::fs::write(
        &path,
        json!({"hooks":{"SessionStart":[unrelated]}}).to_string(),
    )
    .unwrap();
    for name in ["first-helper", "second-helper"] {
        let program = temp.path().join(name);
        run(AgentKind::Copilot, temp.path(), |key| {
            (key == "SHEPRD_ACP_BINARY").then(|| program.clone().into_os_string())
        })
        .unwrap();
    }
    let text = std::fs::read_to_string(path).unwrap();
    let config: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(config["hooks"]["SessionStart"][0], unrelated);
    assert_eq!(config["hooks"]["SessionStart"].as_array().unwrap().len(), 2);
    assert!(!text.contains("first-helper"));
    // On Windows the helper path is pinned inside the installed PowerShell asset.
    if !cfg!(windows) {
        assert!(text.contains("second-helper"));
    }
}

#[test]
fn carried_extension_agents_get_separately_named_producers() {
    let temp = tempfile::tempdir().unwrap();
    for (agent, path) in [
        (AgentKind::Pi, ".pi/agent/extensions/sheprd-pi-session.ts"),
        (
            AgentKind::Omp,
            ".omp/agent/extensions/sheprd-omp-session.ts",
        ),
    ] {
        run(agent, temp.path(), |_| None).unwrap();
        let source = std::fs::read_to_string(temp.path().join(path)).unwrap();
        assert!(source.contains(&format!("const AGENT = \"{}\"", agent.name())));
        assert!(!source.contains("__AGENT__"));
    }
    run(AgentKind::Letta, temp.path(), |_| None).unwrap();
    let config: Value = serde_json::from_str(
        &std::fs::read_to_string(temp.path().join(".letta/settings.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        config["hooks"]["SessionStart"][0]["hooks"][0]["quiet"],
        true
    );
}
