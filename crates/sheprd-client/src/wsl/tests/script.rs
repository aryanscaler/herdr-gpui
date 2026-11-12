use super::*;

#[test]
fn a_protocol_script_rejects_an_invalid_distribution_before_execution() {
    assert!(matches!(
        script_command("-d", "exit 0"),
        Err(crate::Error::Wsl(Error::InvalidDistro))
    ));
}

#[cfg(not(windows))]
#[test]
fn a_protocol_script_never_runs_in_the_local_shell_without_wsl() {
    assert!(matches!(
        script_command("Ubuntu", "exit 0"),
        Err(crate::Error::Wsl(Error::Unsupported))
    ));
}

#[cfg(windows)]
#[test]
fn a_protocol_script_leaves_stdin_for_frames_and_avoids_wsl_variable_expansion() {
    let script = "exec \"$HOME/bin/sheprd-acp\" connect 0123456789abcdef0123456789abcdef";
    let command = script_command("Ubuntu", script).unwrap();
    let args: Vec<_> = command.get_args().collect();
    assert_eq!(
        args,
        [
            "--distribution",
            "Ubuntu",
            "--cd",
            "~",
            "--exec",
            "/bin/sh",
            "-c",
            script
        ]
    );
    assert!(
        command
            .get_program()
            .to_string_lossy()
            .ends_with("System32\\wsl.exe")
    );
}
