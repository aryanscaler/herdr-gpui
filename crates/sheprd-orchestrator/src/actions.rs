//! What the user asks the orchestrator to do: dispatch an agent, stop it,
//! send it text, remove its worktree, or delete a bead. Each runs on its own
//! short-lived thread with its own database connection, so a slow agent
//! start never holds up syncing, and writes only runs herdr-gpui owns.

use super::{
    Error, HerdrSession, ItemKey, Owner, Provider, Result, Run, RunState, SourceKey, Store,
    Terminals, beads,
    error::script,
    model::{Backend, Pane, Workspace},
};
use chrono::Utc;
use sheprd_client::{ConnectTarget, shell_quote};
pub use sheprd_remote::work::valid_model;
use sheprd_remote::{
    teleport::{
        self, AgentKind, Envelope, FreshOrigin, Host, HostRepositories, HostSnapshot,
        SnapshotResult, WorkspaceCreated, WorktreeCreated,
    },
    work::{self, Opened, Worktree, owned_agent::Purpose},
};
use std::{path::PathBuf, sync::atomic::AtomicBool};

mod herdr2;
pub mod legacy;
pub(crate) mod lock;
mod restart;
mod start;

/// How long `herdr agent start` may wait for the agent to be ready.
const AGENT_START_TIMEOUT_MS: &str = "90000";

/// Where actions run: the repository's host and the shared database.
#[derive(Clone)]
pub struct Site {
    pub host: Host,
    pub target: ConnectTarget,
    pub main_root: String,
    pub database: PathBuf,
}

/// Another connected host to start the agent on, set up as smart dispatch
/// and fan-out do: the repository found or cloned there, the base commit
/// shipped when it lacks it.
#[derive(Clone, Debug)]
pub struct Elsewhere {
    pub origin: FreshOrigin,
    pub destination: HostRepositories,
    pub target: ConnectTarget,
}

#[derive(Clone, Debug)]
pub struct DispatchRequest {
    /// The canonical key of the item the agent works on.
    pub item_key: String,
    pub kind: AgentKind,
    /// Passed to the agent as `--model`, when chosen.
    pub model: Option<String>,
    pub prompt: String,
    /// Human issue/PR context, independent of the agent's pasted prompt envelope.
    pub title: String,
    pub branch: String,
    /// The Herdr workspace of the repository, whose worktree list gets the
    /// new checkout.
    pub workspace_id: String,
    /// What the branch starts from; the main checkout's `HEAD` when `None`.
    pub base: Option<String>,
    /// Another host to start on; the repository's own when `None`.
    pub elsewhere: Option<Elsewhere>,
    /// Which Herdr starts work on the repository's own host. A dispatch
    /// elsewhere uses that destination's declared keeper.
    pub terminals: Terminals,
}

impl DispatchRequest {
    /// The item's key when it is a bead.
    fn bead(&self) -> Option<ItemKey> {
        self.item_key
            .parse::<ItemKey>()
            .ok()
            .filter(|key| key.source.provider == Provider::Beads)
    }
}

#[derive(Clone, Debug)]
pub enum Action {
    Adopt {
        run: String,
        receipt: Box<legacy::Receipt>,
    },
    Recover {
        run: String,
    },
    Restart {
        run: String,
    },
    Dispatch(Box<DispatchRequest>),
    /// Interrupt a run's agent.
    Stop {
        run: String,
    },
    /// Type `text` to a run's agent as a prompt.
    Send {
        run: String,
        text: String,
    },
    /// Remove a run's worktree and forget the run.
    Remove {
        run: String,
    },
    /// Delete a bead for good.
    DeleteBead {
        source: SourceKey,
        id: String,
    },
}

impl Action {
    /// Whether it changes a source's items, which the next sync then shows.
    pub(crate) fn changes_items(&self) -> bool {
        match self {
            Self::DeleteBead { .. } => true,
            Self::Dispatch(request) => request.bead().is_some(),
            Self::Adopt { .. }
            | Self::Stop { .. }
            | Self::Send { .. }
            | Self::Remove { .. }
            | Self::Recover { .. }
            | Self::Restart { .. } => false,
        }
    }

