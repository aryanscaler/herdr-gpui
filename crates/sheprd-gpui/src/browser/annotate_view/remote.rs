//! Annotation artifacts copied to the exact remote core caller before its
//! feedback is queued. A connection replacement invalidates an in-flight copy.

use crate::{
    SheprdWindow,
    browser::{FeedbackKey, Tab, annotate},
    core_terminals::files,
    i18n::fl,
    window::Flash,
};
use gpui::{Context, Image};
use sheprd_client::ConnectTarget;
use sheprd_core_client::CoreBackend;
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

const MAX_SCREENSHOT_BYTES: usize = 16 * 1024 * 1024;

pub(super) struct Caller {
    feedback: FeedbackKey,
    host: ConnectTarget,
    backend: CoreBackend,
}

impl Caller {
    pub(super) fn of(view: &SheprdWindow, tab: &Tab) -> Option<Self> {
        let origin = tab.origin.as_ref()?;
        let backend = view
            .core_terminals
            .browser_backend(&tab.scope, origin)?
            .clone();
        let host = view
            .endpoints
            .iter()
            .find(|endpoint| crate::browser::scope(endpoint) == tab.scope)?
            .saved_target()
            .clone();
        host.is_remote().then(|| Self {
            feedback: FeedbackKey {
                scope: tab.scope.clone(),
                pane_id: origin.clone(),
            },
            host,
            backend,
        })
    }

    fn current(&self, view: &SheprdWindow) -> bool {
        view.endpoints.iter().any(|endpoint| {
            crate::browser::scope(endpoint) == self.feedback.scope
                && endpoint.saved_target() == &self.host
        }) && view
            .core_terminals
            .browser_backend(&self.feedback.scope, &self.feedback.pane_id)
            .is_some_and(|backend| std::ptr::eq(backend.core(), self.backend.core()))
    }
}

struct Cancel(Arc<AtomicBool>);
impl Drop for Cancel {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

struct Uploaded {
    paths: Vec<Option<PathBuf>>,
    cleanup: Option<(ConnectTarget, Vec<String>)>,
}

impl Drop for Uploaded {
    fn drop(&mut self) {
        if let Some((host, paths)) = self.cleanup.take() {
            let spawned = std::thread::Builder::new()
                .name("browser-image-cleanup".into())
                .spawn(move || {
                    if let Err(error) = files::cleanup(&host, &paths) {
                        tracing::warn!(%error, "Could not remove an unclaimed browser screenshot");
                    }
                });
            if let Err(error) = spawned {
                tracing::warn!(%error, "Could not start browser screenshot cleanup");
            }
        }
    }
}

/// Blocking. The original files stay in the existing age-cleaned private
/// annotation directory, including WSL's mapped files, which are not copies.
fn upload(
    host: &ConnectTarget,
    images: &[Option<Arc<Image>>],
    cancelled: &AtomicBool,
    save: impl FnOnce(&[Option<Arc<Image>>]) -> crate::Result<Vec<Option<PathBuf>>>,
) -> crate::Result<Uploaded> {
    upload_with(host, images, cancelled, save, files::upload)
}

fn upload_with(
    host: &ConnectTarget,
    images: &[Option<Arc<Image>>],
    cancelled: &AtomicBool,
    save: impl FnOnce(&[Option<Arc<Image>>]) -> crate::Result<Vec<Option<PathBuf>>>,
    transfer: impl FnOnce(&ConnectTarget, &[PathBuf], &AtomicBool) -> crate::Result<Vec<String>>,
) -> crate::Result<Uploaded> {
    let size = images
        .iter()
        .flatten()
        .try_fold(0usize, |size, image| size.checked_add(image.bytes.len()));
    if size.is_none_or(|size| size > MAX_SCREENSHOT_BYTES) {
        return Err(crate::Error::ImageInputTooLarge {
            limit: MAX_SCREENSHOT_BYTES,
        });
    }
    if cancelled.load(Ordering::Acquire) {
        return Err(sheprd_client::Error::UploadCancelled.into());
    }
    if images.iter().all(Option::is_none) {
        return Ok(Uploaded {
            paths: vec![None; images.len()],
            cleanup: None,
        });
    }
    let local = save(images)?;
    let files: Vec<PathBuf> = local.iter().flatten().cloned().collect();
    let remote = if files.is_empty() {
        Vec::new()
    } else {
        transfer(host, &files, cancelled)?
    };
    let mut uploaded = Uploaded {
        paths: Vec::new(),
        cleanup: (!remote.is_empty()).then(|| (host.clone(), remote)),
    };
    let paths = uploaded
        .cleanup
        .as_ref()
        .map(|(_, paths)| paths.as_slice())
        .unwrap_or_default();
    let mut paths = paths.iter();
    uploaded.paths = local
        .iter()
        .map(|path| {
            path.as_ref()
                .and_then(|_| paths.next())
                .map(|path| PathBuf::from(sheprd_client::shell_quote(path)))
        })
        .collect();
    if cancelled.load(Ordering::Acquire) {
        return Err(sheprd_client::Error::UploadCancelled.into());
    }
    Ok(uploaded)
}

impl SheprdWindow {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn send_remote_page_notes(
        &mut self,
        caller: Caller,
        tab: Tab,
        notes: Vec<annotate::Note>,
        pending: Arc<()>,
        save: impl FnOnce(&[Option<Arc<Image>>]) -> crate::Result<Vec<Option<PathBuf>>> + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        let images: Vec<_> = notes.iter().map(|note| note.image.clone()).collect();
        let cancelled = Arc::new(AtomicBool::new(false));
        let cancel = Cancel(cancelled.clone());
        let host = caller.host.clone();
        let saving = cx
            .background_executor()
            .spawn(async move { upload(&host, &images, &cancelled, save) });
        cx.spawn(async move |this, cx| {
            let _cancel = cancel;
            let mut uploaded = saving.await;
            this.update(cx, |this, cx| {
                let page_exists = cx.try_global::<crate::browser::Store>()
                    .and_then(|store| store.get(tab.id))
                    .is_some_and(|current| current.scope == tab.scope && current.origin == tab.origin);
                if !page_exists || !caller.current(this) {
                    this.show_flash(Flash::warning(fl!("agent-notes-session-not-shown")), cx);
                    return;
                }
                let failed = uploaded.is_err();
                let paths = match &uploaded {
                    Ok(uploaded) => uploaded.paths.as_slice(),
                    Err(error) => {
                        tracing::warn!(%error, "Could not transfer browser screenshots to the caller");
                        // Text still reaches the exact caller, never with local paths.
                        &[]
                    }
                };
                let text = annotate::prompt(&tab, &notes, paths, &crate::remote_browser::reload_command());
                let delivered = this.deliver_notes(Some(caller.feedback), text, Some(pending), None, cx);
                if !matches!(delivered, crate::agent_notes::Delivered::Copy(_))
                    && let Ok(uploaded) = &mut uploaded
                {
                    uploaded.cleanup = None;
                }
                if failed {
                    this.show_flash(Flash::warning(fl!("browser-notes-screenshots-failed")), cx);
                }
            }).ok();
        }).detach();
    }
}

#[cfg(test)]
mod tests;
