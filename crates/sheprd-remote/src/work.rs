//! Work started on a host: a new worktree of a repository there, and an
//! agent started in it with its first prompt, whichever Herdr runs the
//! host's terminals. Herdr 1's daemon makes the worktree and its workspace
//! through `herdr worktree create`. herdr-core (Herdr 2) keeps no workspaces
//! or worktrees, so Git makes the checkout, in the folder Herdr 1 would have
//! used, the agent starts in a herdr-core shell there, and the app keeps the
//! workspace that shows it. The new worktree dialog, fan-out, smart dispatch,
//! and the orchestrator all start work through here.
//!
//! Every function runs host scripts and blocks: call them from a background
//! worker, never the UI thread.

mod error;

pub use error::{Error, Result, Step};

use crate::teleport::{AgentKind, Envelope, Host, WorktreeCreated};
use error::script;
use serde::Deserialize;
use sheprd_client::{herdr::worktree, shell_quote};
use sheprd_core_client::{
    agent::{READY_TIMEOUT, readiness},
    wire::Waited,
};
use std::{
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
    time::Instant,
};

/// How long `herdr agent start` may wait for an agent to be ready. Agents
/// starting side by side on one machine are slower than one alone.
const AGENT_START_TIMEOUT_MS: &str = "90000";
/// The longest one `herdr-core wait` is asked to take, so a cancel is seen
/// soon and a remote script never sits quiet for long.
const WAIT_CHECK_IN_SECS: u64 = 5;
/// The most worktrees a listing keeps; a repository has a handful.
const MAX_LISTED: usize = 512;

/// Which Herdr makes a host's worktrees, and through what.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Repository {
    /// Herdr 1's daemon, through `workspace_id`, a workspace of the
    /// repository.
    Herdr1 { workspace_id: String },
    /// Git, for herdr-core, through `checkout`: any checkout of the
    /// repository on the host.
    Herdr2 { checkout: String },
}

/// What [`create`] makes.
#[derive(Clone, Copy, Debug)]
pub struct NewWorktree<'a> {
    pub branch: &'a str,
    /// What a new branch starts from: a commit, a branch, or `HEAD`. A
    /// branch that already exists is checked out as it stands.
    pub base: &'a str,
    /// Herdr 1's name for the new workspace; the app names a kept one.
    pub label: Option<&'a str>,
}

/// A worktree [`create`] made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Worktree {
    pub path: String,
    pub opened: Opened,
}

/// What opened a new worktree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Opened {
    /// Herdr 1's new workspace, and the pane its first tab holds.
    Herdr1 {
        workspace_id: String,
        pane_id: String,
    },
    /// Nothing yet: herdr-core keeps no workspaces, the app keeps one.
    Herdr2,
}

/// An agent to start in a new worktree.
#[derive(Clone, Copy, Debug)]
pub struct Agent<'a> {
    pub kind: AgentKind,
    /// Herdr 1's name for it, unique on its host.
    pub name: &'a str,
    /// Passed to the agent as `--model`; one [`valid_model`] refuses is left
    /// out, so it cannot become another option.
    pub model: Option<&'a str>,
    /// Sent once it says it is ready.
    pub prompt: Option<&'a str>,
}

/// Whether `model` is a name an agent's `--model` could take: no spaces,
/// controls, or leading dash, so it cannot become another option.
pub fn valid_model(model: &str) -> bool {
    !model.is_empty()
        && model.len() <= 128
        && !model.starts_with('-')
        && model
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/' | ':' | '@'))
}

/// Where a started agent runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Started {
    /// Herdr 1's pane.
    Herdr1 { pane_id: String },
    /// The herdr-core terminal, a shell the agent was typed into, so the
    /// shell stays when the agent ends, as in a Herdr 1 pane.
    Herdr2 { terminal: String },
}

/// One checkout of a repository, as `git worktree list` reports it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Checkout {
    pub path: String,
    pub branch: Option<String>,
    pub bare: bool,
    pub detached: bool,
    pub prunable: bool,
}

/// The whole of new work: a worktree on `host`, and `agent` started in it.
/// A worktree whose agent did not start stays, as Herdr 1's does, so the
/// user can see what happened there; the error says which step failed.
pub fn start(
    host: &Host,
    repository: &Repository,
    new: &NewWorktree<'_>,
    agent: &Agent<'_>,
    cancelled: &AtomicBool,
) -> Result<(Worktree, Started)> {
    let worktree = create(host, repository, new, cancelled)?;
    let started = start_agent(host, &worktree, agent, cancelled)?;
    Ok((worktree, started))
}

