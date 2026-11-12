//! Background clipboard/file preparation, fenced to this view's connection.
//! Input typed during preparation waits behind it; failure discards that input
//! so Enter can never run a command with a missing attachment.

use super::super::files::{self, Prepared, Request};
use super::{CoreTerminalView, ToWindow};
use gpui::{App, ExternalPaths, Keystroke, Window};
use sheprd_backend::ClientPaneInputEvent;
use std::path::PathBuf;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};

pub(super) struct Pending {
    cancel: Arc<AtomicBool>,
    result: mpsc::Receiver<crate::Result<Prepared>>,
    held: Vec<ClientPaneInputEvent>,
    bytes: usize,
    fallback: Option<ClientPaneInputEvent>,
}

impl Drop for Pending {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
    }
}

/// Whether `keystroke` pastes: Cmd-V, Ctrl-Shift-V, or Shift-Insert, and
/// plain Ctrl-V where it pastes (Windows, and Linux, whose desktops send it
/// for a universal paste) while the clipboard holds text.
pub(crate) fn pastes(
    keystroke: &Keystroke,
    ctrl_v_pastes: bool,
    clipboard_has_text: impl FnOnce() -> bool,
) -> bool {
    let modifiers = keystroke.modifiers;
    let v = keystroke.key.eq_ignore_ascii_case("v");
    let insert = keystroke.key == "insert";
    if modifiers.alt || modifiers.function {
        return false;
    }
    if (modifiers.platform && !modifiers.control && v)
        || (modifiers.control && modifiers.shift && (v || insert))
        || (modifiers.shift && !modifiers.control && !modifiers.platform && insert)
    {
        return true;
    }
    let plain_ctrl_v = modifiers.control && !modifiers.shift && !modifiers.platform && v;
    ctrl_v_pastes && plain_ctrl_v && clipboard_has_text()
}

impl CoreTerminalView {
    #[cfg(test)]
    pub(in crate::core_terminals) fn pending_paste_for_test(
        &self,
    ) -> mpsc::SyncSender<crate::Result<Prepared>> {
        let (tx, result) = mpsc::sync_channel(1);
        *self.paste_pending.borrow_mut() = Some(Pending {
            cancel: Arc::new(AtomicBool::new(false)),
            result,
            held: Vec::new(),
            bytes: 0,
            fallback: None,
        });
        tx
    }

    /// Sends the clipboard's copied files' paths, or else its text, as a
    /// paste; false when it holds neither or this view only watches.
    pub(super) fn paste_clipboard(&self, window: &mut Window, cx: &mut App) -> bool {
        self.prepare_paste(Request::Clipboard { image_only: false }, window, cx)
    }

    pub(super) fn paste_control_v(
        &self,
        fallback: Option<ClientPaneInputEvent>,
        window: &mut Window,
        cx: &mut App,
    ) {
        if self.prepare_paste(
            Request::Clipboard {
                image_only: !cfg!(any(windows, target_os = "linux")),
            },
            window,
            cx,
        ) && let Some(pending) = self.paste_pending.borrow_mut().as_mut()
        {
            pending.fallback = fallback;
        }
    }

    /// Pastes the paths of files dropped on the pane, which takes the
    /// keyboard as the window does for a dropped-on Herdr pane.
    pub(crate) fn drop_files(&mut self, paths: &ExternalPaths, window: &mut Window, cx: &mut App) {
        if self.observe || paths.paths().is_empty() {
            return;
        }
        cx.stop_propagation();
        if self.paste_paths(paths.paths(), window, cx) {
            window.focus(&self.focus, cx);
        }
    }

    /// Pastes `paths` quoted for a shell; a drop the quoting refuses (too
    /// many paths, control characters) is said, never pasted in part.
    fn paste_paths(&self, paths: &[PathBuf], window: &mut Window, cx: &mut App) -> bool {
        let quoted = crate::window::file_drop::quote_paths(paths);
        match quoted {
            Ok(text) => {
                if self.remote || crate::window::image_source::temporary_drag_image(paths).is_some()
                {
                    return self.prepare_paste(Request::Paths(paths.to_vec()), window, cx);
                }
                self.send(ClientPaneInputEvent::Paste(text));
                true
            }
            Err(error) => {
                if let Some(to_window) = &self.to_window {
                    to_window(ToWindow::Refused(error), window, cx);
                }
                false
            }
        }
    }