    /// What the user reads once it succeeded.
    fn done(&self) -> Done {
        match self {
            Self::Dispatch(_) => Done::Started,
            Self::Recover { .. } => Done::Recovered,
            Self::Adopt { .. } => Done::Adopted,
            Self::Restart { .. } => Done::Started,
            Self::Stop { .. } => Done::Stopped,
            Self::Send { .. } => Done::Sent,
            Self::Remove { .. } => Done::Removed,
            Self::DeleteBead { .. } => Done::BeadDeleted,
        }
    }
}

/// What an action did, for the app to say in its own words.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Done {
    // keep-sorted start
    Adopted,
    BeadDeleted,
    Recovered,
    Removed,
    Sent,
    Started,
    Stopped,
    // keep-sorted end
}

/// Runs `action` to completion; what it did, or why it failed.
pub fn perform(site: &Site, action: &Action, cancelled: &AtomicBool) -> Result<Done> {
    let _lease = lock::take(site)?;
    match action {
        Action::Adopt { run, receipt } => legacy::adopt(site, run, receipt, cancelled)?,
        Action::Recover { run } => restart::recover(site, run, cancelled)?,
        Action::Restart { run } => restart::restart(site, run, cancelled)?,
        Action::Dispatch(request) => dispatch(site, request, cancelled)?,
        Action::Stop { run } => {
            let (mut store, mut found, session, host) = own_run(site, run, cancelled)?;
            match &session.pane {
                Pane::Pending => return Err(Error::NothingToOpen),
                Pane::Herdr1 { pane_id, .. } => {
                    for key in ["esc", "ctrl+c"] {
                        herdr(
                            &host,
                            &["agent", "send-keys", pane_id, key],
                            "Stopping the agent",
                            cancelled,
                        )?;
                    }
                }
                Pane::Herdr2 { terminal } => {
                    herdr2::recover(&host, &found, &session, cancelled)?;
                    herdr2::stop(&host, terminal, cancelled)?;
                }
            }
            found.state = RunState::Cancelled;
            found.updated_at = Utc::now();
            store.update_run(&found, Some(&session))?;
        }
        Action::Send { run, text } => {
            let (_, found, session, host) = own_run(site, run, cancelled)?;
            match &session.pane {
                Pane::Pending => return Err(Error::NothingToOpen),
                Pane::Herdr1 { pane_id, .. } => {
                    herdr(
                        &host,
                        &["agent", "prompt", pane_id, text],
                        "Sending the message",
                        cancelled,
                    )?;
                }
                Pane::Herdr2 { terminal } => {
                    herdr2::recover(&host, &found, &session, cancelled)?;
                    herdr2::send(&host, terminal, text, cancelled)?;
                }
            }
        }
        Action::Remove { run } => {
            let (mut store, found, session) = owned(site, run)?;
            // A checkout without its destination cannot safely be removed
            // or forgotten. New launches record a Pending session first.
            if let Some(workspace) = &found.workspace {
                let session = session.as_ref().ok_or(Error::NothingToOpen)?;
                remove_worktree(site, &found, session, workspace, cancelled)?;
            }
            store.delete_run(&found.id)?;
        }
        Action::DeleteBead { source, id } => beads::delete(&site.host, source, id, cancelled)?,
    }
    Ok(action.done())
}

/// A run Sheprd owns, with its session and the host its agent is on,
/// read fresh from the database.
fn own_run(
    site: &Site,
    id: &str,
    cancelled: &AtomicBool,
) -> Result<(Store, Run, HerdrSession, Host)> {
    let (mut store, run, session) = owned(site, id)?;
    let mut session = session.ok_or_else(|| Error::RunNotFound(id.to_owned()))?;
    let host = host_of(site, &session)?;
    if matches!(session.pane, Pane::Pending) {
        let work::Started::Herdr2 { terminal } =
            herdr2::inspect(&host, &run, &session, Purpose::End, cancelled)?
        else {
            return Err(Error::NothingToOpen);
        };
        session.pane = Pane::Herdr2 { terminal };
        store.update_run(&run, Some(&session))?;
    }
    Ok((store, run, session, host))
}