/// A new worktree of `repository` on `host`.
pub fn create(
    host: &Host,
    repository: &Repository,
    new: &NewWorktree<'_>,
    cancelled: &AtomicBool,
) -> Result<Worktree> {
    match repository {
        Repository::Herdr1 { workspace_id } => create_in_herdr1(
            host,
            workspace_id,
            Some(new.branch),
            new.base,
            new.label,
            cancelled,
        ),
        Repository::Herdr2 { checkout } => {
            let common = host
                .capture(&common_dir_line(checkout), cancelled)
                .map_err(script(Step::CreateWorktree))?;
            let common = String::from_utf8_lossy(&common);
            let folder = worktree::folder(Path::new(common.trim()), new.branch)?;
            let output = host
                .capture(&add_script(checkout, &folder, new), cancelled)
                .map_err(script(Step::CreateWorktree))?;
            let path = String::from_utf8_lossy(&output).trim().to_owned();
            if path.is_empty() {
                return Err(Error::Output {
                    step: Step::CreateWorktree,
                });
            }
            Ok(Worktree {
                path,
                opened: Opened::Herdr2,
            })
        }
    }
}

/// A new worktree through Herdr 1's workspace `workspace_id`, as
/// [`create`] makes one, except that without a `branch` Herdr 1's daemon
/// names it, as smart dispatch leaves it to when the dialog's name is empty.
pub fn create_in_herdr1(
    host: &Host,
    workspace_id: &str,
    branch: Option<&str>,
    base: &str,
    label: Option<&str>,
    cancelled: &AtomicBool,
) -> Result<Worktree> {
    let mut args = vec!["worktree", "create", "--workspace", workspace_id];
    if let Some(branch) = branch {
        args.extend(["--branch", branch]);
    }
    args.extend(["--base", base, "--no-focus"]);
    if let Some(label) = label {
        args.extend(["--label", label]);
    }
    let output = host
        .capture(&Host::herdr_line(&args), cancelled)
        .map_err(script(Step::CreateWorktree))?;
    let created = serde_json::from_slice::<Envelope<WorktreeCreated>>(&output)
        .map_err(|source| Error::Decode {
            step: Step::CreateWorktree,
            source,
        })?
        .result;
    Ok(Worktree {
        path: created.worktree.path,
        opened: Opened::Herdr1 {
            workspace_id: created.workspace.workspace_id,
            pane_id: created.root_pane.pane_id,
        },
    })
}

/// Starts `agent` in `worktree`'s first pane, or a new herdr-core shell in
/// its folder, and sends it its prompt once it is ready. An agent that
/// started but did not take its prompt is [`Error::Unprompted`], which
/// names where it runs so the caller can still show it.
pub fn start_agent(
    host: &Host,
    worktree: &Worktree,
    agent: &Agent<'_>,
    cancelled: &AtomicBool,
) -> Result<Started> {
    let model = agent.model.filter(|model| valid_model(model));
    let started = match &worktree.opened {
        Opened::Herdr1 { pane_id, .. } => {
            let mut args = vec![
                "agent",
                "start",
                agent.name,
                "--kind",
                agent.kind.name(),
                "--pane",
                pane_id,
                "--timeout",
                AGENT_START_TIMEOUT_MS,
            ];
            // Herdr 1 passes what follows `--` to the agent's own command.
            if let Some(model) = model {
                args.extend(["--", "--model", model]);
            }
            host.capture(&Host::herdr_line(&args), cancelled)
                .map_err(script(Step::StartAgent))?;
            Started::Herdr1 {
                pane_id: pane_id.clone(),
            }
        }
        Opened::Herdr2 => {
            let terminal = start_shell(host, &worktree.path, cancelled)?;
            let mut argv = agent.kind.start_argv(None);
            // `--model NAME` goes to every kind, as Herdr 1's `agent start
            // -- --model` passes it; an agent whose command has no such
            // option says so in its shell, which stays open.
            if let Some(model) = model {
                argv.extend(["--model".to_owned(), model.to_owned()]);
            }
            let program = argv
                .iter()
                .map(|arg| shell_quote(arg))
                .collect::<Vec<_>>()
                .join(" ");
            if let Err(error) = send(host, &terminal, &program, Step::StartAgent, cancelled) {
                // No agent ever ran there, and no caller learns its address,
                // so the shell is ended rather than left without a tab.
                end(host, &terminal);
                return Err(error);
            }
            Started::Herdr2 { terminal }
        }
    };
    if let Some(prompt) = agent.prompt
        && let Err(source) = self::prompt(host, &started, prompt, cancelled)
    {
        return Err(Error::Unprompted {
            started,
            source: Box::new(source),
        });
    }
    Ok(started)
}

