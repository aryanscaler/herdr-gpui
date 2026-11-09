//! Moving an agent in a herdr-core pane into a chat. The policy and the move
//! are `sheprd_core_client::convert`'s, on this computer's core; the chat
//! view then attaches through discovery, as it does for a chat started in a
//! core pane.

use super::Error;
use crate::{SheprdWindow, chat::core_pane};
use sheprd_backend::TerminalId;
use sheprd_chat_protocol::Agent;
use sheprd_client::herdr::convert;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};

/// A herdr-core agent on its way into a chat. Dropping it stops the worker
/// between rounds.
#[derive(Debug)]
pub(crate) struct Job {
    cancelled: Arc<AtomicBool>,
    result: mpsc::Receiver<Result<(), Error>>,
}

impl Job {
    /// A move whose worker has not answered yet, and the flag it watches.
    #[cfg(test)]
    pub(crate) fn waiting_for_test() -> (Self, Arc<AtomicBool>) {
        let cancelled = Arc::new(AtomicBool::new(false));
        let (sender, result) = mpsc::sync_channel(1);
        // Kept open, so the worker seems still at work.
        std::mem::forget(sender);
        let job = Self {
            cancelled: cancelled.clone(),
            result,
        };
        (job, cancelled)
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
    }
}

/// The herdr-core terminal a chat key such as `core:t7` names.
pub(crate) fn terminal(key: &str) -> Option<TerminalId> {
    key.strip_prefix("core:")
        .and_then(|terminal| TerminalId::new(terminal).ok())
}

/// The core's own errors, with a refusal of the shared policy kept as one.
fn from_core(error: sheprd_core_client::Error) -> Error {
    match error {
        sheprd_core_client::Error::Convert(error) => Error::Convert(error),
        error => Error::Core(Arc::new(error)),
    }
}

impl SheprdWindow {
    /// Starts moving the agent in herdr-core `terminal` into a chat, and
    /// says which pane, by its chat key, and agent.
    pub(super) fn convert_core_to_chat(
        &mut self,
        terminal: TerminalId,
    ) -> Result<(String, Agent), Error> {
        // `sheprd-acp` and the typed `sh` line are Unix, as for Herdr 1.
        if cfg!(windows) {
            return Err(Error::NotReachable);
        }
        let backend = self
            .core_terminals
            .hub
            .backend()
            .cloned()
            .ok_or(Error::NotConnected)?;
        let info = self
            .core_terminals
            .chat_terminals()
            .find(|info| info.id == terminal)
            .ok_or(convert::Error::NoAgent)?;
        let agent = sheprd_core_client::convert::check(info)?;
        let program = self.chats.program.found()?.to_owned();
        let home = sheprd_client::local_path::home_dir().ok_or(Error::NoHome)?;
        let cancelled = Arc::new(AtomicBool::new(false));
        let flag = cancelled.clone();
        let (sender, result) = mpsc::sync_channel(1);
        let moved = terminal.clone();
        std::thread::Builder::new()
            .name("herdr-core-chat-convert".into())
            .spawn(move || {
                let outcome = sheprd_core_client::convert::run(
                    backend.core(),
                    &moved,
                    agent,
                    &program,
                    &home,
                    &flag,
                )
                // The label the core now carries leads to the chat.
                .map(drop)
                .map_err(from_core);
                let _ = sender.send(outcome);
            })
            .map_err(Error::Worker)?;
        self.chats.converting_core = Some(Job { cancelled, result });
        Ok((core_pane::key(&terminal), agent))
    }

    /// The outcome of a herdr-core move, once its worker is done. Turning
    /// ACP chats off or losing the core stops it: dropping the job tells the
    /// worker, which checks between rounds, before it types the chat's line.
    pub(super) fn poll_core_convert(&mut self) -> Option<Result<(), Error>> {
        let job = self.chats.converting_core.as_ref()?;
        let result = if !self.config.features.acp_chats {
            Err(Error::Disabled)
        } else if self.core_terminals.hub.backend().is_none() {
            Err(convert::Error::Stopped.into())
        } else {
            match job.result.try_recv() {
                Ok(result) => result,
                Err(mpsc::TryRecvError::Empty) => return None,
                Err(mpsc::TryRecvError::Disconnected) => Err(convert::Error::Stopped.into()),
            }
        };
        self.chats.converting_core = None;
        Some(result)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests;