/// A run Sheprd owns and its session, if it has one, read fresh.
fn owned(site: &Site, id: &str) -> Result<(Store, Run, Option<HerdrSession>)> {
    let store = Store::open(&site.database)?;
    let run = store
        .runs()?
        .into_iter()
        .find(|run| run.id == id)
        .ok_or_else(|| Error::RunNotFound(id.to_owned()))?;
    if run.owner != Owner::Sheprd {
        return Err(Error::NotOwner(id.to_owned()));
    }
    let session = store
        .sessions()?
        .into_iter()
        .find(|session| session.run_id == id);
    Ok((store, run, session))
}

/// The host a session's agent is on: the repository's own unless the run was
/// dispatched elsewhere.
fn host_of(site: &Site, session: &HerdrSession) -> Result<Host> {
    if target_of(session) == site.target {
        return Ok(site.host.clone());
    }
    Host::new(&target_of(session)).map_err(|_| Error::UnsupportedHost)
}

/// The target a session's host and Herdr session name.
pub fn target_of(session: &HerdrSession) -> ConnectTarget {
    if let Some(target) = &session.target {
        return target.clone();
    }
    match (&session.host, &session.session) {
        (Some(host), session) => ConnectTarget::Ssh {
            target: host.clone(),
            session: session.clone().unwrap_or_else(|| "default".into()),
        },
        (None, Some(name)) => ConnectTarget::Session {
            name: name.clone(),
            development: false,
        },
        (None, None) => ConnectTarget::Local,
    }
}

fn herdr(
    host: &Host,
    args: &[&str],
    operation: &'static str,
    cancelled: &AtomicBool,
) -> Result<Vec<u8>> {
    host.herdr_exec(args, Host::QUERY_IDLE, cancelled)
        .map_err(script(operation))
}

/// A run's worktree, removed on the host its session lives on. Herdr's
/// `worktree remove` addresses the workspace the worktree was made for, and
/// Herdr forgets a workspace whose last pane has exited, so the workspace id
/// recorded at dispatch goes stale once the run's pane is gone. A workspace
/// the snapshot still lists goes through Herdr, which also closes its panes;
/// a gone one has its recorded checkout removed with git, which only the
/// workspace could have asked Herdr to do. A Herdr 2 run's agent is ended
/// and its checkout removed with git, as herdr-core keeps no worktrees.
fn remove_worktree(
    site: &Site,
    run: &Run,
    session: &HerdrSession,
    workspace: &Workspace,
    cancelled: &AtomicBool,
) -> Result<()> {
    const READING: &str = "Reading the workspaces";
    let host = host_of(site, session)?;
    if matches!(session.pane, Pane::Pending | Pane::Herdr2 { .. }) {
        let exists = validate_checkout(&host, workspace, cancelled)?;
        let tree = workspace.path.as_ref().map(|path| Worktree {
            path: path.to_string_lossy().into_owned(),
            opened: Opened::Herdr2,
        });
        if exists && let (Some(token), Some(tree)) = (&session.checkout_token, &tree) {
            work::owned_checkout::verify(&host, tree, &workspace.branch, token, cancelled)?;
        }
        match herdr2::inspect(&host, run, session, Purpose::End, cancelled) {
            Ok(work::Started::Herdr2 { terminal }) => herdr2::end(&host, &terminal)?,
            Ok(_) => return Err(Error::NothingToOpen),
            Err(error) => {
                // A terminal already removed needs no further mutation; an
                // unknown occupant or unreadable inventory must retain the tree.
                if herdr2::unused_checkout(&host, workspace, cancelled).is_err() {
                    return Err(error);
                }
            }
        }
        herdr2::unused_checkout(&host, workspace, cancelled)?;
        if exists && let (Some(token), Some(tree)) = (&session.checkout_token, &tree) {
            work::owned_checkout::discard(&host, tree, &workspace.branch, token, cancelled)?;
            return Ok(());
        }
        return remove_checkout(&host, workspace, cancelled);
    }
    let alive = host_snapshot(&host, READING, cancelled)?;
    if let Some(found) = alive.workspace(&workspace.id) {
        let same_checkout = found.worktree.as_ref().is_some_and(|tree| {
            workspace
                .path
                .as_deref()
                .is_some_and(|path| path == std::path::Path::new(&tree.checkout_path))
        });
        if !same_checkout {
            return Err(Error::InvalidCheckout);
        }
        herdr(
            &host,
            &[
                "worktree",
                "remove",
                "--workspace",
                &workspace.id,
                "--force",
            ],
            REMOVING,
            cancelled,
        )?;
        return Ok(());
    }
    remove_checkout(&host, workspace, cancelled)
}

