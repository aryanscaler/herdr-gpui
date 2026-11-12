use super::*;
use crate::{
    core_terminals::{Host, LinkClick},
    link_handlers,
};
use sheprd_client::ConnectTarget;
use std::sync::atomic::{AtomicBool, Ordering};

pub(super) struct Running {
    token: Arc<AtomicBool>,
    _timeout: Task<()>,
}

impl Drop for Running {
    fn drop(&mut self) {
        self.token.store(true, Ordering::Release);
    }
}

impl SheprdWindow {
    /// True once a handler claims this click, including errors/busy. There is
    /// never a fallback after execution may have begun.
    pub(super) fn activate_core_handler(
        &mut self,
        click: &LinkClick,
        host: &Host,
        target: Option<&ConnectTarget>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(url) = &click.handler_url else {
            return false;
        };
        if self
            .config
            .link_handlers
            .matching(host.endpoint(), url)
            .is_none()
            && !self
                .config
                .link_plugins
                .iter()
                .any(|plugin| plugin.enabled && plugin.host == host.endpoint())
        {
            return false;
        }
        if self.links.core_handler.is_some() {
            return true;
        }
        let current = self.core_terminals.link_target(host);
        let saved_matches = match host {
            Host::Local => true,
            Host::Remote(scope) => self.endpoints.iter().any(|endpoint| {
                endpoint.enabled
                    && crate::browser::scope(endpoint) == *scope
                    && Some(&endpoint.connection.target) == target
            }),
        };
        let Some(target) = target
            .filter(|target| Some(*target) == current && saved_matches)
            .cloned()
        else {
            self.local_error = Some(link_handlers::Error::HostChanged.localize());
            cx.notify();
            return true;
        };
        let cancelled = Arc::new(AtomicBool::new(false));
        let token = cancelled.clone();
        let deadline = cancelled.clone();
        let timeout = cx.spawn(async move |_, cx| {
            cx.background_executor()
                .timer(Duration::from_secs(30))
                .await;
            deadline.store(true, Ordering::Release);
        });
        self.links.core_handler = Some(Running {
            token: cancelled.clone(),
            _timeout: timeout,
        });
        let click = click.clone();
        let asked = click.clone();
        let handlers = self.config.link_handlers.clone();
        let plugins = self.config.link_plugins.clone();
        let host = host.clone();
        let host_name = host.endpoint().to_owned();
        let route = target.clone();
        let context = self.browser_key();
        let job = cx.background_executor().spawn(async move {
            link_handlers::activate(&handlers, &plugins, &host_name, &target, &asked, &cancelled)
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = job.await;
            let _ = this.update_in(cx, |this, window, cx| {
                if !this
                    .links
                    .core_handler
                    .as_ref()
                    .is_some_and(|running| Arc::ptr_eq(&running.token, &token))
                {
                    return;
                }
                this.links.core_handler = None;
                match result {
                    Ok(false)
                        if this.browser_key() == context
                            && this.core_terminals.link_target(&host) == Some(&route)
                            && match &host {
                                Host::Local => true,
                                Host::Remote(scope) => this.endpoints.iter().any(|endpoint| {
                                    endpoint.enabled
                                        && crate::browser::scope(endpoint) == *scope
                                        && endpoint.connection.target == route
                                }),
                            } =>
                    {
                        this.open_core_fallback(click, &host, window, cx);
                    }
                    Err(error) => {
                        this.local_error =
                            Some(fl!("window-input-link-failed", error = error.localize()));
                        cx.notify();
                    }
                    _ => {}
                }
            });
        })
        .detach();
        true
    }
}
