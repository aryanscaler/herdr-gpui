//! What only the worker reads: the desktop's document and the saved tabs,
//! kept between polls so an unchanged one is not parsed or read again.

use super::{Result, projection, spaces};
use crate::hub::Layout;
use sheprd_core_client::{
    Error,
    wire::{Doc, Terminal},
};
use sheprd_session::SavedTabs;
use std::{path::Path, time::SystemTime};

#[derive(Default)]
pub(super) struct Poller {
    /// The core `doc` came from: versions count per core, so a wiped core
    /// can reach the old number with another document.
    boot: String,
    /// The document's version and its spaces, `None` when it has none.
    doc: Option<(u64, Option<spaces::Spaces>)>,
    checkouts: super::checkouts::Checkouts,
    names: spaces::Names,
    saved: Option<(SystemTime, Option<SavedTabs>)>,
}

impl Poller {
    /// Takes the answer to reading the desktop's document. Only a core
    /// that says there is none clears the spaces: a slow answer says
    /// nothing about the document, and dropping them would make every
    /// space vanish from phones until the next poll. Another core's spaces
    /// are never kept: their addresses mean nothing here.
    pub(super) fn read_spaces(
        &mut self,
        boot: &str,
        answer: sheprd_core_client::Result<Doc>,
    ) -> Result<()> {
        if self.boot != boot {
            boot.clone_into(&mut self.boot);
            self.doc = None;
        }
        match answer {
            Ok(doc) => {
                if self.doc.as_ref().is_none_or(|(at, _)| *at != doc.version) {
                    self.doc = Some((doc.version, spaces::Spaces::read(doc.value)));
                }
            }
            Err(Error::Refused { .. }) => self.doc = None,
            Err(Error::Lost) => return Err(Error::Lost.into()),
            Err(error) => tracing::debug!(%error, "kept the desktop's spaces"),
        }
        Ok(())
    }

    pub(super) fn project(
        &mut self,
        boot: &str,
        terminals: &[Terminal],
        tabs: Option<&Path>,
        scope: Option<&str>,
    ) -> Layout {
        if let Some(tabs) = tabs {
            self.refresh_saved(tabs);
        }
        let saved = self.saved.as_ref().and_then(|(_, saved)| saved.as_ref());
        let mut layout = match &self.doc {
            Some((_, Some(spaces))) => {
                let mut layout = spaces.project(boot, terminals, saved, scope, &mut self.names);
                projection::lead_kept(&mut layout, saved, scope);
                layout
            }
            _ => projection::project(boot, terminals, saved, scope),
        };
        self.checkouts.fill(&mut layout, std::time::Instant::now());
        layout
    }

    /// Whether Sheprd keeps workspace `id` for `scope` in its saved tabs.
    pub(super) fn keeps(&self, scope: &str, id: &str) -> bool {
        self.saved
            .as_ref()
            .and_then(|(_, saved)| saved.as_ref())
            .is_some_and(|saved| saved.workspaces(scope).any(|workspace| workspace.id == id))
    }

    /// Every terminal `scope`'s saved tabs hold, watching ones included.
    pub(super) fn held(&self, scope: Option<&str>) -> std::collections::HashSet<String> {
        let saved = self.saved.as_ref().and_then(|(_, saved)| saved.as_ref());
        saved
            .zip(scope)
            .into_iter()
            .flat_map(|(saved, scope)| saved.core_tabs(scope))
            .flat_map(|tab| tab.panes.terminals())
            .map(|terminal| terminal.as_str().to_owned())
            .collect()
    }

    /// The workspace of `scope`'s saved tab `tab`.
    pub(super) fn saved_workspace(&self, scope: &str, tab: u64) -> Option<String> {
        self.saved
            .as_ref()
            .and_then(|(_, saved)| saved.as_ref())?
            .core_tabs(scope)
            .find(|saved| saved.id == tab)
            .map(|saved| saved.workspace_id.to_owned())
    }

    fn refresh_saved(&mut self, path: &Path) {
        let changed = std::fs::metadata(path).and_then(|file| file.modified());
        match (changed, &self.saved) {
            (Ok(at), Some((read, _))) if at == *read => {}
            (Ok(at), _) => self.saved = Some((at, projection::read(path))),
            (Err(_), _) => self.saved = None,
        }
    }
}
