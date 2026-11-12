#![allow(clippy::unwrap_used)]
use super::*;
mod install;

#[test]
fn reporter_uses_the_injected_terminal_socket_not_shell_selection() {
    let socket = OsString::from("/owning/core.sock");
    let inherited = |name: &str| match name {
        "HC_SOCKET" => Some("/other/core.sock".into()),
        "HC_SESSION" => Some("other-session".into()),
        "HOME" => Some("/home/test".into()),
        _ => None,
    };
    assert_eq!(
        endpoint_variable("HC_SOCKET", Some(&socket), inherited),
        Some(socket)
    );
    assert_eq!(
        endpoint_variable(
            "HC_SESSION",
            Some(&OsString::from("/owning/core.sock")),
            inherited
        ),
        None
    );
    assert_eq!(
        endpoint_variable("HC_SOCKET", None, inherited),
        Some("/other/core.sock".into())
    );
    assert_eq!(
        endpoint_variable("HOME", None, inherited),
        Some("/home/test".into())
    );
}

#[test]
fn bounded_input_accepts_the_limit_and_reads_only_one_byte_beyond() {
    let bytes = vec![b'x'; MAX_RECORD];
    assert_eq!(read_record(bytes.as_slice()).unwrap(), bytes);
    let bytes = vec![b'x'; MAX_RECORD + 50];
    let mut input = std::io::Cursor::new(bytes);
    assert!(matches!(
        read_record(&mut input),
        Err(Error::TooLarge { limit: MAX_RECORD })
    ));
    assert_eq!(input.position(), MAX_RECORD as u64 + 1);
}

#[test]
fn input_failure_keeps_its_source() {
    struct Broken;
    impl Read for Broken {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied))
        }
    }
    let error = read_record(Broken).unwrap_err();
    assert!(std::error::Error::source(&error).is_some());
    assert!(
        matches!(error, Error::Io(source) if source.kind() == std::io::ErrorKind::PermissionDenied)
    );
}

#[test]
fn report_requires_a_local_terminal_in_its_environment() {
    assert!(matches!(report_terminal(None), Err(Error::MissingTerminal)));
    for value in ["", "host/t7", "host:t7", "t7\n"] {
        assert!(matches!(
            report_terminal(Some(value.into())),
            Err(Error::Cli(crate::cli::Error::InvalidTerminal))
        ));
    }
    assert_eq!(report_terminal(Some("t7".into())).unwrap(), "t7");
}
