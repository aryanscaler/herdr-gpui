//! The three blocking phases of a teleport, each run on a background worker:
//! find destinations, review what will move, and move it.

use super::{
    core, credentials,
    error::{Error, Result, Step},
    git,
    host::Host,
    launch::{Work, command_line, handoff_prompt, handoff_resume_prompt, remap, remap_argv},
    provision::{self, Arrival},
    remote::MatchReason,
    sessions::{Route, move_session},
    snapshot::{AgentSession, PaneInfoResult, TabCreated, WorkspaceCreated},
    warning::Warning,
};
use sheprd_backend::Generation;
use sheprd_client::{protocol::ClientShellSnapshot, shell_quote};
use sheprd_session::{Keeper, panes::Panes};
use std::{
    collections::HashMap,
    io::Seek,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

mod discover;
mod land;
pub(super) mod plan;

// Only the live tests read a host's repositories outside discovery.
#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
pub use discover::repositories_of;
pub use discover::resolve;
pub use land::Landed;
use land::{Landing, land, retire};
pub use plan::{Action, GitHubAccess, PanePlan, Review, review};

/// How long an agent may take to write its handoff note.
pub(super) const HANDOFF_TIMEOUT: Duration = Duration::from_secs(300);

/// An endpoint as Teleport addresses it.
#[derive(Debug, Clone)]
pub struct Place {
    pub endpoint_id: String,
    pub label: String,
    pub host: Host,
}

/// Who keeps a workspace, and so where Teleport reads its checkout and how
/// it stops its programs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Keeping {
    /// Herdr 1, whose snapshot names the checkout.
    Herdr1,
    /// The app, for Herdr 2 terminals, with the workspace's folder: the
    /// checkout, which herdr-core knows nothing of.
    App { checkout: String },
}

/// The workspace being moved, as the GUI snapshot describes it.
#[derive(Debug, Clone)]
pub struct Source {
    pub place: Place,
    pub workspace_id: String,
    pub keeping: Keeping,
    /// A label the user chose, carried to the destination workspace.
    pub custom_label: Option<String>,
    pub repo_key: String,
    pub repo_label: String,
    /// The branch the GUI snapshot shows, for finding a checkout it left.
    pub branch: Option<String>,
    /// Tab labels the user chose, by source tab id.
    pub tab_labels: HashMap<String, String>,
    /// The workspace's Herdr 2 tabs, which the app lays out itself: their
    /// terminals run in the source host's herdr-core, and each moves as one
    /// more tab after the Herdr 1 ones.
    pub core_tabs: Vec<Panes>,
    /// Herdr 2 was chosen for new tabs: on a destination with a herdr-core,
    /// Herdr 1 tabs move into it too.
    pub core_preferred: bool,
}

/// A repository open on some host, found through one of its workspaces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repository {
    pub key: String,
    pub label: String,
    /// A workspace of the repository, preferably its main checkout.
    pub workspace_id: String,
}

#[derive(Debug, Clone)]
pub struct HostRepositories {
    pub place: Place,
    /// The host's open repositories from its GUI snapshot, or `None` when it
    /// is not connected and they must be read through its CLI.
    pub repositories: Option<Vec<Repository>>,
    /// Checkouts on this host this client marked as teleported away.
    pub retired: Vec<Retired>,
    /// Who keeps a workspace the move opens there: the app, where Herdr 2
    /// takes new workspaces, else Herdr 1.
    pub lands_in: Keeper,
}

/// A checkout whose work was teleported away, still open as a workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Retired {
    pub repo_key: String,
    pub branch: String,
    pub workspace_id: String,
    pub keeping: Keeping,
}

/// Where the worktree lands on a host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Destination {
    /// The repository is open there already.
    Open {
        repository: Repository,
        reason: MatchReason,
    },
    /// It is not open: open an existing checkout or clone one first.
    Arrive(Arrival),
    /// The work left this checkout earlier and is coming back to it.
    Reclaim {
        repository: Repository,
        workspace_id: String,
        keeping: Keeping,
    },
}

#[derive(Debug, Clone)]
pub struct Candidate {
    pub place: Place,
    pub destination: Destination,
    /// `origin`'s URL, for a clone the destination can fetch itself.
    pub origin: Option<String>,
    /// Who keeps the workspace the move lands in; see
    /// [`HostRepositories::lands_in`].
    pub lands_in: Keeper,
}

