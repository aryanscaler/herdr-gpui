//! Approving a script on this host, pinned to its exact text. A person
//! approves either on a terminal, typing the script's name after reading
//! its plan, or in Sheprd's dialog, which passes the hash it showed.
//!
//! This guards against mistakes, not against a hostile agent: an agent with
//! a shell could run commands directly. It refuses where an agent would
//! most easily approve its own script: off a terminal, from a Herdr pane or
//! herdr-core terminal that runs an agent, and `--hash` from inside either.

use crate::{
    Result, fingerprint::Fingerprint, herdr::Herdr, paths::Paths, runner::now, scan, store::Store,
};
use sheprd_automation::{Limits, Plan, Source};
use std::{
    io::{BufRead, IsTerminal, Write},
    path::Path,
};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("not approved: the name typed did not match")]
    Cancelled,
    #[error(
        "the script changed since its plan was shown (expected {expected}, found {found}); review it again"
    )]
    Changed { expected: String, found: String },
    #[error("approve on a terminal, or in Sheprd's dialog; stdin is not a terminal")]
    NotTerminal,
    #[error(
        "this pane runs an agent ({agent}); ask the user to approve in Sheprd or their own terminal"
    )]
    AgentPane { agent: String },
    #[error("--hash is for Sheprd's dialog; inside a Herdr pane, approve interactively instead")]
    HashInPane,
    #[error(
        "could not tell whether Herdr 2 terminal {terminal} runs an agent; ask the user to approve in Sheprd or their own terminal"
    )]
    CoreUnknown {
        terminal: String,
        #[source]
        source: crate::herdr_core::Error,
    },
}

/// The panes a command runs in, either of which an agent may run. A Herdr 2
/// terminal Sheprd starts carries an empty `HERDR_PANE_ID`, while one started
/// from a Herdr pane inherits that pane's, so both are kept and checked.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Panes<'a> {
    /// A Herdr 1 pane, by its `HERDR_PANE_ID`.
    pub herdr: Option<&'a str>,
    /// A herdr-core (Herdr 2) terminal, by its `HC_TERMINAL_ID`.
    pub core: Option<&'a str>,
}

impl<'a> Panes<'a> {
    /// The panes `HERDR_PANE_ID` and `HC_TERMINAL_ID` name; empty is unset.
    pub fn of(herdr: Option<&'a str>, core: Option<&'a str>) -> Self {
        let set = |id: Option<&'a str>| id.filter(|id| !id.is_empty());
        Self {
            herdr: set(herdr),
            core: set(core),
        }
    }

    /// Whether the command runs inside either.
    pub fn any(&self) -> bool {
        self.herdr.is_some() || self.core.is_some()
    }
}

/// How the approval is given.
pub enum Mode<'a> {
    /// Show the plan and ask on this terminal.
    Terminal,
    /// The hash a dialog showed the user, who confirmed it.
    Hash(&'a str),
}

pub struct Approved {
    pub name: String,
    pub hash: String,
}

/// `panes` are the panes the command runs in; `core_agent` names the agent a
/// herdr-core terminal runs, asked only for one. A terminal whose core
/// cannot say is refused, since an agent may well run there.
pub fn approve(
    paths: &Paths,
    store: &Store,
    herdr: &Herdr,
    file: &Path,
    mode: Mode<'_>,
    panes: Panes<'_>,
    core_agent: impl FnOnce(&str) -> crate::herdr_core::Result<Option<String>>,
) -> Result<Approved> {
    let found = scan::locate(paths, store, file)?;
    let source = Source::read(&found.path)?;
    let plan = Plan::build(&source, Limits::default())?;
    let fingerprint = Fingerprint::of(&source, &plan, found.repo.as_deref());
    let by = match mode {
        Mode::Hash(hash) => {
            if panes.any() {
                return Err(Error::HashInPane.into());
            }
            if hash != fingerprint.hash {
                return Err(Error::Changed {
                    expected: hash.to_owned(),
                    found: fingerprint.hash.clone(),
                }
                .into());
            }
            "herdr-ui"
        }
        Mode::Terminal => {
            if !std::io::stdin().is_terminal() {
                return Err(Error::NotTerminal.into());
            }
            if let Some(pane) = panes.herdr
                && let Some(agent) = herdr.pane_agent(pane).ok().and_then(|pane| pane.agent)
            {
                return Err(Error::AgentPane { agent }.into());
            }
            if let Some(terminal) = panes.core {
                let agent = core_agent(terminal).map_err(|source| Error::CoreUnknown {
                    terminal: terminal.to_owned(),
                    source,
                })?;
                if let Some(agent) = agent {
                    return Err(Error::AgentPane { agent }.into());
                }
            }
            let mut out = std::io::stdout().lock();
            let _ = write!(
                out,
                "{plan}{}\nsha256 {}\n\nType the script's name ({}) to approve it on this host: ",
                fingerprint.describe().unwrap_or_default(),
                fingerprint.hash,
                source.name
            );
            let _ = out.flush();
            let mut answer = String::new();
            let _ = std::io::stdin().lock().read_line(&mut answer);
            if answer.trim() != source.name {
                return Err(Error::Cancelled.into());
            }
            "terminal"
        }
    };
    let path = found.path.to_string_lossy().into_owned();
    let now = now();
    store.approve(&path, &fingerprint.hash, by, now)?;
    // Fire nothing for the time before it was approved.
    store.clear_cursors(&path)?;
    store.put_script(&scan::row(
        &source,
        Ok(&plan),
        Some(&fingerprint),
        path.clone(),
        found.repo.as_deref(),
        now,
    ))?;
    Ok(Approved {
        name: source.name,
        hash: fingerprint.hash,
    })
}

#[cfg(test)]
mod tests;