const REMOVING: &str = "Removing the worktree";

fn validate_checkout(host: &Host, workspace: &Workspace, cancelled: &AtomicBool) -> Result<bool> {
    let Some(path) = workspace.path.as_deref() else {
        return Ok(false);
    };
    let path = path.to_string_lossy();
    let exists = if host.is_local() {
        std::path::Path::new(path.as_ref()).is_dir()
    } else {
        !host
            .capture(
                &format!("if [ -d {} ]; then printf yes; fi\n", shell_quote(&path)),
                cancelled,
            )
            .map_err(script(REMOVING))?
            .is_empty()
    };
    if !exists {
        return Ok(false);
    }
    let identity = work::identity(host, &path, cancelled)?;
    let prefix = work::git(host, &path, &["rev-parse", "--show-prefix"], cancelled)?;
    if !identity.linked
        || identity.branch.as_deref() != Some(&workspace.branch)
        || !prefix.is_empty()
    {
        return Err(Error::InvalidCheckout);
    }
    Ok(true)
}

/// The checkout a run recorded, removed with Git: all a Herdr 1 workspace
/// that is gone could leave, and all a Herdr 2 run has.
fn remove_checkout(host: &Host, workspace: &Workspace, cancelled: &AtomicBool) -> Result<()> {
    // A run without a recorded checkout has nothing on the host to remove.
    let Some(path) = workspace.path.as_deref() else {
        return Ok(());
    };
    if host.is_local() {
        if !path.is_dir() {
            return Ok(());
        }
        if !path.join(".git").is_file() {
            return Err(Error::InvalidCheckout);
        }
        let path = path.to_string_lossy();
        let git = |args: &[&str]| {
            host.exec("git", args, Host::QUERY_IDLE, cancelled)
                .map_err(script(REMOVING))
        };
        let branch = git(&["-C", &path, "symbolic-ref", "--quiet", "HEAD"])?;
        if branch.trim_ascii_end() != format!("refs/heads/{}", workspace.branch).as_bytes() {
            return Err(Error::InvalidCheckout);
        }
        let common = git(&[
            "-C",
            &path,
            "rev-parse",
            "--path-format=absolute",
            "--git-common-dir",
        ])?;
        let common = String::from_utf8_lossy(&common);
        let git_dir = format!("--git-dir={}", common.trim_end_matches(['\r', '\n']));
        git(&[&git_dir, "worktree", "remove", "--force", &path])?;
        return Ok(());
    }
    // Never let Git walk up from a stale path into another checkout. A
    // recorded worktree is a root with a .git file and the recorded branch.
    // Both checks run on the destination, including for WSL.
    let body = format!(
        "path={path}\n\
         [ -d \"$path\" ] || exit 0\n\
         [ -f \"$path/.git\" ] || exit 1\n\
         branch=$(git -C \"$path\" symbolic-ref --quiet HEAD) || exit 1\n\
         [ \"$branch\" = {branch} ] || exit 1\n\
         common=$(git -C \"$path\" rev-parse --path-format=absolute --git-common-dir 2>/dev/null) || exit 1\n\
         git --git-dir=\"$common\" worktree remove --force \"$path\"\n",
        path = shell_quote(&path.to_string_lossy()),
        branch = shell_quote(&format!("refs/heads/{}", workspace.branch)),
    );
    host.capture(&body, cancelled).map_err(script(REMOVING))?;
    Ok(())
}