/// Where the teleported workspace ended up.
#[derive(Debug)]
pub struct Outcome {
    pub endpoint_id: String,
    pub workspace: Landed,
    /// The destination repository, for remembering where the work went.
    pub repo_key: String,
    pub branch: String,
    /// Things that did not carry over, for the user to see.
    pub warnings: Vec<Warning>,
    /// The tabs rebuilt in the destination's herdr-core, for the app to open
    /// in the workspace, in order.
    pub core_tabs: Vec<Panes>,
}

/// The repositories open in `snapshot`, one per Git common directory, each
/// reached through its main checkout's workspace when that is open.
pub fn open_repositories(snapshot: &ClientShellSnapshot) -> Vec<Repository> {
    let mut found: Vec<Repository> = Vec::new();
    for workspace in &snapshot.workspaces {
        let Some(tree) = &workspace.worktree else {
            continue;
        };
        match found.iter_mut().find(|repo| repo.key == tree.key) {
            Some(repo) if !tree.is_linked_worktree => {
                repo.workspace_id.clone_from(&workspace.workspace_id);
            }
            Some(_) => {}
            None => found.push(Repository {
                key: tree.key.clone(),
                label: tree.label.clone(),
                workspace_id: workspace.workspace_id.clone(),
            }),
        }
    }
    found
}

pub(super) fn reference_name() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos());
    format!("refs/herdr-teleport/{nanos:x}-{:x}", std::process::id())
}

/// The sessions the review found in Herdr 2 terminals, by terminal, which
/// must still be running when the move stops them.
fn core_sessions(review: &Review) -> Vec<(String, AgentSession)> {
    review
        .tabs
        .iter()
        .filter(|tab| tab.generation == Generation::Herdr2)
        .flat_map(|tab| &tab.panes)
        .filter_map(|pane| match &pane.work {
            Work::Session { session, .. } => Some((pane.pane_id.clone(), session.clone())),
            _ => None,
        })
        .collect()
}

