use super::*;
use crate::session_helpers::{Command, terminal};
use sheprd_client::herdr::AgentKind;

pub(super) fn parse(args: impl Iterator<Item = OsString>) -> Result<Parsed, Error> {
    let Some(mut args) = unless_help(args) else {
        return Ok(Parsed::Help);
    };
    let command = args.next();
    let command = match command.as_deref().and_then(OsStr::to_str) {
        Some("session-report") => Command::Report(agent(args.next())?),
        Some("session-install") => {
            let name = utf8(args.next(), "AGENT")?;
            let agent = match AgentKind::parse(&name) {
                Some(
                    agent @ (AgentKind::Claude
                    | AgentKind::Codex
                    | AgentKind::Copilot
                    | AgentKind::Opencode
                    | AgentKind::Pi
                    | AgentKind::Omp
                    | AgentKind::Letta),
                ) => agent,
                _ => return Err(Error::SessionAgent(name)),
            };
            Command::Install(agent)
        }
        Some("session-discover") => Command::Discover {
            terminal: terminal(utf8(args.next(), "TERMINAL")?)?,
            agent: agent(args.next())?,
        },
        Some("session-convert") => {
            let terminal = terminal(utf8(args.next(), "TERMINAL")?)?;
            let name = utf8(args.next(), "AGENT")?;
            let agent = name.parse().map_err(|_| Error::UnknownAgent(name))?;
            Command::Convert { terminal, agent }
        }
        _ => return Err(Error::MissingValue("session command")),
    };
    if let Some(extra) = args.next() {
        return Err(Error::UnknownOption(extra));
    }
    Ok(Parsed::Session(command))
}

fn agent(value: Option<OsString>) -> Result<AgentKind, Error> {
    let name = utf8(value, "AGENT")?;
    AgentKind::parse(&name).ok_or(Error::SessionAgent(name))
}
