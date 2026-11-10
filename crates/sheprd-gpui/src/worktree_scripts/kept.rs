//! Worktree scripts of a workspace the app keeps for Herdr 2. herdr-core has
//! no tab to type into first, so a trusted script starts as the program of
//! a new herdr-core tab of the kept workspace, with the same variables
//! Herdr 1's tab gets (see [`super::launch`]). Herdr 1's archive line removes
//! its checkout through `herdr`; here the tab's program is the archive
//! script alone, and the app removes the checkout through
//! `sheprd_remote::work` once that program ends successfully.

use super::{Launch, ScriptKind, launch};
use crate::{
    SheprdWindow,
    i18n::{Localize, fl},
};
use gpui::Context;
use sheprd_backend::{Spawn, TerminalId};
use std::{
    collections::{BTreeMap, VecDeque},
    path::Path,
    time::{Duration, Instant},
};

/// Labels the herdr-core terminal an archive script runs in with its kept
/// workspace, so the app finds it as soon as herdr-core reports it.
const ARCHIVE_LABEL: &str = "sheprd.worktree_archive";
/// How long a new checkout's setup waits for its kept workspace, which an
/// agent landing opens only once its agent started.
const SETUP_WAIT: Duration = Duration::from_secs(120);
/// How long an archive script's terminal may take to be reported started.
const ARCHIVE_START_WAIT: Duration = Duration::from_secs(60);
/// Archives followed and setups waiting at once; one more is refused, or
/// the oldest setup let go, so the lists stay bounded.
const MAX_FOLLOWED: usize = 16;

/// What the window follows of kept workspaces' scripts.
#[derive(Debug, Default)]
pub(crate) struct Kept {
    /// Every archive running, one per kept workspace.
    archives: Vec<Archive>,
    /// New checkouts' setups, oldest first, each waiting for its workspace.
    setups: VecDeque<(Launch, Instant)>,
}

/// An archive script running in a kept workspace's tab; its checkout is
/// removed once the script succeeds, and stays when it fails.
#[derive(Debug)]
struct Archive {
    /// This attempt's own label value, so a retry never takes the terminal
    /// of an earlier attempt, and its exit, for its own.
    attempt: String,
    workspace: String,
    path: String,
    force: bool,
    started: Instant,
    terminal: Option<TerminalId>,
    /// Whether a tab of this computer was seen showing the terminal; one
    /// that stops showing it was closed, which cancels the attempt.
    shown: bool,
}

/// What became of a followed archive this tick.
enum Ended {
    /// Its terminal never showed up, or its workspace closed: let it go.
    Gone,
    Succeeded,
    Failed,
}

impl Launch {
    /// Whether the workspace the script runs for is one the app keeps.
    pub(crate) fn kept(&self) -> bool {
        crate::browser::is_kept_id(&self.workspace)
    }
}

impl SheprdWindow {
    /// Starts a trusted script as the program of a new tab of its kept
    /// workspace. An archive is followed until it ends; a second archive of
    /// a workspace whose first still runs is refused, as is one beyond
    /// [`MAX_FOLLOWED`], so no checkout's removal is ever forgotten.
    pub(super) fn open_kept_script_tab(
        &mut self,
        launch: &Launch,
        script: &str,
        cx: &mut Context<Self>,
    ) -> crate::Result<()> {
        let checkout = launch.checkout.as_ref().ok_or(super::Error::Checkout)?;
        let archive = launch.kind == ScriptKind::Archive;
        if archive {
            let followed = &self.kept_scripts.archives;
            if followed.len() >= MAX_FOLLOWED
                || followed
                    .iter()
                    .any(|known| known.workspace == launch.workspace)
            {
                return Err(super::Error::Busy.into());
            }
        }
        let attempt = uuid::Uuid::new_v4().simple().to_string();
        let mut labels = BTreeMap::new();
        if archive {
            labels.insert(ARCHIVE_LABEL.to_owned(), attempt.clone());
        }
        let spawn = Spawn {
            argv: launch::core_argv(launch.kind),
            cwd: Some(checkout.path.clone().into()),
            env: launch::env(checkout, script),
            labels,
            name: Some(launch.kind.name().to_owned()),
        };
        self.start_in_kept_workspace(&launch.workspace, spawn, cx)?;
        if archive {
            self.kept_scripts.archives.push(Archive {
                attempt,
                workspace: launch.workspace.clone(),
                path: checkout.path.clone(),
                force: launch.force,
                started: Instant::now(),
                terminal: None,
                shown: false,
            });
        }
        Ok(())
    }

