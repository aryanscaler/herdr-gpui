//! What a chat's own buttons chose for its code blocks and HTML fences, over
//! the `[chat]` defaults. A choice lasts while the app runs; the defaults
//! decide again after a restart.

use super::{page::PageKey, visual::VisualId};
use crate::config::HtmlRender;
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    sync::Arc,
};

/// How an HTML fence draws this frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HtmlView {
    /// Its source, as a code block.
    Code,
    /// A card that loads the page when asked.
    Card,
    /// A card while its page loads by itself, as `always` asks.
    Load,
}

/// Each chat pane's choices. Blocks are known by what they draw, so two
/// identical blocks in one chat share a choice.
#[derive(Debug, Default)]
pub(crate) struct CodeChoices {
    /// Code blocks whose Wrap button was used, and whether each wraps.
    wrap: HashMap<VisualId, bool>,
    /// HTML fences whose switch was used, and whether each shows its code.
    html_code: HashMap<PageKey, bool>,
    /// Fences `always` already loaded once. It never loads them again, so a
    /// page the user hid, or the oldest one closed for a newer, stays a card.
    loaded: HashSet<PageKey>,
    /// Fences drawing asked to load, for the list to open with its window.
    queued: RefCell<Vec<(PageKey, Arc<str>)>>,
}

impl CodeChoices {
    /// Whether the code block `id` wraps, given the `[chat]` default.
    pub(crate) fn wraps(&self, id: VisualId, default: bool) -> bool {
        self.wrap.get(&id).copied().unwrap_or(default)
    }

    pub(crate) fn set_wrap(&mut self, id: VisualId, wraps: bool) {
        self.wrap.insert(id, wraps);
    }

    /// How the fence at `key` draws without a live page. `embedded` says
    /// whether pages can show in the window; without it `always` would open
    /// a browser window per fence drawn, so it draws the card as `default`.
    pub(crate) fn html_view(&self, key: PageKey, default: HtmlRender, embedded: bool) -> HtmlView {
        match self.html_code.get(&key) {
            Some(true) => HtmlView::Code,
            Some(false) => HtmlView::Card,
            None => match default {
                HtmlRender::Never => HtmlView::Code,
                HtmlRender::Always if embedded && !self.loaded.contains(&key) => HtmlView::Load,
                HtmlRender::Always | HtmlRender::Default => HtmlView::Card,
            },
        }
    }

    /// Whether the user switched the fence at `key` to its code, which a
    /// page still loading for it must not override.
    pub(crate) fn shows_code(&self, key: PageKey) -> bool {
        self.html_code.get(&key) == Some(&true)
    }

    pub(crate) fn set_shows_code(&mut self, key: PageKey, code: bool) {
        self.html_code.insert(key, code);
    }

    /// Asks, from drawing, for the fence at `key` to load once.
    pub(crate) fn queue_load(&self, key: PageKey, page: &Arc<str>) {
        let mut queued = self.queued.borrow_mut();
        if !queued.iter().any(|(queued, _)| *queued == key) {
            queued.push((key, page.clone()));
        }
    }

    /// The fences drawing asked to load, each marked as loaded.
    pub(crate) fn take_queued(&mut self) -> Vec<(PageKey, Arc<str>)> {
        let queued = std::mem::take(self.queued.get_mut());
        self.loaded.extend(queued.iter().map(|(key, _)| *key));
        queued
    }
}

#[cfg(test)]
mod tests;
