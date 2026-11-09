//! With no Herdr 1 installed, the window's terminal is a herdr-core shell:
//! the user's running core, else the user's `herdr-core`, else the copy
//! bundled beside the app. The shell is named, so every launch comes back
//! to the same one instead of starting another. Only when herdr-core is
//! unavailable too does the window ask the user to install Herdr.

use super::{CoreTerminalView, Host, Start, State, worker};
use crate::SheprdWindow;
use gpui::{prelude::*, *};
use sheprd_backend::{Backend as _, Role, Size, Spawn, TerminalId};
use std::sync::mpsc;

/// The name of the shell the window shows in place of Herdr 1.
const SHELL: &str = "herdr-ui";

#[derive(Default)]
pub(crate) enum Fallback {
    #[default]
    Off,
    /// The shell starting, or found by name.
    Starting(mpsc::Receiver<Result<TerminalId, sheprd_core_client::Error>>),
    Shown(Entity<CoreTerminalView>),
}

impl super::CoreTerminals {
    /// The address of the shell shown in place of Herdr 1, such as `t3`,
    /// once herdr-core has listed it.
    pub(super) fn fallback_terminal(&self) -> Option<&TerminalId> {
        matches!(self.fallback, Fallback::Shown(_))
            .then(|| self.infos.by_name(SHELL))
            .flatten()
            .map(|info| &info.id)
    }

    /// What in this window holds herdr-core `terminal`, for control
    /// requests from it: a tab whose panes hold it, or the shell shown in
    /// place of Herdr 1.
    pub(crate) fn holder(&self, terminal: &TerminalId) -> Option<Holder> {
        Holder::of(self.tab_of(terminal), self.fallback_terminal(), terminal)
    }
}

/// What holds the herdr-core terminal a control request comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Holder {
    /// The shell shown in place of Herdr 1, in its stand-in workspace.
    Fallback,
    Tab(crate::browser::TabId),
}

impl Holder {
    pub(super) fn of(
        tab: Option<crate::browser::TabId>,
        fallback: Option<&TerminalId>,
        terminal: &TerminalId,
    ) -> Option<Self> {
        tab.map(Self::Tab)
            .or_else(|| (fallback == Some(terminal)).then_some(Self::Fallback))
    }
}

impl SheprdWindow {
    /// Runs every tick: while Herdr 1 is missing, connects and shows the
    /// shell; once Herdr 1 answers again, gives the window back to it.
    pub(super) fn poll_core_fallback(&mut self, cx: &mut Context<Self>) {
        if !self.live.missing_installation {
            if !matches!(self.core_terminals.fallback, Fallback::Off) {
                self.core_terminals.fallback = Fallback::Off;
                cx.notify();
            }
            return;
        }
        self.core_terminals.hub.connect(Start::IfMissing);
        // The shell belongs to the default workspace: once the user closes
        // it, the shell ends with it and none starts again until the app
        // keeps that workspace anew.
        let kept = self.default_workspace_kept(cx);
        match &self.core_terminals.fallback {
            Fallback::Off if kept => self.start_fallback_shell(),
            Fallback::Off => {}
            Fallback::Starting(found) => match found.try_recv() {
                Ok(Ok(terminal)) if !kept => {
                    self.end_core_terminal(&Host::Local, terminal);
                    self.core_terminals.fallback = Fallback::Off;
                }
                Ok(Ok(terminal)) => self.show_fallback_shell(terminal, cx),
                Ok(Err(error)) => {
                    tracing::warn!(%error, "Could not start the herdr-core shell");
                    self.core_terminals.fallback = Fallback::Off;
                }
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.core_terminals.fallback = Fallback::Off
                }
            },
            Fallback::Shown(view) if !kept => {
                self.end_core_terminal(&Host::Local, view.read(cx).terminal().clone());
                self.core_terminals.fallback = Fallback::Off;
                cx.notify();
            }
            Fallback::Shown(_) => {}
        }
    }

    fn start_fallback_shell(&mut self) {
        let Some(backend) = self.core_terminals.hub.backend().cloned() else {
            return;
        };
        let (send, receive) = mpsc::sync_channel(1);
        let spawn = Spawn {
            name: Some(SHELL.to_owned()),
            ..Spawn::default()
        };
        let spawned = std::thread::Builder::new()
            .name("herdr-core-shell".into())
            .spawn(move || {
                let found = match backend.spawn(&spawn) {
                    Ok(info) => Ok(info.id),
                    // An earlier launch's shell still runs: show it again.
                    Err(sheprd_core_client::Error::Refused {
                        kind: Some(kind), ..
                    }) if kind == "name_taken" => {
                        TerminalId::new(SHELL).map_err(sheprd_core_client::Error::from)
                    }
                    Err(error) => Err(error),
                };
                let _ = send.send(found);
            });
        if spawned.is_ok() {
            self.core_terminals.fallback = Fallback::Starting(receive);
        }
    }

    fn show_fallback_shell(&mut self, terminal: TerminalId, cx: &mut Context<Self>) {
        let (Some(backend), Ok(size)) = (
            self.core_terminals.hub.backend().cloned(),
            Size::new(80, 24),
        ) else {
            self.core_terminals.fallback = Fallback::Off;
            return;
        };
        let link = match worker::spawn(backend, terminal.clone(), size, Role::Active) {
            Ok(link) => link,
            Err(error) => {
                tracing::warn!(%error, "Could not start a herdr-core terminal worker");
                self.core_terminals.fallback = Fallback::Off;
                return;
            }
        };
        let (look, painter) = (self.core_look(), self.painter.clone());
        let to_window = super::asks::window_call(cx);
        let view = cx.new(|cx| {
            CoreTerminalView::new(terminal, link, look, painter, false, cx).with_window(to_window)
        });
        self.core_terminals.fallback = Fallback::Shown(view);
        cx.notify();
    }

    /// The shell shown in place of Herdr 1's terminal, while it is.
    /// The terminal of the shell shown in place of Herdr 1.
    pub(super) fn core_fallback_terminal(&self, cx: &App) -> Option<TerminalId> {
        Some(self.core_fallback_view()?.read(cx).terminal().clone())
    }

    pub(crate) fn core_fallback_view(&self) -> Option<Entity<CoreTerminalView>> {
        match &self.core_terminals.fallback {
            Fallback::Shown(view) => Some(view.clone()),
            _ => None,
        }
    }

    /// The shell shown in place of Herdr 1's terminal, marked as drawn in
    /// this frame.
    pub(crate) fn draw_core_fallback(&mut self) -> Option<AnyElement> {
        let view = self.core_fallback_view()?;
        self.core_terminals.draw(&view);
        Some(view.into_any_element())
    }

    /// Whether the window should still ask the user to install Herdr: only
    /// once herdr-core is known to be unavailable as well.
    pub(crate) fn needs_herdr_install(&self) -> bool {
        matches!(self.core_terminals.hub.state(), State::Unavailable(_))
    }
}