    /// Runs `launch`, a new checkout's setup, once the kept workspace for
    /// its folder has opened, after any setup queued before it.
    pub(crate) fn queue_kept_setup(&mut self, launch: Launch) {
        let setups = &mut self.kept_scripts.setups;
        if setups.len() >= MAX_FOLLOWED {
            setups.pop_front();
        }
        setups.push_back((launch, Instant::now()));
    }

    /// Starts a queued setup once its workspace is kept, and removes an
    /// archived checkout once its script ended successfully.
    pub(crate) fn poll_kept_scripts(&mut self, cx: &mut Context<Self>) {
        self.poll_kept_setups(cx);
        self.poll_kept_archives(cx);
    }

    /// The workspace this computer keeps at `path`, whichever host is shown.
    fn local_kept_at(&self, path: &str, cx: &Context<Self>) -> Option<String> {
        let scope = self.local_kept_scope()?;
        self.kept_workspace_in(&scope, Path::new(path), cx)
    }

    /// Starts the first queued setup whose workspace has opened, while this
    /// computer is shown and no other script holds the one slot; setups
    /// whose workspace never opened are let go.
    fn poll_kept_setups(&mut self, cx: &mut Context<Self>) {
        let setups = std::mem::take(&mut self.kept_scripts.setups);
        let mut waiting = VecDeque::with_capacity(setups.len());
        let mut ready = None;
        for (launch, queued) in setups {
            let workspace = launch
                .checkout
                .as_ref()
                .and_then(|checkout| self.local_kept_at(&checkout.path, cx));
            match workspace {
                Some(workspace) if ready.is_none() => ready = Some((launch, queued, workspace)),
                Some(_) => waiting.push_back((launch, queued)),
                // The agent it lands with did not start.
                None if queued.elapsed() > SETUP_WAIT => {}
                None => waiting.push_back((launch, queued)),
            }
        }
        self.kept_scripts.setups = waiting;
        let Some((mut launch, queued, workspace)) = ready else {
            return;
        };
        // Scripts run in the shown host's tabs, one at a time; this one
        // keeps its place at the front until both allow it.
        if self.selected_is_remote() || self.script_slot_busy() {
            self.kept_scripts.setups.push_front((launch, queued));
            return;
        }
        // Queued under the selection of its time: it runs on this
        // computer as it is shown now, which may have been selected again.
        launch.workspace = workspace;
        launch.endpoint = (
            self.selection_epoch,
            self.endpoints[self.selected_endpoint].generation,
        );
        launch
            .endpoint_id
            .clone_from(&self.endpoints[self.selected_endpoint].id);
        if let Err(error) = self.start_worktree_script(launch, cx) {
            self.local_error = Some(fl!(
                "workspace-dialog-setup-failed",
                error = error.localize()
            ));
            cx.notify();
        }
    }