    pub(crate) fn set_paste_target(&mut self, target: Option<sheprd_client::ConnectTarget>) {
        if self.paste_target != target {
            self.paste_pending.borrow_mut().take();
            self.paste_target = target;
        }
    }

    fn prepare_paste(&self, request: Request, window: &mut Window, cx: &mut App) -> bool {
        if self.observe || self.stale || self.ended() {
            return false;
        }
        let target = self
            .paste_target
            .as_ref()
            .filter(|target| target.is_remote() == self.remote);
        let error = if target.is_none() {
            Some(crate::window::file_drop::Error::OtherHost.into())
        } else if self.paste_pending.borrow().is_some() {
            Some(crate::Error::ConnectionBusy)
        } else {
            None
        };
        if let Some(error) = error {
            self.refuse_paste(error, window, cx);
            return false;
        }
        let Some(target) = target.cloned() else {
            return false;
        };
        let (tx, result) = mpsc::sync_channel(1);
        let cancel = Arc::new(AtomicBool::new(false));
        *self.paste_pending.borrow_mut() = Some(Pending {
            cancel: cancel.clone(),
            result,
            held: Vec::new(),
            bytes: 0,
            fallback: None,
        });
        cx.background_executor()
            .spawn(async move {
                let _ = tx.send(files::prepare(&target, request, &cancel));
            })
            .detach();
        true
    }

    fn refuse_paste(&self, error: crate::Error, window: &mut Window, cx: &mut App) {
        if let Some(to_window) = &self.to_window {
            to_window(ToWindow::Refused(error), window, cx);
        }
    }

    pub(super) fn hold_paste_input(&self, input: &ClientPaneInputEvent) -> bool {
        let mut pending = self.paste_pending.borrow_mut();
        let Some(pending) = pending.as_mut() else {
            return false;
        };
        let bytes = match input {
            ClientPaneInputEvent::Paste(text) | ClientPaneInputEvent::TextCommit(text) => {
                text.len()
            }
            ClientPaneInputEvent::Key { generated_text, .. } => {
                generated_text.as_ref().map_or(0, String::len)
            }
            _ => 0,
        };
        if pending.held.len() >= 256 || pending.bytes.saturating_add(bytes) > 2 * 1024 * 1024 {
            pending.cancel.store(true, Ordering::Release);
        } else if !pending.cancel.load(Ordering::Acquire) {
            pending.bytes += bytes;
            pending.held.push(input.clone());
        }
        true
    }

    pub(crate) fn take_paste_error(&mut self) -> Option<crate::Error> {
        self.paste_error.take()
    }

    pub(super) fn cancel_paste(&self) -> bool {
        if let Some(pending) = self.paste_pending.borrow().as_ref() {
            pending.cancel.store(true, Ordering::Release);
            true
        } else {
            false
        }
    }

    pub(super) fn poll_paste(&mut self, _: &mut Window, _: &mut App) {
        let result = {
            let pending = self.paste_pending.borrow();
            let Some(pending) = pending.as_ref() else {
                return;
            };
            if pending.cancel.load(Ordering::Acquire) || self.stale || self.ended() {
                Some(Err(sheprd_client::Error::UploadCancelled.into()))
            } else {
                match pending.result.try_recv() {
                    Ok(result) => Some(result),
                    Err(mpsc::TryRecvError::Empty) => None,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        Some(Err(crate::Error::ConnectionNotReady))
                    }
                }
            }
        };
        let Some(result) = result else {
            return;
        };
        let Some(mut pending) = self.paste_pending.borrow_mut().take() else {
            return;
        };
        match result {
            Ok(mut prepared) => {
                if prepared.text.is_empty()
                    && let Some(fallback) = pending.fallback.take()
                    && !self.link.queue_input(fallback)
                {
                    self.paste_error = Some(crate::Error::ConnectionNotReady);
                    return;
                }
                if !prepared.text.is_empty()
                    && !self
                        .link
                        .queue_input(ClientPaneInputEvent::Paste(prepared.text.clone()))
                {
                    self.paste_error = Some(crate::Error::ConnectionNotReady);
                    return;
                }
                prepared.accepted();
                for input in pending.held.drain(..) {
                    if !self.link.queue_input(input) {
                        self.paste_error = Some(crate::Error::ConnectionNotReady);
                        break;
                    }
                }
            }
            Err(error) => self.paste_error = Some(error),
        }
    }
}