/// Ends the herdr-core terminal `terminal`, best effort: one already gone
/// is fine.
fn end(host: &Host, terminal: &str) {
    let line = format!(
        "core_cli kill --json -- {} >/dev/null\n",
        shell_quote(terminal)
    );
    if let Err(error) = host.capture(&line, &AtomicBool::new(false)) {
        tracing::warn!(%error, %terminal, "Could not end a herdr-core shell whose agent never started");
    }
}

/// Sends `prompt` to the agent `started` runs, once it says it is ready.
pub fn prompt(host: &Host, started: &Started, prompt: &str, cancelled: &AtomicBool) -> Result<()> {
    match started {
        // The pane is the target: an agent name could match one elsewhere.
        Started::Herdr1 { pane_id } => host
            .capture(
                &Host::herdr_line(&["agent", "prompt", pane_id, &not_an_option(prompt)]),
                cancelled,
            )
            .map(drop)
            .map_err(script(Step::Prompt)),
        Started::Herdr2 { terminal } => {
            ready(host, terminal, cancelled)?;
            send(host, terminal, prompt, Step::Prompt, cancelled)
        }
    }
}

/// Every checkout of the repository `checkout` belongs to, on a host whose
/// worktrees Git keeps for herdr-core.
pub fn list(host: &Host, checkout: &str, cancelled: &AtomicBool) -> Result<Vec<Checkout>> {
    let line = format!(
        "git -C {} worktree list --porcelain -z\n",
        shell_quote(checkout)
    );
    let output = host.capture(&line, cancelled).map_err(script(Step::List))?;
    Ok(parse_list(&String::from_utf8_lossy(&output)))
}

/// Removes `worktree` with whatever opened it, discarding its uncommitted
/// changes and keeping its branch, so committed work stays reachable:
/// Herdr 1's daemon removes its workspace with it, and on Herdr 2 Git
/// removes the checkout, leaving the app to close the workspace it keeps.
pub fn discard(host: &Host, worktree: &Worktree, cancelled: &AtomicBool) -> Result<()> {
    match &worktree.opened {
        Opened::Herdr1 { workspace_id, .. } => host
            .capture(
                &Host::herdr_line(&["worktree", "remove", "--workspace", workspace_id, "--force"]),
                cancelled,
            )
            .map(drop)
            .map_err(script(Step::Remove)),
        Opened::Herdr2 => remove(host, &worktree.path, true, cancelled),
    }
}

/// Removes the linked checkout at `path`, keeping its branch so committed
/// work stays reachable, as Herdr 1 does. Without `force`, a checkout with
/// uncommitted changes stays and the answer is [`Error::Dirty`].
pub fn remove(host: &Host, path: &str, force: bool, cancelled: &AtomicBool) -> Result<()> {
    let output = host
        .capture(&remove_script(path, force), cancelled)
        .map_err(script(Step::Remove))?;
    match String::from_utf8_lossy(&output).trim() {
        "removed" => Ok(()),
        "dirty" => Err(Error::Dirty),
        _ => Err(Error::Output { step: Step::Remove }),
    }
}

/// `text` as Herdr 1's CLI reads a positional argument: it takes no `--`,
/// so text that starts like an option gets a leading space, which an agent
/// reading its prompt ignores.
fn not_an_option(text: &str) -> std::borrow::Cow<'_, str> {
    if text.starts_with('-') {
        format!(" {text}").into()
    } else {
        text.into()
    }
}

fn common_dir_line(checkout: &str) -> String {
    format!(
        "git -C {} rev-parse --path-format=absolute --git-common-dir\n",
        shell_quote(checkout)
    )
}

/// Adds the checkout under the host's own worktree root, from `new.base`
/// unless its branch exists, and prints its folder. Git's own chatter goes
/// to stderr, so only the folder is read.
fn add_script(checkout: &str, folder: &Path, new: &NewWorktree<'_>) -> String {
    let repo = shell_quote(checkout);
    let destination = shell_quote(&Path::new(worktree::ROOT).join(folder).to_string_lossy());
    let branch = shell_quote(new.branch);
    let base = shell_quote(new.base);
    format!(
        r#"dest="$HOME"/{destination}
if git -C {repo} show-ref --verify --quiet refs/heads/{branch}; then
  git -C {repo} worktree add "$dest" --end-of-options {branch} >&2
else
  git -C {repo} worktree add -b {branch} "$dest" --end-of-options {base} >&2
fi
printf '%s\n' "$dest"
"#
    )
}

