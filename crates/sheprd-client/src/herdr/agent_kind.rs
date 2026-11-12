//! Agents Herdr reports sessions for: their names, the session references
//! Herdr's integrations report for them, and the command lines that start
//! or resume them.

mod identify;

use super::snapshot::{AgentSession, SessionKind};

/// Agents Herdr's integrations report resumable sessions for. Resume commands
/// follow Herdr's own (`agent_resume::plan`); whether Teleport can also bring
/// the session to another host is [`AgentKind::carries`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentKind {
    Claude,
    Codex,
    Opencode,
    Pi,
    Omp,
    Copilot,
    Devin,
    Droid,
    Kimi,
    Mastracode,
    Hermes,
    Qodercli,
    Qwen,
    Kilo,
    Cursor,
    Antigravity,
    Grok,
    Letta,
}

/// The longest session id Herdr records.
const MAX_SESSION_ID: usize = 512;
/// The longest session file path Herdr records.
const MAX_SESSION_PATH: usize = 4096;

/// How many arguments follow a flag on an agent's command line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Arity {
    /// A switch.
    Zero,
    /// Exactly one value, unless given inline with `=`.
    One,
    /// One value when the next argument is not a flag.
    Optional,
    /// Every following argument up to the next flag.
    Many,
}

impl AgentKind {
    /// Every kind, in declaration order.
    pub const ALL: [Self; 18] = [
        Self::Claude,
        Self::Codex,
        Self::Opencode,
        Self::Pi,
        Self::Omp,
        Self::Copilot,
        Self::Devin,
        Self::Droid,
        Self::Kimi,
        Self::Mastracode,
        Self::Hermes,
        Self::Qodercli,
        Self::Qwen,
        Self::Kilo,
        Self::Cursor,
        Self::Antigravity,
        Self::Grok,
        Self::Letta,
    ];

