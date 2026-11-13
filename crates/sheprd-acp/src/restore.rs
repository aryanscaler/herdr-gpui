//! The record Sheprd continues this chat from once Herdr restores its pane
//! after a reboot or a server restart (`sheprd_chat_protocol::restore`).
//!
//! It is written when the agent's session opens and removed when this
//! program ends on its own. A chat that is killed, as a reboot kills it,
//! leaves its record behind; that is the point.

use std::path::{Path, PathBuf};

use sheprd_chat_protocol::{Agent, EndpointId, restore};

use crate::{cli::AgentChoice, herdr::Target};

pub(crate) mod core;

/// A detached view or a failed agent is not a request to forget continuity.
pub(crate) fn expected_end(input: &crate::session::Input) -> bool {
    matches!(
        input,
        crate::session::Input::Shutdown | crate::session::Input::AgentExited { code: Some(0), .. }
    )
}

/// One chat's record, waiting for the agent's session id.
#[derive(Clone, Debug)]
pub(crate) struct Keeper {
    adapter: bool,
    agent: Agent,
    args: Vec<String>,
    pub(super) child: Option<u32>,
    core: bool,
    cwd: PathBuf,
    dir: PathBuf,
    endpoint: EndpointId,
    pane: String,
    socket: PathBuf,
}

impl Keeper {
    /// A core terminal has its own identity, even when its daemon inherited
    /// Herdr 1's environment. Keep these records away from the Herdr 1 scanner:
    /// a core address must never be interpreted as a Herdr pane.
    pub(crate) fn core(
        dir: Option<PathBuf>,
        endpoint: Option<&EndpointId>,
        agent: &AgentChoice,
        adapter: bool,
        cwd: &Path,
        var: impl Fn(&str) -> Option<std::ffi::OsString>,
    ) -> Option<Self> {
        let pane = var("HC_TERMINAL_ID")?.into_string().ok()?;
        let socket = PathBuf::from(var("HC_TERMINAL_SOCKET")?);
        // These are the identity variables core injects into the terminal,
        // not HC_SOCKET, which can name the caller's unrelated core.
        if pane.is_empty() || pane.contains('/') || !socket.is_absolute() {
            return None;
        }
        let target = Target { pane, socket };
        let AgentChoice::Builtin { agent, args } = agent else {
            return None;
        };
        let args = args
            .iter()
            .map(|arg| arg.to_str().map(str::to_owned))
            .collect::<Option<Vec<_>>>()?;
        let mut keeper = Self::new(
            dir.map(|dir| dir.join("core")),
            Some(&target),
            endpoint,
            &AgentChoice::Builtin {
                agent: *agent,
                args: Vec::new(),
            },
            adapter,
            cwd,
        )?;
        keeper.args = args;
        keeper.core = true;
        Some(keeper)
    }

    /// A keeper for a chat a client could continue: a built-in agent, in a
    /// Herdr pane, listening for clients. A custom command, a chat outside
    /// Herdr, or one that does not listen keeps nothing.
    pub(crate) fn new(
        dir: Option<PathBuf>,
        target: Option<&Target>,
        endpoint: Option<&EndpointId>,
        agent: &AgentChoice,
        adapter: bool,
        cwd: &Path,
    ) -> Option<Self> {
        let AgentChoice::Builtin { agent, args } = agent else {
            return None;
        };
        // Extra agent arguments are not kept, so a chat started with them
        // could not be continued as it was.
        if !args.is_empty() {
            return None;
        }
        let target = target?;
        Some(Self {
            adapter,
            agent: *agent,
            args: Vec::new(),
            child: None,
            core: false,
            cwd: cwd.to_owned(),
            dir: dir?,
            endpoint: endpoint?.clone(),
            pane: target.pane.clone(),
            socket: target.socket.clone(),
        })
    }

    /// The record of the chat whose agent session is `session`.
    fn record(&self, session: &str) -> restore::Record {
        restore::Record {
            adapter: self.adapter,
            agent: self.agent,
            cwd: self.cwd.clone(),
            endpoint: self.endpoint.clone(),
            pane: self.pane.clone(),
            session: session.to_owned(),
            socket: self.socket.clone(),
        }
    }

    /// Records the session that just opened, replacing an earlier one. A
    /// session id that could not be typed back safely is not kept, and its
    /// earlier record goes with it, so a restore never continues a session
    /// other than the chat's last.
    pub(crate) fn opened(&self, session: &str) {
        if !self.args.is_empty() || self.record(session).write(&self.dir).is_err() {
            restore::remove(&self.dir, &self.endpoint);
        }
        if self.core
            && let Err(error) = core::opened(self, session)
        {
            eprintln!("sheprd-acp: could not persist automatic chat recovery: {error:#}");
        }
    }

    /// Removes the record: the chat ended on its own.
    pub(crate) fn ended(&self) {
        if self.core
            && let Err(error) = core::closed(self)
        {
            eprintln!("sheprd-acp: could not persist the chat closure: {error:#}");
        }
        restore::remove(&self.dir, &self.endpoint);
        core::agent_ended(&self.dir, &self.endpoint);
    }

    /// Keeps the agent this chat started, which a later restore must find
    /// gone. Only a core chat's explicit restore reads it.
    pub(crate) fn agent_started(&self, pid: u32) -> anyhow::Result<()> {
        core::agent_started(&self.dir, &self.endpoint, pid)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests;
