use super::*;

#[test]
fn cancellation_prevents_any_remote_command_from_being_prepared() {
    let target = ConnectTarget::Ssh {
        target: "example.test".into(),
        session: "default".into(),
    };
    let error = with_command(&target, "exit 0", &AtomicBool::new(true), |_| {
        panic!("cancelled command ran")
    })
    .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::Interrupted);
}

#[test]
fn local_and_arbitrary_socket_targets_never_become_remote_commands() {
    for target in [
        ConnectTarget::Local,
        ConnectTarget::Socket("/untrusted/socket".into()),
    ] {
        let error = with_command(&target, "exit 0", &AtomicBool::new(false), |_| {
            panic!("wrong-host command ran")
        })
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Unsupported);
    }
}

#[test]
fn malformed_ssh_target_is_rejected_before_any_child_is_started() {
    let target = ConnectTarget::Ssh {
        target: "-oProxyCommand=bad".into(),
        session: "default".into(),
    };
    assert!(
        with_command(&target, "exit 0", &AtomicBool::new(false), |_| panic!(
            "invalid target accepted"
        ))
        .is_err()
    );
}

#[test]
fn ssh_attach_script_runs_on_the_named_host_without_opening_a_local_socket() {
    let target = ConnectTarget::Ssh {
        target: "example.test".into(),
        session: "default".into(),
    };
    let script = crate::chat::remote_program_script("connect 0123456789abcdef0123456789abcdef");
    with_command(&target, &script, &AtomicBool::new(false), |command| {
        let args: Vec<_> = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert!(args.iter().any(|arg| arg == "example.test"));
        assert!(
            args.last()
                .is_some_and(|arg| arg.contains("connect 0123456789abcdef0123456789abcdef"))
        );
    })
    .unwrap();
}

#[cfg(not(windows))]
#[test]
fn wsl_on_an_unsupported_platform_is_an_error_not_a_local_fallback() {
    let target = ConnectTarget::Wsl {
        distro: "Ubuntu".into(),
        session: "default".into(),
    };
    assert!(
        with_command(&target, "exit 0", &AtomicBool::new(false), |_| panic!(
            "local fallback"
        ))
        .is_err()
    );
}