/// Move the workspace. `report` announces each step as it starts.
pub fn run(
    source: &Source,
    destination: &Candidate,
    review: &Review,
    mut report: impl FnMut(Step),
    cancelled: &AtomicBool,
) -> Result<Outcome> {
    let from = &source.place.host;
    let to = &destination.place.host;
    let mut warnings = Vec::new();
    // Herdr 2 agents' sessions are read from their processes, which may have
    // moved on to another conversation since the review. Checked before
    // anything changes, so a refusal leaves nothing behind on either host.
    let resumed = core_sessions(review);
    core::confirm(from, &resumed, cancelled)?;

    // The repository comes first: if it cannot be put on the destination,
    // nothing on the source has been touched.
    let repository = arrive(source, destination, &mut report, cancelled)?;
    let key = &repository.key;

    // Notes are written into the source checkout first, so they travel with
    // the uncommitted changes.
    let handoffs: Vec<(Generation, &PanePlan)> = review
        .tabs
        .iter()
        .flat_map(|tab| tab.panes.iter().map(|pane| (tab.generation, pane)))
        .filter(|(_, pane)| matches!(pane.action, Action::Handoff { .. }))
        .collect();
    if !handoffs.is_empty() {
        report(Step::Handoff);
        let timeout = HANDOFF_TIMEOUT.as_millis().to_string();
        for (generation, pane) in handoffs {
            let Action::Handoff { note, .. } = &pane.action else {
                continue;
            };
            let prompt = handoff_prompt(&format!("{}/{note}", review.checkout));
            let asked = match generation {
                Generation::Herdr1 => from.herdr_ok(
                    Step::Handoff,
                    &[
                        "agent",
                        "prompt",
                        &pane.pane_id,
                        &prompt,
                        "--wait",
                        "--timeout",
                        &timeout,
                    ],
                    cancelled,
                ),
                Generation::Herdr2 => {
                    core::prompt(from, &pane.pane_id, &prompt, HANDOFF_TIMEOUT, cancelled)
                }
            };
            match asked {
                Err(Error::Cancelled) => return Err(Error::Cancelled),
                Err(error) => warnings.push(Warning::NoHandoffNote {
                    pane: Some(pane.pane_id.clone()),
                    source: error,
                }),
                Ok(()) => {}
            }
        }
    }

    report(Step::Capture);
    let dest = git::destination_branch(to, key, &review.branch, cancelled)?;
    // Coming back, the branch is checked out in the very checkout being
    // reclaimed, which is backed up rather than refused.
    if !matches!(destination.destination, Destination::Reclaim { .. }) {
        git::check_destination(from, &review.checkout, &review.branch, &dest, cancelled)?;
    }
    let reference = reference_name();
    let mut bundle = tempfile::tempfile().map_err(Error::LocalFile)?;
    git::capture(
        from,
        &review.checkout,
        &reference,
        &dest.tips,
        &mut bundle,
        cancelled,
    )?;
    bundle.rewind().map_err(Error::LocalFile)?;

    report(Step::Transfer);
    let uploaded = git::upload(to, bundle, cancelled)?;
    report(Step::Fetch);
    let fetched = git::fetch(to, key, &uploaded, &reference, cancelled);
    let _ = git::discard_upload(to, &uploaded, cancelled);
    fetched?;

    // Herdr 2 terminals started from here on end again unless the move
    // finishes, since only its outcome gives them tabs.
    let mut spawned = core::Spawned::new(to);
    let placed = (|| {
        let landing = land(
            source,
            destination,
            &repository,
            review,
            &reference,
            &mut report,
            cancelled,
        )?;
        let rebuilt = rebuild_tabs(to, &landing, review, &mut spawned, &mut report, cancelled)?;
        Ok((landing, rebuilt))
    })();
    let (landing, (panes, core_tabs)) = match placed {
        Ok(placed) => placed,
        Err(error) => {
            git::drop_reference(to, key, &reference, &AtomicBool::new(false));
            return Err(error);
        }
    };

    // The source's programs stop here, so every session file is complete. The
    // workspace itself stays, for this client to mark as teleported.
    report(Step::Retire);
    retire(source, &review.checkout, cancelled)?;
    core::stop(from, &core::ids(&source.core_tabs), cancelled)?;

    report(Step::Sessions);
    let mut launches = Vec::new();
    for (plan, target) in review.panes().zip(&panes) {
        let line = match &plan.action {
            Action::Shell | Action::Missing(_) => None,
            Action::Resume(agent) => {
                let Work::Session { session, flags, .. } = &plan.work else {
                    continue;
                };
                let route = Route {
                    source: from,
                    destination: to,
                    from: &review.checkout,
                    to: &landing.checkout,
                    cwd: &target.cwd,
                };
                Some(match move_session(*agent, session, &route, cancelled) {
                    Ok(reference) => agent.resume_argv(&reference, flags),
                    Err(Error::Cancelled) => return Err(Error::Cancelled),
                    Err(error) => {
                        warnings.push(Warning::AgentStartedAfresh {
                            agent: *agent,
                            source: error,
                        });
                        agent.start_argv(None)
                    }
                })
            }
            Action::Handoff { note, to: agent } => {
                let note = format!("{}/{note}", landing.checkout);
                Some(agent.start_argv(Some(&handoff_resume_prompt(&note))))
            }
            Action::Start(argv) | Action::Run(argv) => {
                Some(remap_argv(argv, &review.checkout, &landing.checkout))
            }
        };
        if let Action::Missing(program) = &plan.action {
            warnings.push(Warning::ProgramMissing {
                program: program.clone(),
                host: destination.place.label.clone(),
            });
        }
        if let Some(argv) = line {
            launches.push((target, argv));
        }
    }

    if review.github == GitHubAccess::Token {
        report(Step::Credentials);
        // Pull and push matter, but not more than the move: failures warn.
        let installed = credentials::local_token(cancelled).and_then(|token| {
            let token = token.ok_or(Error::InvalidToken)?;
            credentials::install(to, &landing.checkout, &token, cancelled)
        });
        match installed {
            Ok(credentials::Installed::Complete) => {}
            Ok(credentials::Installed::WithoutEnvrc) => warnings.push(Warning::EnvrcTracked),
            Err(Error::Cancelled) => return Err(Error::Cancelled),
            Err(error) => warnings.push(Warning::GitHubAccess { source: error }),
        }
    }

    report(Step::Launch);
    for (target, argv) in launches {
        let mut line = command_line(&argv);
        if target.cwd != target.created_in {
            line = format!("cd -- {} && {line}", shell_quote(&target.cwd));
        }
        match target.generation {
            Generation::Herdr1 => to.herdr_ok(
                Step::Launch,
                &["pane", "run", &target.pane_id, &line],
                cancelled,
            )?,
            Generation::Herdr2 => core::type_line(to, &target.pane_id, &line, cancelled)?,
        }
    }

    spawned.keep();
    Ok(Outcome {
        endpoint_id: destination.place.endpoint_id.clone(),
        workspace: landing.workspace,
        repo_key: repository.key.clone(),
        branch: review.branch.clone(),
        warnings,
        core_tabs,
    })
}

