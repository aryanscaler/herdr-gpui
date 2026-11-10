//! Mutes of what the app keeps itself for Herdr 2: its workspaces and the
//! tabs of herdr-core terminals. No daemon snapshot names these, so they are
//! keyed by the app's own IDs, which it never hands out twice, under the
//! browser scope they belong to, and forgotten once the app no longer keeps
//! their target.

use super::{MAX_FIELD_BYTES, Mutes, Scope, seconds};
use serde::{Deserialize, Serialize};
use std::time::SystemTime;

/// Kept mutes saved; the least recently changed go first beyond this.
pub(super) const MAX_KEPT: usize = 512;

/// A muted workspace or tab the app keeps.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct KeptMute {
    /// The browser scope (`browser::Scope`) the workspace or tab is kept in.
    scope: String,
    kind: Scope,
    id: String,
    /// When it was muted, in seconds since 1970.
    muted_at: u64,
}

impl KeptMute {
    pub(super) fn valid(&self) -> bool {
        [&self.scope, &self.id].iter().all(|field| {
            !field.is_empty()
                && field.len() <= MAX_FIELD_BYTES
                && !field.chars().any(char::is_control)
        })
    }

    fn is(&self, scope: &str, kind: Scope, id: &str) -> bool {
        self.scope == scope && self.kind == kind && self.id == id
    }
}

impl Mutes {
    /// Whether the app's workspace or tab `id` in `scope` is muted on its own.
    pub(crate) fn kept(&self, scope: &str, kind: Scope, id: &str) -> bool {
        self.kept.iter().any(|mute| mute.is(scope, kind, id))
    }

    /// Mutes or unmutes the app's workspace or tab `id` in `scope`, at
    /// `now`, and forgets the mutes of what `exists` says the app no longer
    /// keeps. Returns whether anything changed; an ID too long to keep is
    /// refused.
    pub(crate) fn set_kept(
        &mut self,
        scope: &str,
        kind: Scope,
        id: &str,
        muted: bool,
        now: SystemTime,
        exists: impl Fn(&str, Scope, &str) -> bool,
    ) -> bool {
        let before = self.kept.len();
        self.kept
            .retain(|mute| exists(&mute.scope, mute.kind, &mute.id));
        let mut changed = self.kept.len() != before;
        if muted != self.kept(scope, kind, id) {
            if muted {
                let mute = KeptMute {
                    scope: scope.to_owned(),
                    kind,
                    id: id.to_owned(),
                    muted_at: seconds(now),
                };
                if mute.valid() {
                    self.kept.push(mute);
                    if self.kept.len() > MAX_KEPT {
                        let excess = self.kept.len() - MAX_KEPT;
                        self.kept.drain(..excess);
                    }
                    changed = true;
                }
            } else {
                self.kept.retain(|mute| !mute.is(scope, kind, id));
                changed = true;
            }
        }
        if changed {
            self.changed();
        }
        changed
    }

    /// Forgets the mutes of tabs `open` says the app no longer holds. Called
    /// at launch, before a tab opens: the app numbers its tabs on from the
    /// highest one it restored, so a closed tab's ID can come back on a new
    /// tab after a restart. Returns whether anything changed.
    pub(crate) fn forget_closed_tabs(&mut self, open: impl Fn(&str, &str) -> bool) -> bool {
        let before = self.kept.len();
        self.kept
            .retain(|mute| mute.kind != Scope::Tab || open(&mute.scope, &mute.id));
        let changed = self.kept.len() != before;
        if changed {
            self.changed();
        }
        changed
    }

    /// Forgets the mutes of the app's workspace `workspace` in `scope` and
    /// of `tabs`, which closed with it. Returns whether anything changed.
    pub(crate) fn forget_kept(&mut self, scope: &str, workspace: &str, tabs: &[String]) -> bool {
        let before = self.kept.len();
        self.kept.retain(|mute| {
            mute.scope != scope
                || match mute.kind {
                    Scope::Workspace => mute.id != workspace,
                    Scope::Tab => !tabs.contains(&mute.id),
                }
        });
        let changed = self.kept.len() != before;
        if changed {
            self.changed();
        }
        changed
    }
}
