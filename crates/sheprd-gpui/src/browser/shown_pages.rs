//! The native pages of the browser tabs the focused workspace's groups
//! show. Showing a tab by choice makes its page at once; a group can also
//! come to show a tab without that, as when a saved layout is restored at
//! startup, a group moves to a neighbour of a closed tab, or a group that
//! lost its tab takes another. Each tick gives those tabs their pages too,
//! so none waits for a click to load.
use super::{Location, Tab, groups::Shown, view::store};
use crate::SheprdWindow;
use gpui::{Context, Window};

impl SheprdWindow {
    /// Creates `tab`'s page if it has none, or records why it cannot be
    /// made. Creating one starts the platform's web content processes, so
    /// this runs from input and ticks, never from render.
    pub(super) fn create_page(&mut self, tab: &Tab, window: &mut Window, cx: &mut Context<Self>) {
        match self.browser.pages.ensure(tab, window, cx) {
            Ok(()) => {
                self.browser.failed.remove(&tab.id);
            }
            Err(error) => {
                tracing::warn!(%error, "Cannot create a browser page");
                self.browser
                    .failed
                    .insert(tab.id, crate::i18n::Localize::localize(&error).into());
            }
        }
    }

    /// Runs on every window tick: gives each page a group shows its native
    /// page. A page that could not be made keeps its failure until the user
    /// shows its tab again, rather than being retried every tick. Editor
    /// tabs are left to their providers, which wait for their servers.
    pub(super) fn ensure_shown_pages(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(store) = store(cx) else {
            return;
        };
        let pages = &self.browser.pages;
        let wanted: Vec<Tab> = self
            .group_slots()
            .into_iter()
            .filter_map(|slot| match self.group_shown(slot.id, cx) {
                Shown::Page(id) => Some(id),
                _ => None,
            })
            .filter(|id| {
                !pages.contains(*id)
                    && !pages.is_building(*id)
                    && !self.browser.failed.contains_key(id)
            })
            .filter_map(|id| store.get(id))
            .filter(|tab| !tab.place.is_editor())
            .filter(|tab| tab.location.as_ref().is_some_and(Location::is_page))
            .cloned()
            .collect();
        for tab in &wanted {
            self.create_page(tab, window, cx);
        }
        if !wanted.is_empty() {
            cx.notify();
        }
    }
}
