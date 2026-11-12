//! A newly spawned terminal belongs to its waiting window only once its
//! result is handed back. A cancelled start must not leave an orphaned chat.

use sheprd_backend::{TerminalId, TerminalInfo};
use std::sync::mpsc::{self, SyncSender};

pub(super) struct Outcome {
    result: Result<TerminalInfo, sheprd_core_client::Error>,
    received: Option<SyncSender<()>>,
}

impl Outcome {
    /// The UI takes responsibility for placement or ending this terminal.
    /// Acknowledging never waits: the one-slot channel is empty until here.
    pub(super) fn take(self) -> Result<TerminalInfo, sheprd_core_client::Error> {
        if let Some(received) = self.received {
            let _ = received.send(());
        }
        self.result
    }

    #[cfg(all(test, unix))]
    pub(super) fn unwatched(result: Result<TerminalInfo, sheprd_core_client::Error>) -> Self {
        Self {
            result,
            received: None,
        }
    }
}

pub(super) fn deliver(
    result: Result<TerminalInfo, sheprd_core_client::Error>,
    sender: SyncSender<Outcome>,
    end: impl FnOnce(&TerminalId),
) -> bool {
    let created = result.as_ref().ok().map(|info| info.id.clone());
    let (received, accepted) = mpsc::sync_channel(1);
    let handed = sender
        .send(Outcome {
            result,
            received: Some(received),
        })
        .is_ok()
        && accepted.recv().is_ok();
    if !handed && let Some(terminal) = created {
        end(&terminal);
    }
    handed
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests;
