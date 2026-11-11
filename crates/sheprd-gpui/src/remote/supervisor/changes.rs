//! The supervisor's part in answering phones about workspaces: while a phone
//! is connected it waits on the daemon for their questions, one long request
//! at a time, the next tick starting the next, and answers each on the
//! background executor. With no phone connected it asks nothing. A phone's
//! split or zoom of a Herdr 2 tab arrives the same way, and the window that
//! holds the tab makes it at once.
//!
//! The daemon's limit counts only questions a phone still waits for, and an
//! answer keeps running after its phone gives up, so the answers running here
//! are limited too; questions taken beyond that wait their turn here. Taking
//! goes on meanwhile, so a split or zoom, which needs no answer worked out,
//! does not wait behind slow workspace answers. A phone that gives up frees
//! its place at the daemon but not here, so taking stops once `QUEUED`
//! questions wait here.

use super::*;
use crate::remote::changes::Sources;
use crate::window::main_window::MainWindow;
use sheprd_remote_control::control::{
    changes::{Ask, AskId},
    layout,
};
use std::collections::VecDeque;

/// How long after a failed wait the daemon is asked again.
const RETRY: Duration = Duration::from_secs(5);

/// How many answers are worked out at once.
pub(super) const RUNNING: usize = 4;
/// How many taken questions may wait for one of those: the daemon's own
/// limit, so the work queued here stays bounded however phones come and go.
pub(super) const QUEUED: usize = 16;

#[derive(Default)]
pub(crate) struct Changes {
    polling: bool,
    /// Questions taken from the daemon that wait for a running answer to end.
    pub(super) queued: VecDeque<Ask>,
    retry_at: Option<Instant>,
    /// Answers being worked out now, at most `RUNNING`.
    pub(super) running: usize,
    pub(super) sources: Arc<Sources>,
}

impl Supervisor {
    /// Whether remote control is on, so phones may start work here.
    pub(crate) fn enabled(cx: &App) -> bool {
        cx.try_global::<Self>()
            .is_some_and(|supervisor| supervisor.config.enabled)
    }

    /// The workspaces phones may ask about, as a window last saw them.
    pub(super) fn set_change_sources(&mut self, sources: Sources) {
        self.changes.sources = Arc::new(sources);
    }

    /// Waits for phones' questions, unless that is underway or no phone
    /// could be asking.
    pub(super) fn poll_changes(cx: &mut App) {
        let executor = cx.background_executor().clone();
        let supervisor = cx.global_mut::<Self>();
        let phones = matches!(&supervisor.health, Health::Running(status) if status.connected > 0);
        let waiting = supervisor
            .changes
            .retry_at
            .is_some_and(|at| Instant::now() < at);
        let full = supervisor.changes.queued.len() >= QUEUED;
        if !phones || waiting || full || supervisor.changes.polling {
            return;
        }
        let Some(address) = supervisor.address.clone() else {
            return;
        };
        supervisor.changes.polling = true;
        let send = supervisor.send;
        let task = executor.spawn(async move { send(&address, &Request::ChangeAsks) });
        cx.spawn(async move |cx| {
            let answer = task.await;
            cx.update(|cx| Self::take_asks(cx, answer));
        })
        .detach();
    }

    pub(super) fn take_asks(cx: &mut App, answer: Result<Response, control::Error>) {
        let supervisor = cx.global_mut::<Self>();
        supervisor.changes.polling = false;
        let Ok(Response::ChangeAsks { asks, layout }) = answer else {
            // A daemon that does not know the request answers at once.
            supervisor.changes.retry_at = Some(Instant::now() + RETRY);
            return;
        };
        supervisor.changes.retry_at = None;
        supervisor.changes.queued.extend(asks);
        Self::start_answers(cx);
        for ask in layout {
            // A split's window answers once its shell is in place.
            if let Some(answer) = change_layout(cx, &ask) {
                Self::send_layout_answer(cx, ask.id, answer);
            }
        }
    }

    /// Hands what a window did with a phone's layout change to the daemon.
    pub(crate) fn send_layout_answer(cx: &mut App, id: AskId, answer: layout::Answer) {
        let Some(supervisor) = cx.try_global::<Self>() else {
            return;
        };
        let Some(address) = supervisor.address.clone() else {
            return;
        };
        let send = supervisor.send;
        cx.background_executor()
            .spawn(async move {
                let request = Request::LayoutAnswer { id, answer };
                if let Err(error) = send(&address, &request) {
                    tracing::debug!(%error, "The daemon did not take a layout answer for a phone");
                }
            })
            .detach();
    }

    /// Starts queued answers while fewer than `RUNNING` run.
    fn start_answers(cx: &mut App) {
        let executor = cx.background_executor().clone();
        let supervisor = cx.global_mut::<Self>();
        let Some(address) = supervisor.address.clone() else {
            // With no daemon to answer, nobody waits for these.
            supervisor.changes.queued.clear();
            return;
        };
        let (send, sources) = (supervisor.send, supervisor.changes.sources.clone());
        let mut started = Vec::new();
        while supervisor.changes.running < RUNNING {
            let Some(ask) = supervisor.changes.queued.pop_front() else {
                break;
            };
            supervisor.changes.running += 1;
            let (address, sources) = (address.clone(), sources.clone());
            started.push(executor.spawn(async move { reply(&address, send, &sources, &ask) }));
        }
        for task in started {
            cx.spawn(async move |cx| {
                task.await;
                cx.update(|cx| {
                    let changes = &mut cx.global_mut::<Self>().changes;
                    changes.running = changes.running.saturating_sub(1);
                    Self::start_answers(cx);
                });
            })
            .detach();
        }
    }
}

/// Makes a phone's change in the window that holds its tab; `None` when
/// that window answers later.
fn change_layout(cx: &mut App, ask: &layout::Ask) -> Option<layout::Answer> {
    if !ask.valid() {
        return Some(layout::Answer::Failed(layout::Failure::Refused));
    }
    for window in MainWindow::all(cx) {
        let made = window.update(cx, |this, window, cx| this.phone_layout(ask, window, cx));
        if let Ok(Some(answer)) = made {
            return answer;
        }
    }
    Some(layout::Answer::Failed(layout::Failure::Gone))
}

/// Works out one answer and hands it to the daemon. Blocking.
fn reply(address: &Path, send: Send, sources: &Sources, ask: &Ask) {
    let answer = sources.answer(ask);
    let id = ask.id;
    if let Err(error) = send(address, &Request::ChangeAnswer { id, answer }) {
        tracing::debug!(%error, "The daemon did not take an answer for a phone");
    }
}