/// The destination repository, opened as a space: as it is when already
/// open, else after opening an existing checkout or cloning one.
pub(super) fn arrive(
    source: &Source,
    destination: &Candidate,
    report: &mut impl FnMut(Step),
    cancelled: &AtomicBool,
) -> Result<Repository> {
    let to = &destination.place.host;
    let (path, key) = match &destination.destination {
        Destination::Open { repository, .. } | Destination::Reclaim { repository, .. } => {
            return Ok(repository.clone());
        }
        Destination::Arrive(Arrival::Existing { path, key }) => (path, key.clone()),
        Destination::Arrive(Arrival::Clone { path }) => {
            report(Step::Clone);
            let key = provision::clone(
                &source.place.host,
                &source.repo_key,
                to,
                path,
                destination.origin.as_deref(),
                cancelled,
            )?;
            (path, key)
        }
    };
    // Where the app keeps the workspaces, a worktree is made with Git from
    // the repository alone: no workspace needs to hold its main checkout.
    if destination.lands_in == Keeper::App {
        return Ok(Repository {
            key,
            label: source.repo_label.clone(),
            workspace_id: String::new(),
        });
    }
    report(Step::Open);
    let opened: WorkspaceCreated = to.herdr(
        Step::Open,
        &["workspace", "create", "--cwd", path, "--no-focus"],
        cancelled,
    )?;
    Ok(Repository {
        key,
        label: source.repo_label.clone(),
        workspace_id: opened.workspace.workspace_id,
    })
}

/// A new workspace on the destination repository's main checkout, cloning
/// the repository first where it is missing.
pub(super) fn open_workspace(
    source: &Source,
    destination: &Candidate,
    label: Option<&str>,
    report: &mut impl FnMut(Step),
    cancelled: &AtomicBool,
) -> Result<String> {
    let to = &destination.place.host;
    let path = match &destination.destination {
        Destination::Open { repository, .. } | Destination::Reclaim { repository, .. } => {
            git::main_checkout(to, &repository.key, cancelled)?
        }
        Destination::Arrive(Arrival::Existing { path, .. }) => path.clone(),
        Destination::Arrive(Arrival::Clone { path }) => {
            report(Step::Clone);
            provision::clone(
                &source.place.host,
                &source.repo_key,
                to,
                path,
                destination.origin.as_deref(),
                cancelled,
            )?;
            path.clone()
        }
    };
    report(Step::Open);
    let mut args = vec!["workspace", "create", "--cwd", &path, "--no-focus"];
    if let Some(label) = label {
        args.extend(["--label", label]);
    }
    let opened: WorkspaceCreated = to.herdr(Step::Open, &args, cancelled)?;
    Ok(opened.workspace.workspace_id)
}

/// A destination pane standing in for a source pane: a Herdr 1 pane, or a
/// Herdr 2 terminal.
#[derive(Debug, Clone)]
struct Target {
    generation: Generation,
    pane_id: String,
    /// Where its program should run, and where its shell started.
    cwd: String,
    created_in: String,
}

