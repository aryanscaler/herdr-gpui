//! Herdr 2 agents asking for the user. herdr-core sends no notifications, only
//! each program's status, so the app hears the changes itself: an agent that
//! becomes blocked plays the request sound, one whose turn ends plays the done
//! sound, and either counts on the Dock or taskbar badge. Herdr 1 decides this
//! in its daemon; the sound settings, their delay, and the rule that a finished
//! turn in the tab in front stays quiet are the same.

use crate::SheprdWindow;
use gpui::{prelude::*, *};
use sheprd_backend::{AgentStatus, TerminalId, TerminalInfo};
use sheprd_client::herdr::AgentKind;
use sheprd_client::protocol::SemanticNotificationSound;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    time::{Duration, Instant},
};

/// A change worth a sound.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum News {
    /// The agent waits on the user: a permission, a question, an error.
    NeedsYou,
    /// The agent's turn ended.
    Finished,
}

impl News {
    /// What a change from `before` to `now` tells the user. A terminal heard
    /// of for the first time tells nothing, so opening the app is quiet.
    pub(super) fn of(before: Option<AgentStatus>, now: AgentStatus) -> Option<Self> {
        match (before?, now) {
            (AgentStatus::Blocked, AgentStatus::Blocked) => None,
            (_, AgentStatus::Blocked) => Some(Self::NeedsYou),
            (AgentStatus::Working, AgentStatus::Done | AgentStatus::Idle) => Some(Self::Finished),
            _ => None,
        }
    }

    /// Whether `status` still says so once the delay is over.
    pub(super) fn holds(self, status: AgentStatus) -> bool {
        match self {
            Self::NeedsYou => status == AgentStatus::Blocked,
            Self::Finished => matches!(status, AgentStatus::Done | AgentStatus::Idle),
        }
    }

    fn sound(self) -> SemanticNotificationSound {
        match self {
            Self::NeedsYou => SemanticNotificationSound::Request,
            Self::Finished => SemanticNotificationSound::Done,
        }
    }
}

/// The most changes waiting out the delay; older ones give way.
const MAX_DUE: usize = 32;
/// How long past its delay a change may still play: one no window could play
/// in time, as every window lost the core, is old news.
const STALE: Duration = Duration::from_secs(5);

/// What each herdr-core agent last reported, and the changes waiting out the
/// delay, app-wide. Every window lists this computer's core on its own, so a
/// change is news once, in whichever window hears it first, and any window
/// still connected plays it.
#[derive(Default)]
pub(crate) struct Heard {
    agents: HashMap<TerminalId, Agent>,
    due: VecDeque<(Instant, TerminalId, News)>,
    /// The terminals each window shows while it is the active one.
    front: HashMap<WindowId, HashSet<TerminalId>>,
}

impl Global for Heard {}

/// What a terminal's agent last reported.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Agent {
    /// When the list that said so was asked for. Windows read the core on
    /// their own, each its own terminals, so an older list can arrive after
    /// a newer one.
    read: Instant,
    status: AgentStatus,
    /// Which agent it is, kept with its news, so whichever window plays it
    /// applies that agent's sound setting.
    kind: Option<AgentKind>,
}

/// A terminal one window listed: its status and agent.
pub(super) type Listed<'a> = (&'a TerminalId, AgentStatus, Option<AgentKind>);

impl Heard {
    /// Takes the status of each `listed` terminal, from a list asked for at
    /// `read`, and returns what changed into news. Terminals the core no
    /// longer had then, by `known`, are forgotten. What a newer list already
    /// said of a terminal stands against an older one.
    pub(super) fn hear<'a>(
        &mut self,
        read: Instant,
        listed: impl IntoIterator<Item = Listed<'a>>,
        known: impl Fn(&TerminalId) -> bool,
    ) -> Vec<(TerminalId, News)> {
        self.agents
            .retain(|terminal, agent| agent.read > read || known(terminal));
        listed
            .into_iter()
            .filter_map(|(terminal, status, kind)| {
                let before = self.agents.get(terminal);
                if before.is_some_and(|before| before.read > read) {
                    return None;
                }
                let news = News::of(before.map(|before| before.status), status);
                self.agents
                    .insert(terminal.clone(), Agent { read, status, kind });
                news.map(|news| (terminal.clone(), news))
            })
            .collect()
    }

    /// Queues `news` heard at `heard`, replacing what waited for the same
    /// terminal.
    pub(super) fn push(&mut self, heard: Instant, terminal: TerminalId, news: News) {
        self.due.retain(|(_, waiting, _)| *waiting != terminal);
        if self.due.len() == MAX_DUE {
            self.due.pop_front();
        }
        self.due.push_back((heard, terminal, news));
    }

    /// Records the terminals `window` shows in front: none unless it is
    /// active. Windows no longer `open` are forgotten.
    pub(super) fn show(
        &mut self,
        window: WindowId,
        shown: HashSet<TerminalId>,
        open: impl Fn(WindowId) -> bool,
    ) {
        self.front.retain(|id, _| open(*id));
        if shown.is_empty() {
            self.front.remove(&window);
        } else {
            self.front.insert(window, shown);
        }
    }

    /// Takes the news to play at `now`: past `delay`, still true of the
    /// latest status, and, for a finished turn, not in front of the user.
    /// What is still waiting stays; what went stale or untrue is dropped.
    pub(super) fn take_due(
        &mut self,
        now: Instant,
        delay: Duration,
    ) -> Vec<(TerminalId, News, Option<AgentKind>)> {
        let mut play = Vec::new();
        for (heard, terminal, news) in std::mem::take(&mut self.due) {
            if now < heard + delay {
                self.due.push_back((heard, terminal, news));
                continue;
            }
            let Some(agent) = self
                .agents
                .get(&terminal)
                .filter(|agent| news.holds(agent.status))
            else {
                continue;
            };
            let seen = news == News::Finished
                && self.front.values().any(|shown| shown.contains(&terminal));
            if !seen && now <= heard + delay + STALE {
                let kind = agent.kind;
                play.push((terminal, news, kind));
            }
        }
        play
    }
}