/// The typed remote host and Herdr session a target names, as sessions record.
pub fn place(target: &ConnectTarget) -> (Option<String>, Option<String>) {
    match target {
        ConnectTarget::Ssh { target, session } => (Some(target.clone()), Some(session.clone())),
        ConnectTarget::Session { name, .. } => (None, Some(name.clone())),
        target if target.is_remote() => (
            Host::new(target).ok().map(|host| host.identity()),
            target.remote_session().map(str::to_owned),
        ),
        _ => (None, None),
    }
}

/// The longest name Herdr's `agent start` accepts. A name must also start
/// with a lowercase letter and hold only lowercase letters, digits, `-`, or `_`.
const MAX_AGENT_NAME_LEN: usize = 32;

/// A Herdr agent name no other application or run uses.
pub fn agent_name() -> String {
    const PREFIX: &str = "herdr-gpui-";
    // A UUID's simple form is 32 hex digits, so the slice stays in range.
    let hex = uuid::Uuid::new_v4().simple().to_string();
    format!("{PREFIX}{}", &hex[..MAX_AGENT_NAME_LEN - PREFIX.len()])
}

/// Start `kind` as `name` in `pane`, the root pane of a worktree just
/// created on `host`, as a dispatch does. While the new shell is still
/// starting, a busy rejection is retried for a bounded time, and only while
/// the pane still holds `terminal`, the one creation returned; without it,
/// the first answer stands. Blocks, so callers run it on a worker.
pub fn start_agent(
    host: &Host,
    kind: AgentKind,
    name: &str,
    pane: &str,
    terminal: Option<&str>,
    cancelled: &AtomicBool,
) -> Result<()> {
    let args = [
        "agent",
        "start",
        name,
        "--kind",
        kind.name(),
        "--pane",
        pane,
        "--timeout",
        AGENT_START_TIMEOUT_MS,
    ];
    start::agent(host, &args, pane, terminal, cancelled)
}

/// Where the new worktree goes: the host, its target, the repository that
/// gets it (a Herdr 1 workspace of it, or a checkout for Herdr 2), and the
/// base commit.
struct Destination {
    host: Host,
    target: ConnectTarget,
    repository: work::Repository,
    commit: String,
    prepared: Option<teleport::Prepared>,
}

/// Ownership kept across fallible launch and journal steps. A started
/// terminal's receipt must survive even when its first database write does not.
#[derive(Default)]
struct Attempt {
    created: Option<(Host, Worktree)>,
    retained_session: Option<HerdrSession>,
}

/// Dispatches `request`. An agent sent to a bead claims it first, on the
/// host of its database, so a bead someone holds starts nothing. A launch
/// that fails gives the claim back, so nothing stays in progress that no
/// agent is working on; the bead's status is otherwise the agent's to change.
fn dispatch(site: &Site, request: &DispatchRequest, cancelled: &AtomicBool) -> Result<()> {
    let mut retained = false;
    let Some(key) = request.bead() else {
        return start_run(site, request, cancelled, &mut retained);
    };
    let claim = beads::claim::take(&site.host, &key.source, &key.native_id, cancelled)?;
    let outcome = start_run(site, request, cancelled, &mut retained);
    if outcome.is_err() && !retained {
        // Run even when cancelled, as a failed run's worktree is removed.
        let released =
            beads::claim::release(&site.host, &key.source, &claim, &AtomicBool::new(false));
        if let Err(error) = released {
            tracing::warn!(
                category = "orchestrator",
                %error,
                "A failed dispatch could not give its claim back"
            );
        }
    }
    outcome
}

