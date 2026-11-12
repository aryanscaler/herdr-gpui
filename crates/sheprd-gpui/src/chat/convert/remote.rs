//! Conversion executes beside the remote core, where process ownership and
//! exact session records can be verified. Never run the local converter against
//! a remote backend or infer a conversation from the newest session file.

use crate::{SheprdWindow, core_terminals::Host};
use sheprd_backend::TerminalId;
use sheprd_chat_protocol::Agent;
use sheprd_client::{ConnectTarget, ScriptLimits, shell_quote};
use std::{
    sync::{Arc, atomic::AtomicBool, mpsc},
    time::Duration,
};

mod error;
pub use error::Error;

#[derive(Clone, Debug)]
pub(super) struct Owner {
    core: String,
    host: Host,
    target: ConnectTarget,
}

fn script(core: &str, terminal: &TerminalId, agent: Agent) -> String {
    let program = crate::chat::remote_program_script(&format!(
        "session-convert {} {agent}",
        shell_quote(terminal.as_str()),
    ));
    format!(
        // Keep the owning host's login-selected socket/session, exactly as
        // its core bridge does. Only terminal attribution is inherited noise.
        "unset HC_TERMINAL_ID HC_TERMINAL_SOCKET; export SHEPRD_EXPECT_CORE={}; {program}",
        shell_quote(core),
    )
}

fn run(owner: &Owner, terminal: &TerminalId, agent: Agent, stop: &AtomicBool) -> Result<(), Error> {
    crate::chat::remote_route::with_command(
        &owner.target,
        &script(&owner.core, terminal, agent),
        stop,
        |command| {
            sheprd_client::run_script_command(
                command,
                std::io::empty(),
                std::io::sink(),
                ScriptLimits {
                    idle: Duration::from_secs(30),
                    output: 16 * 1024,
                },
                stop,
            )
            .map(drop)
        },
    )??;
    Ok(())
}

impl SheprdWindow {
    pub(super) fn remote_core_conversion_current(&self, owner: &Owner) -> bool {
        let Host::Remote(scope) = &owner.host else {
            return false;
        };
        self.saved_target(scope) == Some(&owner.target)
            && self
                .core_terminals
                .host_backend(&owner.host)
                .is_some_and(|backend| backend.core().hello().core.id == owner.core)
    }

    pub(super) fn convert_remote_core_to_chat(
        &mut self,
        key: &str,
    ) -> Result<(String, Agent), super::Error> {
        let (host, info) = self
            .core_terminals
            .routed_chat_terminals()
            .find(|(host, info)| crate::chat::core_pane::key_on(host, &info.id) == key)
            .ok_or(super::Error::NotReachable)?;
        let Host::Remote(scope) = host else {
            return Err(super::Error::NotReachable);
        };
        if info.id.as_str().contains('/') {
            return Err(super::Error::NotReachable);
        }
        let target = self
            .saved_target(scope)
            .filter(|target| target.is_remote())
            .ok_or(super::Error::NotReachable)?
            .clone();
        if self.core_terminals.remote_target(scope) != Some(&target) {
            return Err(super::Error::Stale);
        }
        let core = self
            .core_terminals
            .host_backend(host)
            .ok_or(super::Error::NotConnected)?
            .core()
            .hello()
            .core
            .id
            .clone();
        let agent = sheprd_core_client::convert::check(info)?;
        let terminal = info.id.clone();
        let owner = Owner {
            core,
            host: host.clone(),
            target,
        };
        let worker_owner = owner.clone();
        let cancelled = Arc::new(AtomicBool::new(false));
        let flag = cancelled.clone();
        let (sender, result) = mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("sheprd-remote-chat-convert".into())
            .spawn(move || {
                let _ = sender
                    .send(run(&worker_owner, &terminal, agent, &flag).map_err(super::Error::from));
            })
            .map_err(super::Error::Worker)?;
        self.chats.converting_core = Some(super::core::Job {
            cancelled,
            result,
            remote: Some(owner),
        });
        Ok((key.to_owned(), agent))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests;
