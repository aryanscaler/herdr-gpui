//! Merge concurrent edits to a kept terminal's pane layout.

use sheprd_backend::TerminalId;
use sheprd_session::panes::{Axis, Panes};

/// `here`, a layout changed since `base`, with the changes `there` made to
/// `base` too: its closed panes go, and its new ones split off `active`
/// while there is room. The new ones a full layout cannot take come back
/// beside it, for the caller to give a tab of their own. `None` when every
/// pane went, each window having closed the other's last.
pub(super) fn merged(
    here: &Panes,
    base: &Panes,
    there: &Panes,
    active: &TerminalId,
) -> (Option<Panes>, Vec<TerminalId>) {
    let mut panes = here.clone();
    let mut overflow = Vec::new();
    let gone: Vec<&TerminalId> = (base.terminals().into_iter())
        .filter(|terminal| !there.contains(terminal))
        .collect();
    for terminal in &gone {
        panes.remove(terminal);
    }
    for new in there.terminals() {
        if !base.contains(new) && !panes.contains(new) {
            let at = if panes.contains(active) {
                active.clone()
            } else {
                panes.first().clone()
            };
            if panes.split(&at, Axis::Row, new.clone()).is_err() {
                overflow.push(new.clone());
            }
        }
    }
    // The last pane cannot go while it is alone; with the other side's new
    // panes in, it can, and if it still cannot, nothing is left.
    for terminal in &gone {
        panes.remove(terminal);
    }
    if gone.iter().any(|terminal| panes.contains(terminal)) {
        return (None, overflow);
    }
    (Some(panes), overflow)
}
