use super::*;

#[cfg(unix)]
#[test]
fn a_named_remote_login_reaches_the_helper_with_its_original_endpoint_selection() {
    use std::os::unix::fs::PermissionsExt;
    let home = tempfile::tempdir().unwrap();
    let directory = home.path().join(".herdr/bin");
    std::fs::create_dir_all(&directory).unwrap();
    let program = directory.join("sheprd-acp");
    std::fs::write(&program, "#!/bin/sh\nprintf '%s|%s|%s|%s|%s' \"$HC_SESSION\" \"$HC_SOCKET\" \"$HC_TERMINAL_ID\" \"$HC_TERMINAL_SOCKET\" \"$SHEPRD_EXPECT_CORE\"\n").unwrap();
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
    let output = std::process::Command::new("/bin/sh")
        .args([
            "-c",
            &script("core-one", &TerminalId::new("t1").unwrap(), Agent::Codex),
        ])
        .env_clear()
        .env("HOME", home.path())
        .env("PATH", "/usr/bin:/bin")
        .env("HC_SESSION", "named")
        .env("HC_SOCKET", "/host/named.sock")
        .env("HC_TERMINAL_ID", "unrelated-terminal")
        .env("HC_TERMINAL_SOCKET", "/unrelated/core.sock")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout, b"named|/host/named.sock|||core-one");
}

#[test]
fn conversion_keeps_host_core_selection_and_requires_the_captured_core() {
    let terminal = TerminalId::new("t1").unwrap();
    let script = script("core-one", &terminal, Agent::Codex);
    assert!(script.starts_with("unset HC_TERMINAL_ID HC_TERMINAL_SOCKET;"));
    assert!(!script.contains("unset HC_SOCKET"));
    assert!(!script.contains("unset HC_SESSION"));
    assert!(script.contains(&format!(
        "export SHEPRD_EXPECT_CORE={}",
        shell_quote("core-one")
    )));
    assert!(script.contains(&format!("session-convert {} codex", shell_quote("t1"))));
    assert!(!script.contains("/Users/"));
}

#[test]
fn cancelling_before_conversion_never_contacts_the_host() {
    let owner = Owner {
        core: "core-one".into(),
        host: Host::Remote(crate::browser::Scope::endpoint("ssh:box")),
        target: ConnectTarget::Ssh {
            target: "invalid.example".into(),
            session: "default".into(),
        },
    };
    let result = run(
        &owner,
        &TerminalId::new("t1").unwrap(),
        Agent::Codex,
        &AtomicBool::new(true),
    );
    assert!(
        matches!(result, Err(Error::Connect(error)) if error.kind() == std::io::ErrorKind::Interrupted)
    );
}
