//! Replay core-owned recovery aliases into saved layouts before adoption.
//! Core labels survive an app crash before its asynchronous tabs-file write.

use super::{Host, TabPanes};
use crate::{
    SheprdWindow,
    browser::{Location, Store},
};
use sheprd_backend::TerminalId;
use std::collections::BTreeMap;

impl TabPanes {
    fn remap(&mut self, replacements: &BTreeMap<TerminalId, TerminalId>) -> bool {
        let mut changed = false;
        for (old, new) in replacements {
            if !self.panes.replace(old, new.clone()) {
                continue;
            }
            changed = true;
            if self.active == *old {
                self.active = new.clone();
            }
            if self.previous.as_ref() == Some(old) {
                self.previous = Some(new.clone());
            }
            if self.zoomed.as_ref() == Some(old) {
                self.zoomed = Some(new.clone());
            }
            if let Some(name) = self.names.remove(old) {
                self.names.insert(new.clone(), name);
            }
        }
        changed
    }
}

/// Rebinds one saved tab to its recovered chats. Returns the stored layout
/// to save, if it held a replaced pane, and whether this window's copy
/// changed. The store holds what every window saved; this window's copy may
/// be stale, so it never overwrites the stored layout.
fn recover(
    host: &Host,
    location: &Location,
    shown: Option<&mut TabPanes>,
    mapping: &BTreeMap<TerminalId, TerminalId>,
) -> (Option<Location>, bool) {
    let remapped = |location: &Location| {
        let mut panes = TabPanes::restore(host.clone(), location)?;
        panes.remap(mapping).then(|| panes.location())
    };
    let stored = remapped(location);
    let Some(shown) = shown else {
        return (stored, false);
    };
    let in_sync = shown.saved.as_ref() == Some(location);
    let changed = shown.remap(mapping);
    if in_sync {
        shown.saved = Some(stored.clone().unwrap_or_else(|| location.clone()));
    } else if let Some(base) = shown.saved.as_ref().and_then(remapped) {
        // Rebase the copy last read too, so the next refresh merges another
        // window's changes rather than reading the recovered pane as one
        // this window opened.
        shown.saved = Some(base);
    }
    (stored, changed)
}

impl SheprdWindow {
    pub(super) fn remap_recovered_chats(
        &mut self,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) {
        let focused = self
            .focused_core_chat(window, cx)
            .or_else(|| self.core_terminals.focused_pane(window, cx));
        let mut focus_rebound = None;
        let mappings: Vec<_> = self
            .core_terminals
            .hosts()
            .into_iter()
            .filter_map(|host| {
                let infos = self.core_terminals.infos_of(&host)?;
                let mapping = sheprd_core_client::chat_recovery::mapping(
                    infos.all().map(|info| (info.id.as_str(), &info.labels)),
                );
                (!mapping.is_empty()).then_some((host, mapping))
            })
            .collect();
        let Some(store) = cx.try_global::<Store>() else {
            return;
        };
        let tabs: Vec<_> = store
            .tabs()
            .filter_map(|tab| Some((tab.id, Host::of(&tab.scope), tab.location.clone()?)))
            .collect();
        let mut changed = false;
        for (tab, host, location) in tabs {
            let Some((_, mapping)) = mappings.iter().find(|(mapped, _)| *mapped == host) else {
                continue;
            };
            let (stored, shown_changed) = recover(
                &host,
                &location,
                self.core_terminals.tabs.get_mut(&tab),
                mapping,
            );
            if focused.as_ref().is_some_and(|(focused, terminal)| {
                *focused == tab && mapping.contains_key(terminal)
            }) && shown_changed
            {
                focus_rebound = Some(tab);
            }
            changed |= shown_changed || stored.is_some();
            let Some(location) = stored else {
                continue;
            };
            Store::update(cx, |store| store.visited(tab, Some(location), None));
        }
        if changed {
            self.sync_core_views(cx);
        }
        if let Some(tab) = focus_rebound {
            self.focus_core_terminal(tab, window, cx);
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests;
