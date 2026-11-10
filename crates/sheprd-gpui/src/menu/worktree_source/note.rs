//! The note a new checkout gets naming the pull request or issue it was
//! made for, read from the dialog's pending row and written off the UI
//! thread once the checkout exists.

use super::Pending;
use crate::{
    SheprdWindow,
    i18n::fl,
    repo_items::{self, Item},
};
use gpui::prelude::*;

impl SheprdWindow {
    /// Write the note naming what the created checkout is for. The daemon owns
    /// the checkout, so its own reported path is used rather than a guess, and
    /// the write itself is file I/O and never runs on the UI thread.
    pub(in crate::menu) fn write_worktree_note(
        &mut self,
        response: &serde_json::Value,
        cx: &mut Context<Self>,
    ) {
        if let Some(checkout) = response["workspace"]["worktree"]["checkout_path"].as_str() {
            self.write_worktree_note_at(checkout.to_owned(), cx);
            return;
        }
        let item = self.menu.worktree.as_mut().and_then(|source| {
            source
                .lookup
                .origin
                .as_ref()
                .and(source.pending.take())
                .filter(|pending| matches!(pending, Pending::Item(_)))
        });
        if item.is_some() && self.pr_origin().is_some() {
            self.local_error = Some(fl!("worktree-source-no-checkout-path"));
        }
    }

    /// Writes the note naming the pull request or issue the dialog's pending
    /// row is for into the new checkout at `checkout`, off the UI thread.
    pub(in crate::menu) fn write_worktree_note_at(
        &mut self,
        checkout: String,
        cx: &mut Context<Self>,
    ) {
        if let Some(note) = self.take_worktree_note() {
            write_note(note, checkout, cx);
        }
    }

    /// The note the dialog's pending pull request or issue row leaves in its
    /// new checkout, read when the checkout is asked for so it can be
    /// written once the checkout exists, even after the dialog closed.
    pub(in crate::menu) fn worktree_note(&self) -> Option<Note> {
        let host = self.pr_origin()?;
        let source = self.menu.worktree.as_ref()?;
        let (Some(Pending::Item(item)), Some(origin)) = (&source.pending, &source.lookup.origin)
        else {
            return None;
        };
        Some(Note {
            host,
            item: item.clone(),
            origin: origin.clone(),
        })
    }

    fn take_worktree_note(&mut self) -> Option<Note> {
        let note = self.worktree_note();
        if let Some(source) = &mut self.menu.worktree {
            source.pending = None;
        }
        note
    }
}

/// A pull request or issue note for a new checkout: who reads it, the item,
/// and the repository it is from.
#[derive(Debug)]
pub(crate) struct Note {
    host: crate::pull_request::Origin,
    item: Item,
    origin: repo_items::Origin,
}

/// Writes `note` into `checkout` off the UI thread, reporting a failure.
pub(in crate::menu) fn write_note(note: Note, checkout: String, cx: &mut Context<SheprdWindow>) {
    let Note { host, item, origin } = note;
    let write = cx
        .background_executor()
        .spawn(async move { repo_items::write_context(&host, &checkout, &item, &origin) });
    cx.spawn(async move |this, cx| {
        let result = write.await;
        let _ = this.update(cx, |this, cx| {
            if let Err(error) = result {
                this.local_error = Some(error.to_string());
                cx.notify();
            }
        });
    })
    .detach();
}
