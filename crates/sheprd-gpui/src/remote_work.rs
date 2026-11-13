//! Work commands from a remote terminal, with separate authority from browser
//! requests. Claim on the core before acting; an ambiguous result never retries.
mod claim;

use crate::{
    SheprdWindow,
    browser::{Scope, Store, TabId},
    core_terminals::Host,
    window::main_window::MainWindow,
};
use gpui::{Context, Global, Window};
use sheprd_backend::TerminalId;
use sheprd_control_protocol::protocol::{
    ErrorCode, Response,
    work::remote::{self, Request},
};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

const LIMIT: usize = 8;
const TIMEOUT: Duration = Duration::from_secs(30);
type Key = (Scope, TerminalId, String);

#[derive(Default)]
struct Claims(HashMap<Key, Instant>);
impl Global for Claims {}

#[derive(Default)]
pub(crate) struct State {
    pending: Vec<Pending>,
}

struct Pending {
    workspace: String,
    scope: Scope,
    terminal: TerminalId,
    tab: TabId,
    key: String,
    claim: String,
    reply: String,
    request: Option<Result<Request, Response>>,
    backend: sheprd_core_client::CoreBackend,
    started: Instant,
    endpoint: sheprd_client::ConnectTarget,
    step: Step,
}

/// Caller identity captured before the asynchronous claim starts.
#[derive(Clone, Copy)]
pub(crate) struct Target<'a> {
    pub(crate) scope: &'a Scope,
    pub(crate) terminal: &'a TerminalId,
    pub(crate) tab: TabId,
    pub(crate) workspace: &'a str,
}

impl Pending {
    fn target(&self) -> Target<'_> {
        Target {
            scope: &self.scope,
            terminal: &self.terminal,
            tab: self.tab,
            workspace: &self.workspace,
        }
    }
}

enum Step {
    Claim(std::sync::mpsc::Receiver<sheprd_core_client::Result<bool>>),
    Reply(std::sync::mpsc::Receiver<sheprd_core_client::Result<()>>),
    Waiting,
}

impl SheprdWindow {
    pub(crate) fn poll_remote_work(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let now = Instant::now();
        let pending = std::mem::take(&mut self.remote_work.pending);
        for mut pending in pending {
            let current = self.endpoints.iter().any(|endpoint| {
                crate::browser::scope(endpoint) == pending.scope
                    && endpoint.saved_target() == &pending.endpoint
            }) && self
                .core_terminals
                .hub_of(&Host::Remote(pending.scope.clone()))
                .and_then(|hub| hub.backend())
                .is_some_and(|backend| {
                    !backend.core().is_lost()
                        && std::ptr::eq(backend.core(), pending.backend.core())
                });
            let value = self.core_terminals.browser_request_value(
                &pending.scope,
                &pending.terminal,
                &pending.key,
            );
            if !current || value.is_none() || now.duration_since(pending.started) >= TIMEOUT {
                if current {
                    self.core_terminals.clear_labels(
                        &pending.scope,
                        &pending.terminal,
                        vec![pending.key, pending.claim, pending.reply],
                    );
                }
                continue;
            }
            match &pending.step {
                Step::Claim(receiver) => match receiver.try_recv() {
                    Ok(Ok(true)) => {
                        let Some(request) = pending.request.take() else {
                            continue;
                        };
                        let response = match request {
                            Ok(request) => {
                                self.serve_remote_work(pending.target(), request, window, cx)
                            }
                            Err(response) => response,
                        };
                        let response = remote::encode_reply(&response).unwrap_or_else(|_| {
                            // Fixed, bounded fallback; no user text or checkout data.
                            r#"{"status":"error","code":"failed","message":"Remote work reply is too large"}"#.into()
                        });
                        let Some(reply) = self.core_terminals.browser_reply(
                            &pending.scope,
                            &pending.terminal,
                            pending.reply.clone(),
                            response,
                        ) else {
                            continue;
                        };
                        pending.step = Step::Reply(reply);
                    }
                    Ok(Ok(false) | Err(_)) | Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        continue;
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => {}
                },
                Step::Reply(receiver) => match receiver.try_recv() {
                    Ok(Ok(())) => pending.step = Step::Waiting,
                    Ok(Err(_)) | Err(std::sync::mpsc::TryRecvError::Disconnected) => continue,
                    Err(std::sync::mpsc::TryRecvError::Empty) => {}
                },
                Step::Waiting => {}
            }
            self.remote_work.pending.push(pending);
        }
        for found in self.core_terminals.work_label_requests() {
            let Some(workspace) = cx
                .try_global::<Store>()
                .and_then(|store| store.get(found.tab))
                .filter(|tab| tab.scope == found.scope)
                .map(|tab| tab.workspace_id.clone())
            else {
                continue;
            };
            for (key, value) in found.tokens {
                let Some(id) = remote::request_id(&key) else {
                    continue;
                };
                if value == "claimed" {
                    continue;
                }
                let mut request = Request::decode(&value)
                    .map_err(|error| Response::error(ErrorCode::InvalidRequest, error.to_string()));
                // Let another authoritative holder answer its explicitly
                // named request. Never substitute this window for a wrong one.
                if let Ok(asked) = &request
                    && let Some(id) = asked
                        .window
                        .filter(|id| *id != window.window_handle().window_id().as_u64())
                {
                    let holds = MainWindow::all(cx)
                        .into_iter()
                        .find(|handle| handle.window_id().as_u64() == id)
                        .is_some_and(|handle| {
                            handle
                                .read_with(cx, |view, _| {
                                    view.core_terminals
                                        .drives_work_terminal(&found.scope, &found.terminal)
                                })
                                .unwrap_or(false)
                        });
                    if holds {
                        continue;
                    }
                    request = Err(Response::error(
                        ErrorCode::WorkspaceNotFound,
                        "The requested window does not drive this terminal on its host",
                    ));
                }
                if self.remote_work.pending.len() >= LIMIT {
                    continue;
                }
                let Some(endpoint) = self
                    .endpoints
                    .iter()
                    .find(|endpoint| crate::browser::scope(endpoint) == found.scope)
                else {
                    continue;
                };
                let endpoint = endpoint.saved_target().clone();
                let claimed = cx.default_global::<Claims>();
                claimed
                    .0
                    .retain(|_, started| now.duration_since(*started) < TIMEOUT * 4);
                let identity = (found.scope.clone(), found.terminal.clone(), key.clone());
                if claimed.0.len() >= LIMIT * 8 || claimed.0.contains_key(&identity) {
                    continue;
                }
                claimed.0.insert(identity, now);
                let Some(backend) = self
                    .core_terminals
                    .hub_of(&Host::Remote(found.scope.clone()))
                    .and_then(|hub| hub.backend())
                    .cloned()
                else {
                    continue;
                };
                let claim = format!("{}{id}", remote::CLAIM_PREFIX);
                let Some(receiver) = claim::start(
                    backend.clone(),
                    found.terminal.clone(),
                    key.clone(),
                    claim.clone(),
                    value,
                ) else {
                    continue;
                };
                self.remote_work.pending.push(Pending {
                    workspace: workspace.clone(),
                    scope: found.scope.clone(),
                    terminal: found.terminal.clone(),
                    tab: found.tab,
                    reply: format!("{}{id}", remote::REPLY_PREFIX),
                    key,
                    claim,
                    request: Some(request),
                    backend,
                    started: now,
                    endpoint,
                    step: Step::Claim(receiver),
                });
            }
        }
    }
}