    /// The kind for the agent name Herdr reports.
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "claude" => Self::Claude,
            "codex" => Self::Codex,
            "opencode" => Self::Opencode,
            "pi" => Self::Pi,
            "omp" => Self::Omp,
            "copilot" => Self::Copilot,
            "devin" => Self::Devin,
            "droid" => Self::Droid,
            "kimi" => Self::Kimi,
            "mastracode" => Self::Mastracode,
            "hermes" => Self::Hermes,
            "qodercli" => Self::Qodercli,
            "qwen" => Self::Qwen,
            "kilo" => Self::Kilo,
            "cursor" => Self::Cursor,
            "agy" => Self::Antigravity,
            "grok" => Self::Grok,
            "letta" => Self::Letta,
            _ => return None,
        })
    }

    /// The agent name Herdr reports.
    pub fn name(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Opencode => "opencode",
            Self::Pi => "pi",
            Self::Omp => "omp",
            Self::Copilot => "copilot",
            Self::Devin => "devin",
            Self::Droid => "droid",
            Self::Kimi => "kimi",
            Self::Mastracode => "mastracode",
            Self::Hermes => "hermes",
            Self::Qodercli => "qodercli",
            Self::Qwen => "qwen",
            Self::Kilo => "kilo",
            Self::Cursor => "cursor",
            Self::Antigravity => "agy",
            Self::Grok => "grok",
            Self::Letta => "letta",
        }
    }

    /// The name of the Herdr integration that reports this agent's sessions.
    fn integration(self) -> &'static str {
        match self {
            Self::Antigravity => "antigravity_cli",
            _ => self.name(),
        }
    }

    /// The program that runs this agent. Teleport hosts are POSIX, so
    /// Cursor's is never Windows' `cursor-agent.cmd`.
    pub fn binary(self) -> &'static str {
        match self {
            Self::Cursor => "cursor-agent",
            _ => self.name(),
        }
    }

    /// Whether Teleport can bring this agent's session to another host: it
    /// knows where the agent keeps it, or the agent keeps it on a server.
    /// The rest resume through Herdr's own restore on the host that stores
    /// the session; where that storage lives is unconfirmed, so Teleport
    /// starts them afresh instead.
    pub fn carries(self) -> bool {
        matches!(
            self,
            Self::Claude
                | Self::Codex
                | Self::Opencode
                | Self::Pi
                | Self::Omp
                | Self::Copilot
                | Self::Letta
        )
    }

    /// Whether a reported session reference identifies a session this kind
    /// can resume. References come from Herdr's integrations or the verified
    /// Sheprd session-binding reader, with the reference kinds restore accepts.
    pub fn accepts(self, session: &AgentSession) -> bool {
        (session.source.strip_prefix("herdr:") == Some(self.integration())
            || session.source == super::sessions::SOURCE)
            && session.agent == self.name()
            && match (self, session.kind) {
                (Self::Pi | Self::Omp, SessionKind::Path) => valid_path(&session.value),
                (_, SessionKind::Id) => self.valid_id(&session.value),
                (_, SessionKind::Path) => false,
            }
    }

    /// Ids are passed as one argument, never through a shell unquoted; they
    /// must still not read as a flag or carry control characters.
    fn valid_id(self, id: &str) -> bool {
        let plain = !id.is_empty()
            && id.len() <= MAX_SESSION_ID
            && !id.starts_with('-')
            && !id.chars().any(char::is_control);
        // Letta's default conversation is named by its agent.
        plain && (self != Self::Letta || id.strip_prefix("default:") != Some(""))
    }

    /// How many values follow `flag` on the original command line. Only
    /// agents whose flags are known keep any; the rest resume with none,
    /// as Herdr's restore does.
    fn arity(self, flag: &str) -> Option<Arity> {
        let one: &[&str] = match self {
            Self::Claude => &[
                "--model",
                "--permission-mode",
                "--add-dir",
                "--allowedTools",
                "--allowed-tools",
                "--disallowedTools",
                "--disallowed-tools",
                "--mcp-config",
                "--settings",
                "--append-system-prompt",
                "--system-prompt",
                "--agent",
                "--effort",
                "--fallback-model",
            ],
            Self::Codex => &[
                "--enable",
                "--disable",
                "-m",
                "--model",
                "-c",
                "--config",
                "-s",
                "--sandbox",
                "-a",
                "--ask-for-approval",
                "-p",
                "--profile",
                "--add-dir",
            ],
            Self::Opencode => &["-m", "--model", "--agent"],
            Self::Pi | Self::Omp => &["--model", "--provider", "--thinking"],
            // From `copilot --help` (1.0.8).
            Self::Copilot => &[
                "--add-dir",
                "--add-github-mcp-tool",
                "--add-github-mcp-toolset",
                "--additional-mcp-config",
                "--agent",
                "--disable-mcp-server",
                "--log-level",
                "--max-autopilot-continues",
                "--model",
                "--plugin-dir",
                "--reasoning-effort",
                "--stream",
            ],
            _ => &[],
        };
        let (optional, many): (&[&str], &[&str]) = match self {
            Self::Copilot => (
                &["--alt-screen", "--bash-env", "--mouse"],
                &[
                    "--allow-tool",
                    "--allow-url",
                    "--available-tools",
                    "--deny-tool",
                    "--deny-url",
                    "--excluded-tools",
                    "--secret-env-vars",
                ],
            ),
            _ => (&[], &[]),
        };
        if one.contains(&flag) {
            Some(Arity::One)
        } else if optional.contains(&flag) {
            Some(Arity::Optional)
        } else if many.contains(&flag) {
            Some(Arity::Many)
        } else if self.keeps_flags() {
            Some(Arity::Zero)
        } else {
            None
        }
    }

    fn keeps_flags(self) -> bool {
        matches!(
            self,
            Self::Claude | Self::Codex | Self::Opencode | Self::Pi | Self::Omp | Self::Copilot
        )
    }

    /// Flags naming a session, a working directory, or a one-shot mode, with
    /// the values they take; the resume command supplies its own.
    fn replaced(self, flag: &str) -> Option<Arity> {
        let (one, zero, optional): (&[&str], &[&str], &[&str]) = match self {
            Self::Claude => (
                &["--resume", "-r", "--session-id", "--fork-session"],
                &["--continue", "-c", "--print", "-p"],
                &[],
            ),
            Self::Codex => (&["-C", "--cd"], &[], &[]),
            Self::Opencode => (
                &["-s", "--session", "--port", "--hostname"],
                &["-c", "--continue", "--fork"],
                &[],
            ),
            Self::Pi | Self::Omp => (
                &["--session", "--resume", "-r", "--fork", "--session-dir"],
                &["-c", "--continue", "--no-session"],
                &[],
            ),
            Self::Copilot => (
                &[
                    "-i",
                    "--interactive",
                    "-p",
                    "--prompt",
                    "--config-dir",
                    "--log-dir",
                    "--output-format",
                ],
                &["--continue", "--acp", "-s", "--silent", "--share-gist"],
                &["--resume", "--share"],
            ),
            _ => (&[], &[], &[]),
        };
        if one.contains(&flag) {
            Some(Arity::One)
        } else if zero.contains(&flag) {
            Some(Arity::Zero)
        } else if optional.contains(&flag) {
            Some(Arity::Optional)
        } else {
            None
        }
    }

    /// The flags of an agent's original command line worth keeping: model
    /// and permission choices. Positionals are dropped, because they are an
    /// initial prompt or project directory that must not be replayed.
    pub fn kept_flags(self, argv: &[String]) -> Vec<String> {
        let mut kept = Vec::new();
        let mut args = argv.iter().skip(1).peekable();
        while let Some(arg) = args.next() {
            if arg == "--" {
                break;
            }
            if !arg.starts_with('-') {
                continue;
            }
            let (name, inline) = arg
                .split_once('=')
                .map_or((arg.as_str(), false), |(name, _)| (name, true));
            let (keep, arity) = match self.replaced(name) {
                Some(arity) => (false, arity),
                None => match self.arity(name) {
                    Some(arity) => (true, arity),
                    None => continue,
                },
            };
            let mut values = Vec::new();
            if !inline {
                match arity {
                    Arity::Zero => {}
                    Arity::One => match args.next() {
                        Some(value) => values.push(value),
                        // A flag missing its value is not worth replaying.
                        None => continue,
                    },
                    Arity::Optional => values.extend(args.next_if(|next| !next.starts_with('-'))),
                    Arity::Many => {
                        while let Some(value) = args.next_if(|next| !next.starts_with('-')) {
                            values.push(value);
                        }
                    }
                }
            }
            if keep {
                kept.push(arg.clone());
                kept.extend(values.into_iter().cloned());
            }
        }
        kept
    }

    /// The command that reopens `reference` (an id, or the session file's
    /// destination path), with `flags` from the original command line.
    pub fn resume_argv(self, reference: &str, flags: &[String]) -> Vec<String> {
        let mut argv = vec![self.binary().to_owned()];
        let mut flag = |flag: &str| argv.extend([flag.to_owned(), reference.to_owned()]);
        match self {
            Self::Claude
            | Self::Devin
            | Self::Droid
            | Self::Hermes
            | Self::Qodercli
            | Self::Qwen
            | Self::Cursor
            | Self::Grok => flag("--resume"),
            Self::Opencode | Self::Pi | Self::Kimi | Self::Kilo => flag("--session"),
            Self::Mastracode => flag("--thread"),
            Self::Antigravity => flag("--conversation"),
            Self::Codex => argv.extend(["resume".to_owned(), reference.to_owned()]),
            // Their resume flags take an optional value, so it must be inline.
            Self::Copilot | Self::Omp => argv.push(format!("--resume={reference}")),
            Self::Letta => match reference.strip_prefix("default:") {
                Some(agent) => {
                    argv.extend(["--conversation", "default", "--agent", agent].map(str::to_owned))
                }
                None => flag("--conversation"),
            },
        }
        argv.extend(flags.iter().cloned());
        argv
    }
}

