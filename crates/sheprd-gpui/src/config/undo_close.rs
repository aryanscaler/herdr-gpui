//! `[undo_close_tab]`: closing a terminal tab waits a few seconds before
//! Herdr closes it, so Reopen Closed Tab can bring back the same tab, with
//! its programs still running, rather than a new shell.
use super::Error;
use crate::Result;
use serde::Deserialize;
use std::time::Duration;

/// How long a closed tab stays recoverable, unless the config says.
const DEFAULT_SECONDS: u64 = 10;
/// Longer, and a tab the user meant to close keeps running for no reason.
const MAX_SECONDS: u64 = 60;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UndoCloseTab {
    /// Off by default: a close then reaches Herdr, and every other client,
    /// at once, as it always has.
    pub enabled: bool,
    /// How long the tab stays open, hidden, before Herdr closes it.
    pub window: Duration,
}

impl Default for UndoCloseTab {
    fn default() -> Self {
        Self {
            enabled: false,
            window: Duration::from_secs(DEFAULT_SECONDS),
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub(super) struct UndoCloseTabSettings {
    enabled: bool,
    seconds: Option<u64>,
}

impl TryFrom<UndoCloseTabSettings> for UndoCloseTab {
    type Error = crate::Error;

    fn try_from(settings: UndoCloseTabSettings) -> Result<Self> {
        let seconds = settings.seconds.unwrap_or(DEFAULT_SECONDS);
        if !(1..=MAX_SECONDS).contains(&seconds) {
            return Err(Error::InvalidUndoCloseSeconds.into());
        }
        Ok(Self {
            enabled: settings.enabled,
            window: Duration::from_secs(seconds),
        })
    }
}