/// Removes the checkout `path` is in, from its top folder, since a
/// workspace may have started in a subfolder, through its repository,
/// since Git will not remove the checkout it runs in, and says what
/// happened.
fn remove_script(path: &str, force: bool) -> String {
    let path = shell_quote(path);
    let (check, flag) = if force {
        ("", " --force")
    } else {
        (
            "if [ -n \"$(git -C \"$tree\" status --porcelain)\" ]; then echo dirty; exit 0; fi\n",
            "",
        )
    };
    format!(
        r#"tree=$(git -C {path} rev-parse --show-toplevel)
{check}common=$(git -C "$tree" rev-parse --path-format=absolute --git-common-dir)
git --git-dir="$common" worktree remove{flag} "$tree" >&2
echo removed
"#
    )
}

/// `git worktree list --porcelain -z`: one NUL-ended field per line of a
/// record, and an empty field between records.
fn parse_list(output: &str) -> Vec<Checkout> {
    let mut checkouts = Vec::new();
    let mut current: Option<Checkout> = None;
    for field in output.split('\0') {
        if field.is_empty() {
            checkouts.extend(current.take());
            continue;
        }
        let (key, value) = field.split_once(' ').unwrap_or((field, ""));
        match key {
            "worktree" => {
                checkouts.extend(current.take());
                current = Some(Checkout {
                    path: value.to_owned(),
                    ..Checkout::default()
                });
            }
            "branch" => {
                if let Some(checkout) = &mut current {
                    checkout.branch = Some(
                        value
                            .strip_prefix("refs/heads/")
                            .unwrap_or(value)
                            .to_owned(),
                    );
                }
            }
            "bare" | "detached" | "prunable" => {
                if let Some(checkout) = &mut current {
                    match key {
                        "bare" => checkout.bare = true,
                        "detached" => checkout.detached = true,
                        _ => checkout.prunable = true,
                    }
                }
            }
            _ => {}
        }
    }
    checkouts.extend(current);
    checkouts.truncate(MAX_LISTED);
    checkouts
}

/// What `herdr-core run --json` answers.
#[derive(Deserialize)]
struct Ran {
    address: String,
}

/// Starts an idle shell in the host's core, in `cwd`, as the app's own new
/// tab does, and answers its terminal's address.
fn start_shell(host: &Host, cwd: &str, cancelled: &AtomicBool) -> Result<String> {
    let line = format!(
        "core_cli run --bg --json --no-parent --cwd {}\n",
        shell_quote(cwd)
    );
    let output = host
        .capture(&line, cancelled)
        .map_err(script(Step::StartAgent))?;
    let ran: Ran = serde_json::from_slice(output.trim_ascii()).map_err(|source| Error::Decode {
        step: Step::StartAgent,
        source,
    })?;
    Ok(ran.address)
}

/// Pastes `text` into `terminal` and presses Enter.
fn send(host: &Host, terminal: &str, text: &str, step: Step, cancelled: &AtomicBool) -> Result<()> {
    let line = format!(
        "core_cli send --json -- {} {} >/dev/null\n",
        shell_quote(terminal),
        shell_quote(text)
    );
    host.capture(&line, cancelled)
        .map(drop)
        .map_err(script(step))
}

/// Waits until the agent in `terminal` says it is ready for a prompt, as
/// herdr-core's own agent steps do, at most [`READY_TIMEOUT`].
fn ready(host: &Host, terminal: &str, cancelled: &AtomicBool) -> Result<()> {
    let deadline = Instant::now() + READY_TIMEOUT;
    let line = format!(
        "core_cli wait --json --timeout {WAIT_CHECK_IN_SECS} {}\n",
        shell_quote(terminal)
    );
    while Instant::now() < deadline {
        if cancelled.load(Ordering::Acquire) {
            return Err(Error::Cancelled);
        }
        let output = host
            .capture(&line, cancelled)
            .map_err(script(Step::Prompt))?;
        let waited: Waited =
            serde_json::from_slice(output.trim_ascii()).map_err(|source| Error::Decode {
                step: Step::Prompt,
                source,
            })?;
        if let Some(ready) = readiness(&waited) {
            return Ok(ready?);
        }
    }
    Err(sheprd_core_client::Error::AgentNotReady { status: None }.into())
}

// Host scripts need /bin/sh.
#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests;