fn valid_path(path: &str) -> bool {
    let drive = path.as_bytes().get(..3).is_some_and(|prefix| {
        prefix[0].is_ascii_alphabetic() && prefix[1] == b':' && matches!(prefix[2], b'/' | b'\\')
    });
    (path.starts_with('/') || drive)
        && path.len() <= MAX_SESSION_PATH
        && !path.chars().any(char::is_control)
}
impl AgentKind {
    /// Whether [`AgentKind::start_argv`] knows how to give this agent a
    /// first prompt, so it can continue from a handoff note.
    pub fn takes_prompt(self) -> bool {
        matches!(
            self,
            Self::Claude | Self::Codex | Self::Opencode | Self::Pi | Self::Omp | Self::Copilot
        )
    }

    /// The command that starts this agent afresh, optionally with a first
    /// prompt when it [takes one](AgentKind::takes_prompt).
    pub fn start_argv(self, prompt: Option<&str>) -> Vec<String> {
        let mut argv = vec![self.binary().to_owned()];
        if let Some(prompt) = prompt.filter(|_| self.takes_prompt()) {
            match self {
                Self::Opencode => argv.push("--prompt".to_owned()),
                // Copilot's `-p` runs the prompt and exits.
                Self::Copilot => argv.push("-i".to_owned()),
                _ => {}
            }
            argv.push(prompt.to_owned());
        }
        argv
    }

    /// Agents a handoff may fall back to, most capable first.
    pub const FALLBACKS: [Self; 4] = [Self::Claude, Self::Codex, Self::Opencode, Self::Pi];
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests;
