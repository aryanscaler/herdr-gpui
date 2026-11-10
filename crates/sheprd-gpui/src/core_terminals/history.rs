//! History commands on the herdr-core pane holding the keyboard: find,
//! find again, clear, and editing the history in `$EDITOR`. A Herdr 1 pane
//! asks its daemon for these; a herdr-core view answers them from its own
//! emulator, so they work whatever Herdr 1 is installed or supports.

use super::{CoreTerminalView, Error, Host, Purpose, fallback::Fallback, panes::Axis};
use crate::{SheprdWindow, browser::Scope, controls::Command, i18n::fl, window::Flash};
use gpui::{prelude::*, *};
use sheprd_backend::{Spawn, TerminalId};
use std::path::Path;

/// A tab's pane, by its tab and terminal.
type Pane = (crate::browser::TabId, TerminalId);

impl super::CoreTerminals {
    /// Whether the window drew `view` in its last frame: a pane of a tab
    /// a group shows, or the stand-in shell.
    pub(crate) fn shows(&self, view: &Entity<CoreTerminalView>) -> bool {
        self.drawn.contains(&view.entity_id())
    }

    /// Starts a frame: the views it draws are marked as it draws them.
    pub(crate) fn start_frame(&mut self) {
        self.drawn.clear();
    }

    /// Marks `view` as drawn in this frame.
    pub(crate) fn draw(&mut self, view: &Entity<CoreTerminalView>) {
        self.drawn.push(view.entity_id());
    }

    /// The view holding the keyboard, a tab's pane or the stand-in shell,
    /// even while its own find bar has it.
    pub(crate) fn focused_view(
        &self,
        window: &Window,
        cx: &App,
    ) -> Option<Entity<CoreTerminalView>> {
        self.shown()
            .find(|view| view.read(cx).focus_handle().contains_focused(window, cx))
            .cloned()
    }
}

impl SheprdWindow {
    /// Runs `command` on the herdr-core view holding the keyboard. False
    /// when none does or the command is not a history command, so the
    /// window runs it as usual.
    pub(crate) fn core_history_command(
        &mut self,
        command: Command,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        // As pane commands do, one with the keyboard elsewhere goes to the
        // pane a kept workspace in front shows.
        let view = self.core_terminals.focused_view(window, cx).or_else(|| {
            let pane = self.kept_front_pane(cx)?;
            self.core_terminals.views.get(&pane).cloned()
        });
        let Some(view) = view else {
            return false;
        };
        match command {
            Command::Find => view.update(cx, |view, cx| view.open_find(window, cx)),
            Command::FindNext | Command::FindPrevious => view.update(cx, |view, cx| {
                view.find_again(command == Command::FindPrevious, window, cx)
            }),
            Command::ClearPane => view.update(cx, |view, cx| view.clear_history(cx)),
            Command::CopyMode => view.update(cx, |view, cx| view.enter_copy_mode(window, cx)),
            Command::EditScrollback => view.update(cx, |view, _| view.request_edit()),
            _ => return false,
        }
        true
    }

    /// Opens the history text a view read in `$EDITOR`, once it arrives:
    /// saved to a private file off the UI thread, then edited in a pane
    /// beside the view that closes when the editor quits, which also
    /// removes the file.
    pub(super) fn poll_core_edits(&mut self, cx: &mut Context<Self>) {
        let mut views: Vec<(Option<Pane>, Entity<CoreTerminalView>)> = self
            .core_terminals
            .views
            .iter()
            .map(|(key, view)| (Some(key.clone()), view.clone()))
            .collect();
        if let Fallback::Shown(view) = &self.core_terminals.fallback {
            views.push((None, view.clone()));
        }
        for (pane, view) in views {
            let Some(text) = view.update(cx, |view, _| view.take_edit()) else {
                continue;
            };
            match text {
                Ok(text) => self.edit_core_history(pane, text, cx),
                Err(error) => self.core_edit_failed(&error, cx),
            }
        }
    }

