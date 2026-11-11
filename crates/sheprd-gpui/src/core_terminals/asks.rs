//! What a herdr-core view asks of its window: to open a link clicked in it,
//! or to say why something was refused. A file link that opens in the
//! terminal editor gets a herdr-core split of its own beside the pane, as a
//! Herdr pane's editor opens in a Herdr split.

use super::{Purpose, view::ToWindow, view::WindowCall};
use crate::{
    SheprdWindow,
    editor::EditorTarget,
    i18n::{Localize as _, fl},
    window::Flash,
};
use gpui::{prelude::*, *};
use sheprd_backend::{Spawn, TerminalId};
use std::rc::Rc;

/// How views made by this window reach it. The window's tick polls its
/// views while it is being updated, so an ask runs once that update ends,
/// in the order asked. A closed window answers nothing.
pub(super) fn window_call(cx: &Context<SheprdWindow>) -> WindowCall {
    let owner = cx.entity().downgrade();
    Rc::new(move |ask, window, cx| {
        let owner = owner.clone();
        window.defer(cx, move |window, cx| {
            let _ = owner.update(cx, |this, cx| this.core_view_asks(ask, window, cx));
        });
    })
}

impl SheprdWindow {
    fn core_view_asks(&mut self, ask: ToWindow, window: &mut Window, cx: &mut Context<Self>) {
        match ask {
            ToWindow::OpenLink(click) => self.open_core_link(click, window, cx),
            ToWindow::Copied if self.config.clipboard_toast.enabled => {
                self.show_flash(Flash::success(fl!("window-find-copied")), cx);
            }
            ToWindow::Copied => {}
            ToWindow::Refused(error) => self.show_flash(Flash::warning(error.localize()), cx),
        }
    }

    /// Opens `target` in the terminal editor beside `pane` when `pane` is a
    /// herdr-core terminal: in a herdr-core split of its tab, or, for the
    /// shell shown in place of Herdr 1, in a new herdr-core tab. False when
    /// `pane` is no herdr-core terminal, for the caller to open it as it
    /// would for a Herdr pane.
    pub(crate) fn open_link_in_core_editor(
        &mut self,
        target: &EditorTarget,
        pane: &str,
        cx: &mut Context<Self>,
    ) -> bool {
        let Ok(at) = TerminalId::new(pane) else {
            return false;
        };
        let purpose = match self.core_terminals.tab_of(&at) {
            Some(tab) => {
                self.open_in_core_editor(target, tab, at, cx);
                return true;
            }
            // The shell shown in place of Herdr 1, or a terminal only a
            // mirror shows, which takes no split: a tab of its own.
            None if self.core_fallback_terminal(cx).as_ref() == Some(&at)
                || self.core_terminals.mirrors(&at) =>
            {
                match self.browser_key() {
                    Some((scope, workspace)) => Purpose::Tab { scope, workspace },
                    // No workspace to hold a tab: the system opens it.
                    None => {
                        self.open_in_system_app(target.path.clone(), cx);
                        return true;
                    }
                }
            }
            None => return false,
        };
        let argv = match crate::editor::argv(target, self.config.editor_command.as_ref()) {
            Ok(argv) => argv,
            Err(error) => {
                self.show_flash(Flash::warning(error.localize()), cx);
                return true;
            }
        };
        let spawn = Spawn {
            argv,
            cwd: target.path.parent().map(std::path::Path::to_owned),
            ..Spawn::default()
        };
        self.start_core_program(purpose, spawn, cx);
        true
    }
}