fn start_run(
    site: &Site,
    request: &DispatchRequest,
    cancelled: &AtomicBool,
    retained: &mut bool,
) -> Result<()> {
    let target = request
        .elsewhere
        .as_ref()
        .map_or(&site.target, |elsewhere| &elsewhere.target);
    Host::new(target).map_err(|_| Error::UnsupportedHost)?;
    let mut store = Store::open(&site.database)?;
    let name = agent_name();
    let now = Utc::now();
    let mut run = Run {
        id: uuid::Uuid::new_v4().hyphenated().to_string(),
        item_key: request.item_key.clone(),
        workspace: None,
        agent: request.kind.name().to_owned(),
        model: request.model.clone(),
        state: RunState::Provisioning,
        message: None,
        session_id: Some(name.clone()),
        started_at: now,
        updated_at: now,
        owner: Owner::Sheprd,
    };
    store.save_run(&run, None)?;
    store.save_prompt(&run.id, &request.prompt)?;
    let mut attempt = Attempt::default();
    let outcome = destination(site, request, cancelled).and_then(|destination| {
        // Carry uncertainty outside error journaling too: a database failure
        // must not release a claim while an accepted launch may be working.
        *retained = true;
        let outcome = launch(
            &destination,
            request,
            &name,
            &mut run,
            &mut store,
            &mut attempt,
            cancelled,
        );
        // The new branch keeps the shipped commit; its reference can go.
        if let Some(prepared) = &destination.prepared {
            prepared.finish(&AtomicBool::new(false));
        }
        outcome
    });
    if let Err(error) = &outcome {
        // Undo the checkout of a run that never got going; the branch stays.
        // A checkout that would not go keeps its session, which names the
        // host, so Remove can try again; one that went is no longer the run's.
        let mut kept = match attempt.retained_session {
            Some(session) => Some(session),
            None => store
                .sessions()?
                .into_iter()
                .find(|session| session.run_id == run.id),
        };
        if let Some((host, worktree)) = attempt.created {
            kept = store
                .sessions()?
                .into_iter()
                .find(|session| session.run_id == run.id);
            if kept.is_none() && matches!(worktree.opened, Opened::Herdr2) {
                let (host, session) = place(target);
                kept = Some(HerdrSession {
                    checkout_token: None,
                    adopted: None,
                    run_id: run.id.clone(),
                    target: Some(target.clone()),
                    host,
                    session,
                    pane: Pane::Pending,
                    agent_name: name.clone(),
                    updated_at: Utc::now(),
                });
            }
            // A terminal must be confirmed ended before its checkout goes.
            // Transport failure keeps both records so Remove can retry.
            let ended = match kept.as_ref().map(|session| &session.pane) {
                Some(Pane::Herdr2 { terminal }) => herdr2::end(&host, terminal),
                Some(Pane::Pending) | None if matches!(worktree.opened, Opened::Herdr2) => {
                    match run.workspace.as_ref() {
                        Some(workspace) => {
                            herdr2::unused_checkout(&host, workspace, &AtomicBool::new(false))
                        }
                        None => Err(Error::NothingToOpen),
                    }
                }
                _ => Ok(()),
            };
            let removed = ended.and_then(|()| match &worktree.opened {
                Opened::Herdr1 { workspace_id, .. } => herdr(
                    &host,
                    &["worktree", "remove", "--workspace", workspace_id, "--force"],
                    "Removing the worktree",
                    &AtomicBool::new(false),
                )
                .map(drop),
                Opened::Herdr2 => match run.workspace.as_ref() {
                    Some(workspace) => remove_checkout(&host, workspace, &AtomicBool::new(false)),
                    None => Err(Error::NothingToOpen),
                },
            });
            if removed.is_ok() {
                run.workspace = None;
                kept = None;
            }
        }
        run.state = RunState::Failed;
        *retained = run.workspace.is_some() && kept.is_some();
        run.message = Some(error.to_string().chars().take(500).collect());
        run.updated_at = Utc::now();
        // A run removed meanwhile stays removed; the failure is still told.
        match store.update_run(&run, kept.as_ref()) {
            Ok(()) | Err(Error::RunNotFound(_)) => {}
            Err(journal_error) if *retained => {
                // One bounded recovery write; keep the original failure and
                // its source even when storage stays unavailable.
                tracing::warn!(category = "orchestrator", %journal_error, "Could not persist a retained agent's recovery receipt");
            }
            Err(error) => return Err(error),
        }
    }
    outcome.map_err(|source| {
        if *retained {
            Error::Retained {
                source: Box::new(source),
            }
        } else {
            source
        }
    })
}

