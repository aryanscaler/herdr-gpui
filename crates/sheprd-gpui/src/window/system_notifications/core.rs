//! System notifications for Herdr 2 agents. herdr-core sends no
//! notifications, so the window posts one itself for each change its
//! attention service plays (`core_terminals::attention`): an agent that
//! needs the user, or one whose turn ended out of view. The title names the
//! agent, the body its task and place, as a Herdr 1 notice's does, and a
//! click shows its pane in whichever window holds it.

use super::{Posted, Registry, Target, body, deliver};
use crate::{SheprdWindow, i18n::fl};
use gpui::{App, Context, SharedString, SystemNotification, WindowHandle};
use sheprd_backend::TerminalId;
use sheprd_client::protocol::SemanticNotificationKind;
use std::time::Instant;

/// Untrusted titles and labels are cut here, as a Herdr 1 notice's are.
const TEXT_LIMIT: usize = 160;

/// The tag of `terminal`'s notification: one per terminal, so a newer change
/// replaces an older one where the platform supports it.
fn tag(terminal: &TerminalId) -> String {
    format!("herdr-core:{terminal}")
}

/// The title of `kind`'s notification about `agent`.
pub(super) fn title(kind: SemanticNotificationKind, agent: &str) -> String {
    match kind {
        SemanticNotificationKind::NeedsAttention => {
            fl!("core-notifications-needs-you", agent = agent)
        }
        _ => fl!("core-notifications-finished", agent = agent),
    }
}

/// The body's lines: the terminal's own title, when it says more than the
/// agent's name, over the agent's place.
pub(super) fn text(task: Option<&str>, agent: &str, place: Option<&str>) -> Option<String> {
    let task = task
        .map(|task| crate::notifications::safe_text(task, TEXT_LIMIT))
        .map(|task| task.trim().to_owned())
        .filter(|task| !task.is_empty() && task != agent);
    match (task, place) {
        (Some(task), Some(place)) => Some(format!("{task}\n{place}")),
        (Some(text), None) => Some(text),
        (None, Some(place)) => Some(place.to_owned()),
        (None, None) => None,
    }
}

/// What a notification names: the agent, its terminal's title, and where
/// it works.
#[derive(Default)]
struct Details {
    agent: Option<String>,
    task: Option<String>,
    place: Option<String>,
}

impl SheprdWindow {
    /// Posts a system notification about the agent in herdr-core terminal
    /// `terminal`, when notices go to the OS, the quiet hours allow it, and,
    /// for a finished turn, finished notices are on.
    /// A no-op without the registry, which only normal launches install.
    pub(crate) fn post_core_notification(
        &self,
        handle: WindowHandle<SheprdWindow>,
        terminal: &TerminalId,
        kind: SemanticNotificationKind,
        cx: &mut Context<Self>,
    ) {
        let config = self.config.notifications;
        if config.delivery() != crate::config::NotificationDelivery::System
            || config.quiet()
            || (kind == SemanticNotificationKind::Finished && !config.finished)
        {
            return;
        }
        if cx.try_global::<Registry>().is_none() {
            return;
        }
        // Any window may hear the change first; the details are the
        // window's that holds the agent.
        let details = self.core_agent_details(terminal, cx).or_else(|| {
            cx.windows()
                .into_iter()
                .filter_map(|window| window.downcast::<SheprdWindow>())
                .filter(|window| *window != handle)
                .find_map(|window| window.read(cx).ok()?.core_agent_details(terminal, cx))
        });
        let Details { agent, task, place } = details.unwrap_or_default();
        let agent = agent.unwrap_or_else(|| fl!("core-notifications-agent"));
        let task = task.as_deref();
        // The host leads the place line once there is another host to tell
        // this computer from.
        let host = (self.endpoints.len() > 1)
            .then(|| {
                self.endpoints
                    .iter()
                    .find(|endpoint| !endpoint.connection.target.is_remote())
                    .map(|endpoint| crate::notifications::safe_text(&endpoint.label, 80))
            })
            .flatten();
        let text = text(task, &agent, place.as_deref());
        let tag = SharedString::from(tag(terminal));
        let notification = SystemNotification {
            tag: tag.clone(),
            title: title(kind, &agent).into(),
            body: body(host.as_deref(), text.as_deref()),
            actions: Vec::new(),
        };
        cx.global_mut::<Registry>().record(Posted {
            tag,
            window: handle,
            target: Target::Core(terminal.clone()),
            at: Instant::now(),
            #[cfg(target_os = "linux")]
            bus_id: None,
        });
        deliver(notification, cx);
    }