    pub(crate) fn edit_core_history(
        &mut self,
        pane: Option<Pane>,
        text: String,
        cx: &mut Context<Self>,
    ) {
        let ended = pane.as_ref().is_some_and(|key| {
            self.core_terminals
                .views
                .get(key)
                .is_some_and(|view| view.read(cx).ended())
        });
        let workspace = pane.as_ref().and_then(|(tab, _)| {
            let tab = cx.try_global::<crate::browser::Store>()?.get(*tab)?;
            Some((tab.scope.clone(), tab.workspace_id.clone()))
        });
        let Some(purpose) = editor_purpose(pane, ended, workspace, self.browser_key()) else {
            self.show_flash(Flash::warning(fl!("core-terminals-open-workspace")), cx);
            return;
        };
        // The history is saved on this computer, where a remote core's
        // editor could not open it.
        if self.purpose_host(&purpose) != Host::Local {
            self.show_flash(Flash::warning(fl!("core-terminals-edit-remote")), cx);
            return;
        }
        let saved = cx.background_spawn(async move { save_history(&text) });
        // A window closed meanwhile drops the file with this task.
        cx.spawn(async move |this, cx| {
            let saved = saved.await;
            let _ = this.update(cx, |this, cx| match saved {
                Ok(file) => {
                    let spawn = editor_spawn(&file);
                    this.start_core_program_with(purpose, spawn, Some(file), cx);
                }
                Err(error) => this.core_edit_failed(&error, cx),
            });
        })
        .detach();
    }

    fn core_edit_failed(&mut self, error: &Error, cx: &mut Context<Self>) {
        tracing::warn!(%error, "Could not edit a herdr-core terminal's history");
        self.show_flash(Flash::warning(fl!("core-terminals-edit-failed")), cx);
    }
}

/// Where the editor for a view's history opens: beside `pane`, or in a new
/// tab of the workspace `workspace` holds `pane`'s tab in. The stand-in
/// shell has no tab to split, and an `ended` pane would be closed out of a
/// split the editor made, taking its history with it, so both get a tab of
/// their own: a pane's in its own workspace, wherever the window has moved
/// since, and the stand-in shell's in the `current` one.
pub(super) fn editor_purpose(
    pane: Option<Pane>,
    ended: bool,
    workspace: Option<(Scope, String)>,
    current: Option<(Scope, String)>,
) -> Option<Purpose> {
    let key = match pane {
        Some((tab, at)) if !ended => {
            return Some(Purpose::Split {
                tab,
                at,
                axis: Axis::Row,
            });
        }
        Some(_) => workspace,
        None => current,
    };
    key.map(|(scope, workspace)| Purpose::Tab { scope, workspace })
}

/// Saves `text` to a file only the user can read. The file is removed when
/// the returned path drops, until the editor's start hands it over.
fn save_history(text: &str) -> Result<tempfile::TempPath, Error> {
    use std::io::Write as _;
    let mut file = tempfile::Builder::new()
        .prefix("sheprd-scrollback-")
        .suffix(".txt")
        .tempfile()
        .map_err(Error::EditFile)?;
    file.write_all(text.as_bytes()).map_err(Error::EditFile)?;
    file.flush().map_err(Error::EditFile)?;
    Ok(file.into_temp_path())
}

/// The editor herdr-core runs on `path`: `$VISUAL`, else `$EDITOR`, else
/// `vi` (Notepad on Windows), through a shell so an editor with arguments
/// such as `code --wait` works, removing the file once the editor quits.
/// The app's own editor settings go along, as a core started elsewhere may
/// not have them.
pub(super) fn editor_spawn(path: &Path) -> Spawn {
    let path = path.display().to_string();
    #[cfg(not(windows))]
    let argv = vec![
        "/bin/sh".to_owned(),
        "-c".to_owned(),
        r#"eval "${VISUAL:-${EDITOR:-vi}} \"\$1\""; status=$?; rm -f "$1"; exit $status"#
            .to_owned(),
        "sheprd-scrollback".to_owned(),
        path,
    ];
    #[cfg(windows)]
    let argv = {
        let editor = ["VISUAL", "EDITOR"]
            .into_iter()
            .filter_map(|name| std::env::var(name).ok())
            .find(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "notepad.exe".to_owned());
        vec![
            "cmd.exe".to_owned(),
            "/D".to_owned(),
            "/C".to_owned(),
            format!("{editor} \"{path}\" & del \"{path}\""),
        ]
    };
    let env = ["VISUAL", "EDITOR"]
        .into_iter()
        .filter_map(|name| Some((name.to_owned(), std::env::var(name).ok()?)))
        .filter(|(_, value)| !value.trim().is_empty())
        .collect();
    Spawn {
        argv,
        env,
        ..Spawn::default()
    }
}
