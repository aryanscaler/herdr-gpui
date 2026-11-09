//! What a herdr-core terminal asks of the window beyond its cells: bells to
//! ring and text to put on the clipboard (OSC 52). Herdr 1 forwards these
//! from its daemon; herdr-core streams raw output, so the app's emulator
//! finds them. Both go through the same paths as Herdr 1's: `[bell]`, its
//! burst limit, quiet hours and the mute of the device the core runs on, and
//! the clipboard toast. A mirror only watches, so what
//! it sees is left to the tab that drives the terminal.

use super::{CoreTerminalView, Host};
use crate::{SheprdWindow, i18n::fl, window::Flash};
use gpui::{Context, Entity, Window};

/// Bells and the latest clipboard write a view collected since the window
/// last took them.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Effects {
    pub(crate) bells: u32,
    pub(crate) clipboard: Option<String>,
}

impl Effects {
    /// Adds what a newer take found; its clipboard write replaces an older
    /// one, as each write replaces the whole clipboard.
    pub(crate) fn merge(&mut self, newer: Self) {
        self.bells = self.bells.saturating_add(newer.bells);
        if newer.clipboard.is_some() {
            self.clipboard = newer.clipboard;
        }
    }
}

impl SheprdWindow {
    /// Takes new screens from `views` and what they ask of the window, each
    /// with the host whose core runs it while its tab is in sight. A view
    /// whose tab is in another workspace than the one shown keeps its screen
    /// current but rings nothing, nor does one whose host is muted as a
    /// device, as a Herdr daemon's bells are held back; clipboard writes
    /// still land, as Herdr's do from any pane.
    pub(super) fn poll_core_views(
        &mut self,
        views: impl IntoIterator<Item = (Entity<CoreTerminalView>, Option<Host>)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut effects = Effects::default();
        let mut dropped = false;
        for (view, shown) in views {
            let mut taken = view.update(cx, |view, cx| {
                view.poll(window, cx);
                view.take_effects()
            });
            dropped |= view.read(cx).take_dropped();
            let muted = |host: &Host| {
                crate::notification_mutes::Mutes::any(cx)
                    .is_some_and(|mutes| mutes.host(host.endpoint()))
            };
            if shown.as_ref().is_none_or(muted) {
                taken.bells = 0;
            }
            effects.merge(taken);
        }
        // Typing while the connection was away sent nothing: say so once.
        if dropped {
            self.show_flash(Flash::warning(fl!("core-terminals-input-dropped")), cx);
        }
        self.apply_core_effects(effects, window, cx);
    }

    /// Rings the bells as `[bell]` asks and writes the clipboard.
    fn apply_core_effects(
        &mut self,
        effects: Effects,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if effects.bells > 0 && !self.config.notifications.quiet() {
            self.bell
                .queue(u16::try_from(effects.bells).unwrap_or(u16::MAX));
            self.ring_bell(window);
        }
        if self.apply_clipboard_writes(effects.clipboard, cx) {
            cx.notify();
        }
    }
}
