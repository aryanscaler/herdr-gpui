//! What each pane runs, and how to run it again on the destination.

use super::snapshot::{AgentSession, Pane, ProcessInfo};
pub use sheprd_client::herdr::AgentKind;

/// What a source pane is doing, and so what its destination pane will do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Work {
    /// An idle shell prompt; the new pane's shell is enough.
    Shell,
    /// A supported agent with a resumable session.
    Session {
        agent: AgentKind,
        session: AgentSession,
        flags: Vec<String>,
    },
    /// A detected agent without a session Teleport can move. `argv` starts
    /// the same agent afresh; a handoff note carries its context.
    Agent { name: String, argv: Vec<String> },
    /// Any other foreground program, run again with the same arguments.
    Command(Vec<String>),
}

impl Work {
    pub fn classify(pane: &Pane, process: &ProcessInfo) -> Self {
        let job = process.foreground_job();
        let argv = job.map(|job| job.argv.clone()).unwrap_or_default();
        if let Some(session) = &pane.agent_session
            && let Some(agent) = AgentKind::parse(&session.agent)
            && pane.agent.as_deref().and_then(AgentKind::parse) == Some(agent)
            && agent.carries()
            && agent.accepts(session)
        {
            return Self::Session {
                agent,
                session: session.clone(),
                flags: agent.kept_flags(&argv),
            };
        }
        if let Some(name) = pane.agent.as_ref().filter(|name| !name.is_empty()) {
            let argv = if argv.is_empty() {
                vec![name.clone()]
            } else {
                argv
            };
            return Self::Agent {
                name: name.clone(),
                argv,
            };
        }
        if argv.is_empty() {
            Self::Shell
        } else {
            Self::Command(argv)
        }
    }

    /// The program the destination must have, if any.
    pub fn program(&self) -> Option<&str> {
        match self {
            Self::Shell => None,
            Self::Session { agent, .. } => Some(agent.binary()),
            Self::Agent { argv, .. } | Self::Command(argv) => argv.first().map(|p| basename(p)),
        }
    }
}

fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// `path` moved from under `from` to under `to`, if it lies under `from`.
pub fn remap(path: &str, from: &str, to: &str) -> Option<String> {
    let from = from.trim_end_matches('/');
    let rest = path.strip_prefix(from)?;
    (rest.is_empty() || rest.starts_with('/'))
        .then(|| format!("{}{rest}", to.trim_end_matches('/')))
}

/// Replace every occurrence of the checkout path `from` in `text`, where the
/// match ends at a path boundary (so `/w/feat` never rewrites `/w/feature`).
pub fn rewrite_paths(text: &[u8], from: &str, to: &str) -> Vec<u8> {
    let from = from.trim_end_matches('/').as_bytes();
    let to = to.trim_end_matches('/').as_bytes();
    if from.is_empty() || from == to {
        return text.to_vec();
    }
    let mut out = Vec::with_capacity(text.len());
    let mut index = 0;
    while index < text.len() {
        if text[index..].starts_with(from) {
            let next = text.get(index + from.len()).copied();
            let boundary = !next.is_some_and(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b));
            if boundary {
                out.extend_from_slice(to);
                index += from.len();
                continue;
            }
        }
        out.push(text[index]);
        index += 1;
    }
    out
}

/// A command line re-targeted at the destination: paths under the source
/// checkout move with it, and an absolute program outside the checkout is
/// found on the destination's `PATH` instead.
pub fn remap_argv(argv: &[String], from: &str, to: &str) -> Vec<String> {
    argv.iter()
        .enumerate()
        .map(|(index, arg)| {
            let moved =
                String::from_utf8_lossy(&rewrite_paths(arg.as_bytes(), from, to)).into_owned();
            if index == 0 && moved.starts_with('/') && remap(arg, from, to).is_none() {
                basename(&moved).to_owned()
            } else {
                moved
            }
        })
        .collect()
}

/// Resume a transferred reference with checkout-relative flag paths moved.
pub(super) fn resume_command(
    agent: AgentKind,
    reference: &str,
    flags: &[String],
    from: &str,
    to: &str,
) -> Vec<String> {
    resume_command_with(agent, reference, flags, |path| remap(path, from, to))
}

pub(super) fn resume_command_with(
    agent: AgentKind,
    reference: &str,
    flags: &[String],
    mapped: impl Fn(&str) -> Option<String>,
) -> Vec<String> {
    // Only the retained flags are source paths. A path-valued session
    // reference already names the destination and must not be remapped twice.
    let flags = flags
        .iter()
        .map(|flag| {
            if let Some((name, value)) = flag.split_once('=')
                && name.starts_with('-')
            {
                return mapped(value)
                    .map(|value| format!("{name}={value}"))
                    .unwrap_or_else(|| flag.clone());
            }
            mapped(flag).unwrap_or_else(|| flag.clone())
        })
        .collect::<Vec<_>>();
    agent.resume_argv(reference, &flags)
}

/// One POSIX shell command line for `argv`.
pub fn command_line(argv: &[String]) -> String {
    argv.iter()
        .map(|arg| {
            if !arg.is_empty()
                && arg
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_./=:,+@%".contains(&b))
            {
                arg.clone()
            } else {
                sheprd_client::shell_quote(arg)
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Claude Code's project directory name for `cwd`: every character that is
/// not an ASCII letter or digit becomes `-`, and names over 200 characters are
/// cut and suffixed with a base-36 hash of the full path.
pub fn claude_project_dir(cwd: &str) -> String {
    const LIMIT: usize = 200;
    let name: String = cwd
        .encode_utf16()
        .map(|unit| match char::from_u32(u32::from(unit)) {
            Some(c) if c.is_ascii_alphanumeric() => c,
            _ => '-',
        })
        .collect();
    if name.len() <= LIMIT {
        return name;
    }
    // Java-style string hash over UTF-16 units, as JavaScript computes it.
    let hash = cwd.encode_utf16().fold(0i32, |hash, unit| {
        hash.wrapping_shl(5)
            .wrapping_sub(hash)
            .wrapping_add(i32::from(unit))
    });
    format!(
        "{}-{}",
        &name[..LIMIT],
        base36(i64::from(hash).unsigned_abs())
    )
}

fn base36(mut value: u64) -> String {
    const DIGITS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut out = Vec::new();
    loop {
        out.push(DIGITS[(value % 36) as usize]);
        value /= 36;
        if value == 0 {
            break;
        }
    }
    out.reverse();
    String::from_utf8_lossy(&out).into_owned()
}

/// pi's session directory name for `cwd`.
pub fn pi_session_dir(cwd: &str) -> String {
    let trimmed = cwd.strip_prefix(['/', '\\']).unwrap_or(cwd);
    format!("--{}--", trimmed.replace(['/', '\\', ':'], "-"))
}

/// Where the agent in the `index`th handed-off pane writes its note, relative
/// to the checkout. The note is an untracked file, so it travels with the
/// uncommitted changes.
pub fn handoff_path(index: usize) -> String {
    format!(".herdr/teleport/handoff-{}.md", index + 1)
}

/// The prompt that asks an agent to leave its context behind in the checkout.
pub fn handoff_prompt(path: &str) -> String {
    format!(
        "This work is moving to another machine. Write a handoff note to {path} \
         (create the directory) so another agent can continue seamlessly: the goal, what is \
         done, the current state including uncommitted changes, decisions and constraints, \
         open questions, and the exact next steps. Do not change anything else, then stop."
    )
}

pub fn handoff_resume_prompt(path: &str) -> String {
    format!("Read {path} and continue the work it describes.")
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests;