/// The terminals in front of the user in a window: none unless it is the
/// active one, connected; else the panes of the herdr-core tab its active
/// group shows (`page`), or the stand-in shell when that group shows the
/// terminal (`fallback`). A browser or code page in front hides both.
fn in_front(
    active: bool,
    page: Option<HashSet<TerminalId>>,
    fallback: Option<TerminalId>,
) -> HashSet<TerminalId> {
    if !active {
        return HashSet::new();
    }
    page.unwrap_or_else(|| fallback.into_iter().collect())
}

impl SheprdWindow {
    /// This computer's terminals the window shows, with what the core last
    /// said of each: its local tabs' panes, and the shell shown in place of
    /// Herdr 1's terminal when it is not installed.
    fn agent_terminals<'a>(&'a self, cx: &'a App) -> impl Iterator<Item = &'a TerminalInfo> {
        let fallback = self
            .core_fallback_view()
            .and_then(|view| self.core_terminals.infos.get(view.read(cx).terminal()));
        self.core_terminals.chat_terminals().chain(fallback)
    }

    /// Hears what changed in the latest list of the window's terminals.
    /// Call when the list changed.
    pub(super) fn hear_core_agents(&mut self, cx: &mut Context<Self>) {
        let Some(read) = self.core_terminals.infos.read_at() else {
            return;
        };
        let listed: Vec<(TerminalId, AgentStatus, Option<AgentKind>)> = self
            .agent_terminals(cx)
            .map(|info| {
                (
                    info.id.clone(),
                    info.agent,
                    sheprd_core_client::agent_kind(info),
                )
            })
            .collect();
        let infos = &self.core_terminals.infos;
        let heard = cx.default_global::<Heard>();
        let news = heard.hear(
            read,
            listed.iter().map(|(id, status, kind)| (id, *status, *kind)),
            |terminal| infos.get(terminal).is_some(),
        );
        let now = Instant::now();
        for (terminal, news) in news {
            heard.push(now, terminal, news);
        }
    }

    /// Plays the news whose delay is over and still holds, and counts the
    /// agents that need the user on the badge. Runs every tick, after
    /// `core_terminals.shown` is current. A window that lost the core
    /// counts, shows, and plays nothing, as what it last heard is stale.
    pub(super) fn tell_core_agents(&mut self, window: &Window, cx: &mut Context<Self>) {
        let id = window.window_handle().window_id();
        let connected = self.core_terminals.hub.backend().is_some();
        #[cfg(any(target_os = "macos", windows))]
        {
            // Counted as a Herdr 1 agent is: blocked, or done.
            let attention: HashSet<TerminalId> = self
                .agent_terminals(cx)
                .filter(|_| connected)
                .filter(|info| matches!(info.agent, AgentStatus::Blocked | AgentStatus::Done))
                .map(|info| info.id.clone())
                .collect();
            crate::app_badge::sync_core(id, attention, cx);
        }
        // What the active window shows: the tab of its active group, a
        // chat's included, whatever holds the keyboard, or the stand-in
        // shell while that group shows the terminal rather than a page.
        let page = self.core_terminals.shown.as_ref().map(|(tab, _)| {
            self.core_terminals
                .tabs
                .get(tab)
                .filter(|shown| shown.holds_chats())
                .map(|shown| shown.panes.terminals().into_iter().cloned().collect())
                .unwrap_or_default()
        });
        let terminal_shown = self.active_group().is_some_and(|group| {
            matches!(self.group_shown(group, cx), crate::browser::Shown::Terminal)
        });
        let fallback = self
            .core_fallback_view()
            .map(|view| view.read(cx).terminal().clone());
        let shown = in_front(
            connected && window.is_window_active(),
            page,
            terminal_shown.then_some(fallback).flatten(),
        );
        let open: HashSet<WindowId> = cx
            .windows()
            .iter()
            .map(|handle| handle.window_id())
            .collect();
        let heard = cx.default_global::<Heard>();
        heard.show(id, shown, |window| open.contains(&window));
        if !connected || heard.due.is_empty() {
            return;
        }
        // Until the settings load, the default delay of a second.
        let delay = Duration::from_secs(self.sound.delay().unwrap_or(1));
        for (terminal, news, kind) in heard.take_due(Instant::now(), delay) {
            self.sound.play_agent(
                news.sound(),
                kind.map(|kind| kind.name()),
                terminal.as_str(),
            );
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests;
