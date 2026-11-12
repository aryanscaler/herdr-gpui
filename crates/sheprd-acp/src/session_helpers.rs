//! Host-local helpers for process-bound session reports and conversion.

mod error;

pub(crate) use error::Error;
use sheprd_client::herdr::{AgentKind, sessions::MAX_RECORD};
use sheprd_core_client::{Core, Endpoint};
use std::{
    ffi::OsString,
    io::{Read, Write},
    sync::atomic::AtomicBool,
};

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Command {
    Install(AgentKind),
    Report(AgentKind),
    Discover {
        terminal: String,
        agent: AgentKind,
    },
    Convert {
        terminal: String,
        agent: sheprd_chat_protocol::Agent,
    },
}

/// Reject routed addresses before opening a connection or reading local state.
pub(crate) fn terminal(value: String) -> Result<String, crate::cli::Error> {
    if !value
        .as_bytes()
        .first()
        .is_some_and(u8::is_ascii_alphanumeric)
        || value.len() > 256
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte))
    {
        return Err(crate::cli::Error::InvalidTerminal);
    }
    Ok(value)
}

fn report_terminal(value: Option<OsString>) -> Result<String, Error> {
    let value = value.ok_or(Error::MissingTerminal)?;
    let value = value.into_string().map_err(|_| Error::TerminalEncoding)?;
    terminal(value).map_err(Error::from)
}

fn read_record(input: impl Read) -> Result<Vec<u8>, Error> {
    let mut bytes = Vec::new();
    input.take(MAX_RECORD as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > MAX_RECORD {
        return Err(Error::TooLarge { limit: MAX_RECORD });
    }
    Ok(bytes)
}

fn endpoint_variable(
    name: &str,
    report_socket: Option<&OsString>,
    env: impl Fn(&str) -> Option<OsString>,
) -> Option<OsString> {
    match (name, report_socket) {
        ("HC_SOCKET", Some(socket)) => Some(socket.clone()),
        ("HC_SESSION", Some(_)) => None,
        _ => env(name),
    }
}

pub(crate) fn run(command: Command) -> crate::Result<()> {
    execute(command).map_err(Into::into)
}

fn execute(command: Command) -> Result<(), Error> {
    if let Command::Install(agent) = command {
        let home = std::env::home_dir().ok_or(Error::NoHome)?;
        let program = std::env::current_exe()?;
        sheprd_client::herdr::sessions::install::run(agent, &home, |key| {
            if key == "SHEPRD_ACP_BINARY" {
                Some(program.as_os_str().to_owned())
            } else {
                std::env::var_os(key)
            }
        })?;
        return Ok(());
    }
    // Resolve and validate the report's environment before reading stdin or
    // connecting. The core validates the payload and its producing process.
    let report_socket = if matches!(command, Command::Report(_)) {
        Some(
            std::env::var_os("HC_TERMINAL_SOCKET")
                .filter(|socket| !socket.is_empty())
                .ok_or(Error::MissingTerminalSocket)?,
        )
    } else {
        None
    };
    let report = if matches!(command, Command::Report(_)) {
        Some((
            report_terminal(std::env::var_os("HC_TERMINAL_ID"))?,
            read_record(std::io::stdin().lock())?,
        ))
    } else {
        None
    };
    let endpoint = Endpoint::resolve(|name| {
        endpoint_variable(name, report_socket.as_ref(), |key| std::env::var_os(key))
    })?;
    let core = Core::connect(&endpoint)?;
    if let Some(expected) = std::env::var_os("SHEPRD_EXPECT_CORE")
        && expected.to_str() != Some(core.hello().core.id.as_str())
    {
        return Err(Error::CoreIdentity);
    }
    match command {
        Command::Install(_) => unreachable!("installation returned before connecting"),
        Command::Report(agent) => {
            let (terminal, bytes) = report.ok_or(Error::MissingTerminal)?;
            sheprd_core_client::sessions::report(&core, &terminal, agent, &bytes)?;
        }
        Command::Discover { terminal, agent } => {
            let binding = sheprd_core_client::sessions::discover(&core, &terminal, agent)?;
            let mut stdout = std::io::stdout().lock();
            serde_json::to_writer(&mut stdout, &binding)?;
            writeln!(stdout)?;
        }
        Command::Convert { terminal, agent } => {
            let terminal = terminal
                .try_into()
                .map_err(sheprd_core_client::Error::from)?;
            let program = std::env::current_exe()?;
            let home = std::env::home_dir().ok_or(Error::NoHome)?;
            let endpoint = sheprd_core_client::convert::run(
                &core,
                &terminal,
                agent,
                &program,
                &home,
                &AtomicBool::new(false),
            )?;
            writeln!(std::io::stdout().lock(), "{endpoint}")?;
        }
    }
    Ok(())
}

pub(crate) fn exit_code(discover: bool, error: &crate::Error) -> u8 {
    if discover
        && matches!(
            error,
            crate::Error::SessionHelpers(Error::Core(sheprd_core_client::Error::Sessions(
                sheprd_core_client::sessions::Error::Absent
                    | sheprd_core_client::sessions::Error::Cleared
            )))
        )
    {
        3
    } else {
        1
    }
}

#[cfg(test)]
mod tests;