    /// Follows every running archive: removes the checkout of each that
    /// succeeded, and says when one failed.
    pub(crate) fn poll_kept_archives(&mut self, cx: &mut Context<Self>) {
        let mut archives = std::mem::take(&mut self.kept_scripts.archives);
        let mut ended = Vec::new();
        archives.retain_mut(|archive| match self.kept_archive_state(archive, cx) {
            Some(end) => {
                ended.push((
                    archive.workspace.clone(),
                    archive.path.clone(),
                    archive.force,
                    end,
                ));
                false
            }
            None => true,
        });
        self.kept_scripts.archives = archives;
        for (workspace, path, force, end) in ended {
            match end {
                Ended::Gone => {}
                Ended::Succeeded => self.remove_kept_checkout(workspace, path, force, cx),
                Ended::Failed => {
                    // The tab stays on the script's output, and the checkout
                    // with it.
                    self.local_error = Some(fl!("worktree-scripts-archive-failed"));
                    cx.notify();
                }
            }
        }
    }

    /// How `archive` ended, `None` while it runs.
    fn kept_archive_state(&self, archive: &mut Archive, cx: &Context<Self>) -> Option<Ended> {
        if archive.terminal.is_none() {
            archive.terminal = self.local_terminal_labelled(ARCHIVE_LABEL, &archive.attempt);
        }
        let Some(terminal) = &archive.terminal else {
            return (archive.started.elapsed() > ARCHIVE_START_WAIT).then_some(Ended::Gone);
        };
        // A tab closed on the script cancels it: nothing is removed, and
        // the next archive of the workspace may start. One never shown
        // within the start wait was not opened at all.
        let shown = self.local_terminal_shown(terminal);
        if shown {
            archive.shown = true;
        } else if archive.shown || archive.started.elapsed() > ARCHIVE_START_WAIT {
            return Some(Ended::Gone);
        }
        // Closed with its workspace: nothing is left to remove for.
        let kept = self.local_kept_scope().is_some_and(|scope| {
            cx.try_global::<crate::browser::Store>()
                .is_some_and(|store| store.kept_workspace(&scope, &archive.workspace).is_some())
        });
        if !kept {
            return Some(Ended::Gone);
        }
        let exit = self.core_terminal_exit(terminal, cx)?;
        Some(if exit.code == Some(0) {
            Ended::Succeeded
        } else {
            Ended::Failed
        })
    }
}

// For the kept workspaces' tests, which need host scripts.
#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
impl SheprdWindow {
    /// Whether an archive script of `workspace` is being followed.
    pub(crate) fn follows_kept_archive(&self, workspace: &str) -> bool {
        self.kept_scripts
            .archives
            .iter()
            .any(|archive| archive.workspace == workspace)
    }

    pub(crate) fn follow_archive_for_test(&mut self, workspace: String, terminal: TerminalId) {
        self.kept_scripts.archives.push(Archive {
            attempt: "test-attempt".into(),
            workspace,
            path: "/nonexistent/archive-checkout".into(),
            force: false,
            started: Instant::now(),
            terminal: Some(terminal),
            shown: true,
        });
    }

    /// The setup waiting longest for its kept workspace, if any.
    pub(crate) fn queued_kept_setup(&self) -> Option<&Launch> {
        self.kept_scripts.setups.front().map(|(launch, _)| launch)
    }

    /// The attempt label and terminal of the archive of `workspace`.
    pub(crate) fn kept_archive_attempt(
        &self,
        workspace: &str,
    ) -> Option<(String, Option<TerminalId>)> {
        self.kept_scripts
            .archives
            .iter()
            .find(|archive| archive.workspace == workspace)
            .map(|archive| (archive.attempt.clone(), archive.terminal.clone()))
    }

    /// Ages every followed archive past the wait for its tab to show.
    pub(crate) fn age_kept_archives(&mut self) {
        for archive in &mut self.kept_scripts.archives {
            if let Some(started) = Instant::now().checked_sub(ARCHIVE_START_WAIT * 2) {
                archive.started = started;
            }
        }
    }

    /// Every setup waiting for its kept workspace, oldest first.
    pub(crate) fn queued_kept_setups(&self) -> Vec<&Launch> {
        self.kept_scripts
            .setups
            .iter()
            .map(|(launch, _)| launch)
            .collect()
    }
}
