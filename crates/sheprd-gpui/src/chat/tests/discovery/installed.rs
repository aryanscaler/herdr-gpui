use super::*;

#[cfg(unix)]
#[test]
fn chats_reuse_the_exact_acp_build_installed_for_phone_pairing() {
    use std::os::unix::fs::PermissionsExt;
    let home = tempfile::tempdir().unwrap();
    let directory = home
        .path()
        .join(".local/share/herdr-remote-control/bin")
        .join(crate::APP_VERSION);
    std::fs::create_dir_all(&directory).unwrap();
    let program = directory.join("sheprd-acp");
    std::fs::write(&program, "#!/bin/sh\nprintf 'paired ACP %s\\n' \"$1\"\n").unwrap();
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
    for line in [
        remote_program_script("agents"),
        crate::chat::start::remote_command_line(sheprd_chat_protocol::Agent::Codex),
    ] {
        let output = std::process::Command::new("/bin/sh")
            .args(["-c", &line])
            .env_clear()
            .env("HOME", home.path())
            .env("PATH", "/usr/bin:/bin")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).starts_with("paired ACP "));
    }
}
