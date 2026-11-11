//! Lets processes on this machine, such as an agent in a Herdr pane, drive
//! the parts of the app Herdr cannot: show the user a page and hear back
//! what the user noted on it, open the app's own tabs, and read or change
//! state only this client keeps. Herdr itself has no way to reach a client,
//! so the app listens on a socket of its own and `sheprd <command> ...`
//! talks to it. Only this user's local processes can reach it: an agent on
//! an SSH host has no path to this machine's socket.

mod client;
mod server;

use crate::browser::Location;
pub(crate) use crate::cli::control::Placement;
use crate::window::main_window::MainWindow;
#[cfg(windows)]
pub(crate) use client::send_focus;
#[cfg(feature = "mockup")]
pub(crate) use client::send_notes;
pub(crate) use client::{run, send_link, status as exit_status};
use gpui::App;
use protocol::Caller;
pub use protocol::ErrorCode;
pub(crate) use protocol::NotesTo;
pub(crate) use protocol::Response;
pub(crate) use protocol::cloud as batch;
pub use protocol::{
    app::{FontSize, Size, Switch},
    events::{Cursor, EventKind},
    settings::Sound,
    tabs::ZoomChange,
    work::Script,
};
pub(crate) use sheprd_control_protocol::Error;
use sheprd_control_protocol::{protocol, socket};
use std::path::{Path, PathBuf};

#[cfg(any(target_os = "macos", windows))]
/// The command that reloads a caller's pages, for the notes prompt.
pub(crate) fn reload_command() -> String {
    static COMMAND: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    COMMAND
        .get_or_init(|| {
            format!(
                "{} browser reload",
                crate::agent_skill::command(crate::agent_skill::executable().as_deref())
            )
        })
        .clone()
}

/// Where this user's app listens: a socket in its state directory, or a
/// named pipe on Windows.
fn socket_path() -> crate::Result<PathBuf> {
    let state = crate::preferences::state_dir().ok_or(crate::Error::MissingStateRoot)?;
    Ok(socket::address(&state)?)
}

/// What a request asks the windows to find: a workspace, optionally of one
/// particular local daemon, and the pane asking.
pub(crate) struct Target<'a> {
    pub daemon: Option<&'a Path>,
    pub workspace: Option<&'a str>,
    pub pane: Option<&'a str>,
    /// The herdr-core terminal the caller runs in, by its address.
    pub terminal: Option<&'a str>,
}

impl Target<'_> {
    /// Who the caller is, as a page it opens remembers it and its notes go
    /// back to: its Herdr pane, or its herdr-core terminal by the key chats
    /// use for one.
    pub(crate) fn origin(&self) -> Option<std::borrow::Cow<'_, str>> {
        origin(self.pane, self.terminal)
    }

    /// How strictly a request's workspace is matched, strict first. A caller
    /// that names its daemon is matched on that daemon only: another
    /// session's workspace of the same ID must never be changed for it.
    pub(crate) fn strictness(&self) -> &'static [bool] {
        if self.daemon.is_some() {
            &[true]
        } else {
            &[true, false]
        }
    }
}

/// The page a request asks a window to open, and how to show it.
pub(crate) struct Opening<'a> {
    pub location: &'a Location,
    /// Whether the window switches to the tab.
    pub focus: bool,
    pub placement: Placement,
}

/// Where a window opened a requested tab.
pub(crate) enum Placed {
    Opened {
        workspace_id: String,
        placement: Placement,
    },
    Full,
}

/// Identifiers a request carries are compared, never interpreted, but they
/// are echoed in answers, so they stay short and printable.
fn plain(value: &str, limit: usize) -> bool {
    !value.is_empty()
        && value.len() <= limit
        && value.chars().all(|c| !c.is_control() && !c.is_whitespace())
}

/// A daemon socket path: compared and echoed like an identifier, but it may
/// hold spaces, as a Windows profile such as `C:\Users\Jane Doe` does.
fn plain_path(value: &str) -> bool {
    !value.is_empty() && value.len() <= 4096 && !value.chars().any(char::is_control)
}

fn valid_caller(caller: &Caller) -> bool {
    caller
        .workspace_id
        .as_deref()
        .is_none_or(|id| plain(id, 256))
        && caller.pane_id.as_deref().is_none_or(|id| plain(id, 256))
        && caller
            .terminal_id
            .as_deref()
            .is_none_or(|id| sheprd_backend::TerminalId::new(id).is_ok() && plain(id, 256))
        && caller.daemon_socket.as_deref().is_none_or(plain_path)
}

#[cfg(test)]
pub(crate) fn valid_caller_for_test(caller: &Caller) -> bool {
    valid_caller(caller)
}

/// A caller's pane, or its herdr-core terminal as `core:t7`, the key a chat
/// in that terminal has; Herdr pane IDs never take that form.
pub(crate) fn origin<'a>(
    pane: Option<&'a str>,
    terminal: Option<&str>,
) -> Option<std::borrow::Cow<'a, str>> {
    match (pane, terminal) {
        (Some(pane), _) => Some(std::borrow::Cow::Borrowed(pane)),
        (None, Some(terminal)) => {
            let terminal = sheprd_backend::TerminalId::new(terminal).ok()?;
            Some(std::borrow::Cow::Owned(crate::chat::core_pane::key(
                &terminal,
            )))
        }
        (None, None) => None,
    }
}

fn main_windows(cx: &App) -> Vec<MainWindow> {
    let active = cx.active_window();
    let mut windows = MainWindow::all(cx);
    // The frontmost window wins a workspace that several windows show.
    windows.sort_by_key(|handle| Some(handle.window_id()) != active.map(|a| a.window_id()));
    windows
}

pub(crate) use server::install;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn echoed_identifiers_are_short_and_printable() {
        assert!(plain("w_1", 256));
        for invalid in ["", "w 1", "w\u{1b}[2J", "w\n1"] {
            assert!(!plain(invalid, 256), "{invalid:?}");
        }
        assert!(!plain(&"w".repeat(257), 256));
    }

    #[test]
    fn daemon_paths_may_hold_spaces_but_not_control_characters() {
        assert!(plain_path(
            r"C:\Users\Jane Doe\AppData\Roaming\herdr\herdr-client.sock"
        ));
        assert!(plain_path("/home/jane/.config/herdr/herdr.sock"));
        for invalid in ["", "/tmp/a\nb.sock", "/tmp/\u{1b}[2J"] {
            assert!(!plain_path(invalid), "{invalid:?}");
        }
        assert!(!plain_path(&"a".repeat(4097)));
    }
}
