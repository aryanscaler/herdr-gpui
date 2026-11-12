use sheprd_client::herdr::{AgentKind, convert::Error};

/// CLI flags and server flags differ. Unknown modes fail before the terminal
/// stops, rather than silently changing permissions or replaying a prompt.
pub(super) fn chat(agent: AgentKind, argv: &[String]) -> Result<Vec<String>, Error> {
    let flags = agent.kept_flags(argv);
    match agent {
        AgentKind::Claude => Ok(flags),
        AgentKind::Copilot => {
            let mut flags = flags;
            let mut args = argv.iter();
            while let Some(arg) = args.next() {
                if arg == "--config-dir" {
                    flags.push(arg.clone());
                    flags.push(args.next().ok_or(Error::Line)?.clone());
                } else if arg.starts_with("--config-dir=") {
                    flags.push(arg.clone());
                }
            }
            Ok(flags)
        }
        // The ACP CLI takes no model/agent flags. Its loaded session keeps
        // those choices; do not forward unsupported flags to a new server.
        AgentKind::Opencode if flags.is_empty() => Ok(flags),
        AgentKind::Codex => codex(&flags),
        _ => Err(Error::Line),
    }
}

fn codex(flags: &[String]) -> Result<Vec<String>, Error> {
    let mut result = Vec::new();
    let mut args = flags.iter();
    while let Some(arg) = args.next() {
        let (name, inline) = arg
            .split_once('=')
            .map_or((arg.as_str(), None), |(key, value)| (key, Some(value)));
        let mut value = || {
            inline
                .or_else(|| args.next().map(String::as_str))
                .ok_or(Error::Line)
        };
        let (key, value) = match name {
            "--no-alt-screen" => continue,
            "--full-auto" => {
                result.extend(
                    [
                        "-c",
                        "approval_policy=\"on-request\"",
                        "-c",
                        "sandbox_mode=\"workspace-write\"",
                    ]
                    .map(str::to_owned),
                );
                continue;
            }
            "--dangerously-bypass-approvals-and-sandbox" | "--yolo" => {
                result.extend(
                    [
                        "-c",
                        "approval_policy=\"never\"",
                        "-c",
                        "sandbox_mode=\"danger-full-access\"",
                    ]
                    .map(str::to_owned),
                );
                continue;
            }
            "--search" => ("web_search", "live"),
            "-c" | "--config" => {
                result.extend(["-c".into(), value()?.to_owned()]);
                continue;
            }
            "--enable" | "--disable" => {
                result.extend([name.to_owned(), value()?.to_owned()]);
                continue;
            }
            "-m" | "--model" => ("model", value()?),
            "-s" | "--sandbox" => ("sandbox_mode", value()?),
            "-a" | "--ask-for-approval" => ("approval_policy", value()?),
            "-p" | "--profile" => ("profile", value()?),
            // Anything else needs an explicit app-server equivalent.
            _ => return Err(Error::Line),
        };
        let quoted = serde_json::to_string(value).map_err(|_| Error::Line)?;
        result.extend(["-c".into(), format!("{key}={quoted}")]);
    }
    Ok(result)
}