/// Rebuild the source tabs in `landing`, then close the tabs it replaces.
/// Returns one target per pane in `review.panes()` order, and the pane trees
/// of the tabs rebuilt in Herdr 2, whose terminals are also in `spawned`.
fn rebuild_tabs(
    to: &Host,
    landing: &Landing,
    review: &Review,
    spawned: &mut core::Spawned<'_>,
    report: &mut impl FnMut(Step),
    cancelled: &AtomicBool,
) -> Result<(Vec<Target>, Vec<Panes>)> {
    report(Step::Tabs);
    let checkout = &landing.checkout;
    let cwd_of = |plan: Option<&PanePlan>| {
        plan.and_then(|plan| plan.cwd.as_deref())
            .and_then(|cwd| remap(cwd, &review.checkout, checkout))
            .unwrap_or_else(|| checkout.clone())
    };
    let mut targets: HashMap<String, Target> = HashMap::new();
    let mut core_tabs = Vec::new();
    // The first tab rebuilt in Herdr 1 takes the landing's empty tab.
    let mut first = landing.first.clone();
    for tab in &review.tabs {
        let build = tab.tree.plan();
        if tab.into == Generation::Herdr2 {
            let mut terminals = HashMap::new();
            for source_pane in &build.slots {
                let cwd = cwd_of(tab.pane(source_pane));
                // A start is not cancelled midway, so every terminal the
                // core starts is known to `spawned`, which ends them.
                let terminal = core::start_shell(to, &cwd, &AtomicBool::new(false))?;
                spawned.push(terminal.clone());
                if cancelled.load(Ordering::Acquire) {
                    return Err(Error::Cancelled);
                }
                terminals.insert(source_pane.clone(), terminal.clone());
                let target = Target {
                    generation: Generation::Herdr2,
                    pane_id: terminal,
                    cwd: cwd.clone(),
                    created_in: cwd,
                };
                targets.insert(source_pane.clone(), target);
            }
            core_tabs.push(
                core::panes(&tab.tree, &terminals).ok_or_else(|| Error::Decode {
                    step: Step::Tabs,
                    source: serde::de::Error::custom("a Herdr 2 tab could not be laid out"),
                })?,
            );
            continue;
        }
        let first_cwd = cwd_of(build.slots.first().and_then(|id| tab.pane(id)));
        let (tab_id, root, root_cwd) = match first.take() {
            Some((tab_id, root)) => (tab_id, root, checkout.clone()),
            None => {
                let made: TabCreated = to.herdr(
                    Step::Tabs,
                    &[
                        "tab",
                        "create",
                        "--workspace",
                        landing.herdr1.as_deref().ok_or_else(|| Error::Decode {
                            step: Step::Tabs,
                            source: serde::de::Error::custom(
                                "a Herdr 1 tab cannot open in a workspace the app keeps",
                            ),
                        })?,
                        "--cwd",
                        &first_cwd,
                        "--no-focus",
                    ],
                    cancelled,
                )?;
                (made.tab.tab_id, made.root_pane.pane_id, first_cwd.clone())
            }
        };
        if let Some(label) = &tab.label {
            to.herdr_ok(Step::Tabs, &["tab", "rename", &tab_id, label], cancelled)?;
        }
        let mut slots = vec![(root, root_cwd)];
        for step in &build.steps {
            let cwd = cwd_of(build.slots.get(step.created).and_then(|id| tab.pane(id)));
            let ratio = format!("{:.4}", step.ratio);
            let target = &slots[step.target].0;
            let made: PaneInfoResult = to.herdr(
                Step::Tabs,
                &[
                    "pane",
                    "split",
                    target,
                    "--direction",
                    step.direction.as_str(),
                    "--ratio",
                    &ratio,
                    "--cwd",
                    &cwd,
                    "--no-focus",
                ],
                cancelled,
            )?;
            slots.push((made.pane.pane_id, cwd));
        }
        for (source_pane, (pane_id, created_in)) in build.slots.iter().zip(slots) {
            targets.insert(
                source_pane.clone(),
                Target {
                    generation: Generation::Herdr1,
                    pane_id,
                    cwd: cwd_of(tab.pane(source_pane)),
                    created_in,
                },
            );
        }
    }
    for tab in &landing.stale_tabs {
        to.herdr_ok(Step::Tabs, &["tab", "close", tab], cancelled)?;
    }
    let targets = review
        .panes()
        .map(|plan| {
            targets
                .get(&plan.pane_id)
                .cloned()
                .ok_or_else(|| Error::Decode {
                    step: Step::Tabs,
                    source: serde::de::Error::custom("a pane was not recreated"),
                })
        })
        .collect::<Result<_>>()?;
    Ok((targets, core_tabs))
}