fn destination(
    site: &Site,
    request: &DispatchRequest,
    cancelled: &AtomicBool,
) -> Result<Destination> {
    let base = request.base.as_deref().unwrap_or("HEAD");
    if let Some(elsewhere) = &request.elsewhere {
        let commit =
            teleport::base_commit(&elsewhere.origin, base, cancelled).map_err(Error::Teleport)?;
        let prepared = teleport::prepare(
            &elsewhere.origin,
            &elsewhere.destination,
            &commit,
            &mut |_| {},
            cancelled,
        )
        .map_err(Error::Teleport)?;
        return Ok(Destination {
            host: elsewhere.destination.place.host.clone(),
            target: elsewhere.target.clone(),
            repository: prepared.work_repository().clone(),
            commit,
            prepared: Some(prepared),
        });
    }
    let commit = if site.host.is_local() {
        super::git::commit(std::path::Path::new(&site.main_root), base)?
    } else {
        let output = site
            .host
            .exec(
                "git",
                &[
                    "-C",
                    &site.main_root,
                    "rev-parse",
                    "--verify",
                    "--quiet",
                    &format!("{base}^{{commit}}"),
                ],
                Host::QUERY_IDLE,
                cancelled,
            )
            .map_err(script("Reading the base commit"))?;
        String::from_utf8_lossy(&output).trim().to_owned()
    };
    if commit.len() < 40 || !commit.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(Error::Output {
            operation: "Reading the base commit",
        });
    }
    let repository = match request.terminals {
        Terminals::Herdr1 => work::Repository::Herdr1 {
            workspace_id: parent_workspace(site, &request.workspace_id, cancelled)?,
        },
        // Git makes the worktree from any checkout of the repository.
        Terminals::Herdr2 => work::Repository::Herdr2 {
            checkout: site.main_root.clone(),
        },
    };
    Ok(Destination {
        host: site.host.clone(),
        target: site.target.clone(),
        repository,
        commit,
        prepared: None,
    })
}

/// The host's workspaces, tabs, and panes, as `herdr api snapshot` reports
/// them.
fn host_snapshot(
    host: &Host,
    operation: &'static str,
    cancelled: &AtomicBool,
) -> Result<HostSnapshot> {
    let output = herdr(host, &["api", "snapshot"], operation, cancelled)?;
    serde_json::from_slice::<Envelope<SnapshotResult>>(&output)
        .map(|envelope| envelope.result.snapshot)
        .map_err(|source| Error::OutputJson { operation, source })
}

/// The workspace new worktrees of `workspace_id`'s repository are created
/// from. Herdr creates them only from a repository's parent workspace, so a
/// view opened in a linked worktree goes through its parent, opening one on
/// the main checkout when none is.
fn parent_workspace(site: &Site, workspace_id: &str, cancelled: &AtomicBool) -> Result<String> {
    const READING: &str = "Reading the workspaces";
    const OPENING: &str = "Opening the repository";
    let snapshot = host_snapshot(&site.host, READING, cancelled)?;
    if let Some(parent) = parent_of(&snapshot, workspace_id) {
        return Ok(parent.to_owned());
    }
    let output = herdr(
        &site.host,
        &[
            "workspace",
            "create",
            "--cwd",
            &site.main_root,
            "--no-focus",
        ],
        OPENING,
        cancelled,
    )?;
    Ok(
        serde_json::from_slice::<Envelope<WorkspaceCreated>>(&output)
            .map_err(|source| Error::OutputJson {
                operation: OPENING,
                source,
            })?
            .result
            .workspace
            .workspace_id,
    )
}

/// `workspace_id` unless it is a linked worktree, else an open parent
/// workspace of its repository; `None` when no parent is open. A workspace
/// the snapshot lacks is passed through for Herdr to report.
pub fn parent_of<'a>(snapshot: &'a HostSnapshot, workspace_id: &'a str) -> Option<&'a str> {
    let Some(linked) = snapshot
        .workspace(workspace_id)
        .and_then(|workspace| workspace.worktree.as_ref())
        .filter(|tree| tree.is_linked_worktree)
    else {
        return Some(workspace_id);
    };
    snapshot
        .workspaces
        .iter()
        .find(|workspace| {
            workspace
                .worktree
                .as_ref()
                .is_some_and(|tree| tree.repo_key == linked.repo_key && !tree.is_linked_worktree)
        })
        .map(|workspace| workspace.workspace_id.as_str())
}

