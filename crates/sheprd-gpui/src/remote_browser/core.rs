//! A window's side of requests from agents in Herdr 2 terminals on another
//! host (see `core_terminals::requests`): serve each new one once, in the
//! workspace of the tab that shows its terminal, then remove its labels.
//! Caller identity comes from the terminal carrying the labels, never from
//! a pane identifier supplied by the requesting process.

use super::{PART, PREFIX, Request, Seen, SeenKey};
use crate::{SheprdWindow, browser::Store};
use gpui::{Context, Window};

impl SheprdWindow {
    pub(super) fn poll_core_browser_requests(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.poll_remote_feedback(window, cx);
        for found in self.core_terminals.label_requests() {
            let Some(index) = self
                .endpoints
                .iter()
                .position(|endpoint| crate::browser::scope(endpoint) == found.scope)
            else {
                continue;
            };
            let Some(workspace) = cx
                .try_global::<Store>()
                .and_then(|store| store.get(found.tab))
                .map(|tab| tab.workspace_id.clone())
            else {
                continue;
            };
            let mut served = Vec::new();
            for request in Request::all(&found.tokens) {
                if let super::Action::Feedback { wait_seconds } = request.action {
                    self.begin_remote_feedback(
                        &found.scope,
                        &found.terminal,
                        &request.id,
                        wait_seconds,
                        window,
                        cx,
                    );
                    continue;
                }
                let name = format!("{PREFIX}{}", request.id);
                let part = format!("{name}{PART}");
                served.extend(
                    found
                        .tokens
                        .iter()
                        .map(|(token, _)| token)
                        .filter(|token| **token == name || token.starts_with(&part))
                        .cloned(),
                );
                let key = SeenKey {
                    scope: found.scope.clone(),
                    workspace: workspace.clone(),
                    id: format!("{}:{}", found.terminal, request.id),
                    window: matches!(request.action, super::Action::Reload)
                        .then_some(window.window_handle().window_id().as_u64()),
                };
                let request = Request {
                    pane: Some(crate::chat::core_pane::key_on(
                        &crate::core_terminals::Host::Remote(found.scope.clone()),
                        &found.terminal,
                    )),
                    ..request
                };
                if cx.default_global::<Seen>().first(key) {
                    self.serve_remote_request(index, workspace.clone(), request, window, cx);
                }
            }
            if !served.is_empty() {
                self.core_terminals
                    .clear_labels(&found.scope, &found.terminal, served);
            }
        }
    }
}