    /// What a notification about `terminal`'s agent says of it, from this
    /// window, while it holds the agent.
    fn core_agent_details(&self, terminal: &TerminalId, cx: &App) -> Option<Details> {
        if !self.holds_core_agent(terminal, cx) {
            return None;
        }
        let info = self.core_agent_info(terminal, cx);
        Some(Details {
            agent: info.and_then(sheprd_core_client::agent_name),
            task: info.and_then(|info| info.title.clone()),
            place: self.core_agent_place(terminal, cx),
        })
    }

    /// What the core last said of `terminal`: a pane of one of the window's
    /// tabs, or the shell shown in place of Herdr 1.
    fn core_agent_info<'a>(
        &'a self,
        terminal: &TerminalId,
        cx: &'a App,
    ) -> Option<&'a sheprd_backend::TerminalInfo> {
        self.core_terminals
            .chat_terminals()
            .find(|info| info.id == *terminal)
            .or_else(|| {
                let fallback = self.core_fallback_view()?;
                let shown = fallback.read(cx).terminal();
                (shown == terminal)
                    .then(|| {
                        self.core_terminals
                            .reported(&crate::core_terminals::Host::Local, shown)
                    })
                    .flatten()
            })
    }

    /// Where `terminal`'s agent works: its tab's workspace, and the tab's
    /// title when it has one.
    fn core_agent_place(&self, terminal: &TerminalId, cx: &App) -> Option<String> {
        let tab = self.core_terminals.tab_of(terminal)?;
        let shown = cx.try_global::<crate::browser::Store>()?.get(tab)?;
        let workspace = self
            .core_host()
            .map(|index| self.sessions(index))
            .and_then(|sessions| {
                sessions
                    .workspace(&shown.workspace_id)
                    .map(|workspace| workspace.label.clone())
            });
        let tab_title = Some(shown.title.trim()).filter(|title| !title.is_empty());
        let place = match (workspace, tab_title) {
            (Some(workspace), Some(tab)) => format!("{workspace} · {tab}"),
            (Some(workspace), None) => workspace,
            (None, Some(tab)) => tab.to_owned(),
            (None, None) => return None,
        };
        Some(crate::notifications::safe_text(&place, TEXT_LIMIT))
    }

    /// Whether this window shows `terminal`, in a tab or as the shell shown
    /// in place of Herdr 1.
    fn holds_core_agent(&self, terminal: &TerminalId, cx: &App) -> bool {
        self.core_terminals.tab_of(terminal).is_some()
            || self
                .core_fallback_view()
                .is_some_and(|view| view.read(cx).terminal() == terminal)
    }

    /// Shows the pane of `terminal`'s agent, as a click on its notification
    /// does. A settings page in front keeps the window as it is, as it does
    /// for a Herdr 1 notice.
    fn open_core_notification(
        &mut self,
        terminal: &TerminalId,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        window.activate_window();
        if self.menu.page.is_some() {
            return;
        }
        if let Some(tab) = self.core_terminals.tab_of(terminal) {
            self.open_core_overview_row(tab, terminal, window, cx);
        } else if let Some(view) = self.core_fallback_view() {
            window.focus(&view.read(cx).focus_handle().clone(), cx);
        }
    }
}

/// Opens the agent a click on its notification names: in the window that
/// posted it while it still holds the agent, else in another that does.
/// Every window lists this computer's core, so the one that heard the
/// change first may not be the one that shows it.
pub(super) fn open(
    posted: WindowHandle<SheprdWindow>,
    terminal: &TerminalId,
    cx: &mut App,
) -> bool {
    let holds = |handle: WindowHandle<SheprdWindow>, cx: &App| {
        handle
            .read(cx)
            .is_ok_and(|view| view.holds_core_agent(terminal, cx))
    };
    let target = std::iter::once(posted)
        .chain(
            cx.windows()
                .into_iter()
                .filter_map(|window| window.downcast::<SheprdWindow>()),
        )
        .find(|handle| holds(*handle, cx))
        .unwrap_or(posted);
    target
        .update(cx, |view, window, cx| {
            view.open_core_notification(terminal, window, cx);
        })
        .is_ok()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests;