fn launch(
    destination: &Destination,
    request: &DispatchRequest,
    name: &str,
    run: &mut Run,
    store: &mut Store,
    attempt: &mut Attempt,
    cancelled: &AtomicBool,
) -> Result<()> {
    let workspace_id = match &destination.repository {
        work::Repository::Herdr1 { workspace_id } => workspace_id,
        work::Repository::Herdr2 { checkout } => {
            return herdr2::launch(
                destination,
                checkout,
                request,
                name,
                run,
                store,
                attempt,
                cancelled,
            );
        }
    };
    let host = &destination.host;
    let output = herdr(
        host,
        &[
            "worktree",
            "create",
            "--workspace",
            workspace_id,
            "--branch",
            &request.branch,
            "--base",
            &destination.commit,
            "--no-focus",
        ],
        "Creating the worktree",
        cancelled,
    )?;
    let created = serde_json::from_slice::<Envelope<WorktreeCreated>>(&output)
        .map_err(|source| Error::OutputJson {
            operation: "Creating the worktree",
            source,
        })?
        .result;
    attempt.created = Some((
        host.clone(),
        Worktree {
            path: created.worktree.path.clone(),
            opened: Opened::Herdr1 {
                workspace_id: created.workspace.workspace_id.clone(),
                pane_id: created.root_pane.pane_id.clone(),
            },
        },
    ));
    let (ssh, session) = place(&destination.target);
    run.workspace = Some(Workspace {
        backend: Backend::Herdr,
        id: created.workspace.workspace_id.clone(),
        host: ssh.clone(),
        path: Some(created.worktree.path.clone().into()),
        branch: request.branch.clone(),
    });
    run.state = RunState::Starting;
    run.updated_at = Utc::now();
    let herdr_session = HerdrSession {
        checkout_token: None,
        adopted: None,
        run_id: run.id.clone(),
        target: Some(destination.target.clone()),
        host: ssh,
        session,
        pane: Pane::Herdr1 {
            workspace_id: created.workspace.workspace_id,
            pane_id: created.root_pane.pane_id.clone(),
        },
        agent_name: name.to_owned(),
        updated_at: run.updated_at,
    };
    store.update_run(run, Some(&herdr_session))?;
    let mut start = vec![
        "agent",
        "start",
        name,
        "--kind",
        request.kind.name(),
        "--pane",
        &created.root_pane.pane_id,
        "--timeout",
        AGENT_START_TIMEOUT_MS,
    ];
    let model = request.model.as_deref().filter(|model| valid_model(model));
    if let Some(model) = model {
        start.extend(["--", "--model", model]);
    }
    start::agent(
        host,
        &start,
        &created.root_pane.pane_id,
        created.root_pane.terminal_id.as_deref(),
        cancelled,
    )?;
    let title = sheprd_client::text::safe_text(&request.title, 80);
    // Herdr's variadic labels treat `--` as text; keep a title from being
    // parsed as an option without inserting a visible separator.
    let title = if title.trim_start().starts_with('-') {
        format!("Task {title}").chars().take(80).collect()
    } else {
        title
    };
    if !title.trim().is_empty() {
        herdr(
            host,
            &["tab", "rename", &created.tab.tab_id, &title],
            "Naming the agent tab",
            cancelled,
        )?;
        herdr(
            host,
            &["pane", "rename", &created.root_pane.pane_id, &title],
            "Naming the agent pane",
            cancelled,
        )?;
    }
    // The pane is the target: a name could match an agent started elsewhere.
    herdr(
        host,
        &[
            "agent",
            "prompt",
            &created.root_pane.pane_id,
            &request.prompt,
        ],
        "Sending the prompt",
        cancelled,
    )?;
    run.state = RunState::Running;
    run.updated_at = Utc::now();
    store.update_run(run, Some(&herdr_session))?;
    Ok(())
}
