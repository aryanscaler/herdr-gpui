use super::*;
use crate::session_helpers::Command;
use sheprd_client::herdr::AgentKind;

#[test]
fn parses_session_helpers() {
    assert_eq!(
        parse(args(&["session-report", "omp"])).unwrap(),
        Parsed::Session(Command::Report(AgentKind::Omp))
    );
    assert_eq!(
        parse(args(&["session-discover", "t7", "codex"])).unwrap(),
        Parsed::Session(Command::Discover {
            terminal: "t7".into(),
            agent: AgentKind::Codex
        })
    );
    assert_eq!(
        parse(args(&["session-convert", "t7", "claude"])).unwrap(),
        Parsed::Session(Command::Convert {
            terminal: "t7".into(),
            agent: Agent::Claude
        })
    );
}

#[test]
fn help_precedes_missing_or_invalid_arguments() {
    for command in ["session-report", "session-discover", "session-convert"] {
        for flag in ["--help", "-h"] {
            assert_eq!(parse(args(&[command, flag])).unwrap(), Parsed::Help);
            assert_eq!(
                parse(args(&[command, "host/t7", "bogus", flag])).unwrap(),
                Parsed::Help
            );
        }
        assert!(parse(args(&[command])).is_err());
        assert!(parse(args(&[command, "t7", "codex", "extra"])).is_err());
    }
}

#[test]
fn rejects_routing_and_non_identifiers() {
    for command in ["session-discover", "session-convert"] {
        for terminal in [
            "", "host/t7", "host:t7", "..", "t 7", "t7\n", "t\\7", "-flag",
        ] {
            assert_eq!(
                parse(args(&[command, terminal, "codex"])),
                Err(Error::InvalidTerminal)
            );
        }
    }
}

#[cfg(unix)]
#[test]
fn non_utf8_arguments_are_not_lossily_accepted() {
    use std::os::unix::ffi::OsStringExt;
    let invalid = OsString::from_vec(vec![0xff]);
    assert_eq!(
        parse(vec!["session-report".into(), invalid.clone()]),
        Err(Error::NotUtf8("AGENT"))
    );
    assert_eq!(
        parse(vec!["session-discover".into(), invalid, "codex".into()]),
        Err(Error::NotUtf8("TERMINAL"))
    );
}
