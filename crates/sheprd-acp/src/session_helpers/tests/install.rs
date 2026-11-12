use super::*;

#[test]
fn only_absent_discovery_has_exit_status_three() {
    let absent = crate::Error::SessionHelpers(Error::Core(sheprd_core_client::Error::Sessions(
        sheprd_core_client::sessions::Error::Absent,
    )));
    assert_eq!(exit_code(true, &absent), 3);
    assert_eq!(exit_code(false, &absent), 1);
    let invalid = crate::Error::SessionHelpers(Error::MissingTerminal);
    assert_eq!(exit_code(true, &invalid), 1);
    let io = crate::Error::SessionHelpers(Error::Io(std::io::ErrorKind::PermissionDenied.into()));
    assert_eq!(exit_code(true, &io), 1);
}

#[test]
fn installation_cli_accepts_only_supported_agents_and_help() {
    use crate::cli::{Parsed, parse};
    for (name, agent) in [
        ("claude", AgentKind::Claude),
        ("codex", AgentKind::Codex),
        ("copilot", AgentKind::Copilot),
        ("opencode", AgentKind::Opencode),
        ("pi", AgentKind::Pi),
        ("omp", AgentKind::Omp),
        ("letta", AgentKind::Letta),
    ] {
        assert_eq!(
            parse(["session-install".into(), name.into()]).unwrap(),
            Parsed::Session(Command::Install(agent))
        );
    }
    assert_eq!(
        parse(["session-install".into(), "--help".into()]).unwrap(),
        Parsed::Help
    );
    assert!(parse(["session-install".into(), "droid".into()]).is_err());
    assert!(parse(["session-install".into()]).is_err());
    assert!(parse(["session-install".into(), "codex".into(), "extra".into()]).is_err());
}
