//! The blocking work behind a fan-out. Every function here runs host scripts
//! and must only be called from a background worker, never the UI thread.
//!
//! Worktrees and agents are made through `sheprd_remote::work`, so a lane
//! is the same new work on Herdr 1, through its CLI, and on Herdr 2, through
//! Git and a herdr-core shell.

use super::{
    error::{Error, Result, Step, script},
    journal::Resume,
    plan::{DiffStat, Lane, parse_stats, stats_script},
};
use crate::teleport::{self, AgentKind, FreshOrigin, Host, HostRepositories, Prepared};
use sheprd_remote::work;
use std::sync::atomic::{AtomicBool, Ordering};

/// What a fan-out launches.
#[derive(Debug, Clone)]
pub(crate) struct Request {
    /// The repository lanes branch from, through its main checkout.
    pub(crate) origin: FreshOrigin,
    /// What makes the origin's lanes; see [`super::Origin::repository`].
    pub(crate) repository: work::Repository,
    /// The ref every lane branches from, resolved once on the origin so
    /// every lane, on every host, starts from the same commit.
    pub(crate) base: String,
    pub(crate) prompt: String,
    pub(crate) lanes: Vec<Lane>,
    /// Where each lane runs, in lane order: `None` on the origin, else the
    /// other host, which is set up once before its first lane.
    pub(crate) hosts: Vec<Option<HostRepositories>>,
}

/// A lane's new worktree, and the host it is on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Checkout {
    pub(crate) endpoint_id: String,
    pub(crate) host: Host,
    /// Its folder, and Herdr 1's workspace and pane when Herdr 1 made it.
    pub(crate) worktree: work::Worktree,
    /// On Herdr 2, the herdr-core shell its agent was typed into, once that
    /// started; Herdr 1's agent runs in the worktree's own pane.
    pub(crate) terminal: Option<String>,
}

impl Checkout {
    pub(crate) fn path(&self) -> &str {
        &self.worktree.path
    }

    /// Herdr 1's workspace for it; a Herdr 2 checkout's workspace is one
    /// the app keeps, found by its folder.
    pub(crate) fn workspace_id(&self) -> Option<&str> {
        match &self.worktree.opened {
            work::Opened::Herdr1 { workspace_id, .. } => Some(workspace_id),
            work::Opened::Herdr2 => None,
        }
    }

    /// Where its agent runs, once known.
    pub(super) fn started(&self) -> Option<work::Started> {
        match &self.worktree.opened {
            work::Opened::Herdr1 { pane_id, .. } => Some(work::Started::Herdr1 {
                pane_id: pane_id.clone(),
            }),
            work::Opened::Herdr2 => self
                .terminal
                .clone()
                .map(|terminal| work::Started::Herdr2 { terminal }),
        }
    }
}

/// How far one lane has got.
#[derive(Debug)]
pub(crate) enum Progress {
    /// Its host is getting the repository and the base commit.
    SettingUp,
    CreatingWorktree,
    Created(Box<Checkout>),
    StartingAgent,
    /// The herdr-core shell a Herdr 2 lane's agent was typed into.
    Terminal(String),
    Prompting,
    /// The agent took the prompt and is working on it.
    Running,
    Failed(Error),
}

#[derive(Debug)]
pub(crate) enum Report {
    /// The commit every lane branches from and is compared against.
    Base(String),
    Lane(usize, Progress),
}

/// Where lanes on one host are created through.
#[derive(Clone)]
struct Site {
    endpoint_id: String,
    label: String,
    host: Host,
    /// What makes its worktrees: Herdr 1's workspace of the repository, or
    /// on Herdr 2 Git through a checkout of it.
    repository: work::Repository,
    /// The agents installed on another host. The origin's were listed
    /// before anything was picked.
    agents: Option<Vec<AgentKind>>,
}

/// Another host set up for its lanes, whose shipped commit's reference is
/// dropped once every lane is created.
struct SetUp {
    endpoint_id: String,
    result: Option<(Site, Prepared)>,
}

/// Each lane's site, setting up every other host once: the repository and
/// the base commit put there. A host that cannot be set up fails its lanes,
/// the first with the cause. Returns the sites, `None` for a failed lane,
/// and the hosts set up.
fn sites(
    request: &Request,
    commit: &str,
    report: &impl Fn(Report) -> Result<()>,
    cancelled: &AtomicBool,
) -> (Vec<Option<Site>>, Vec<SetUp>) {
    let origin = &request.origin;
    let mut set_up: Vec<SetUp> = Vec::new();
    let mut sites = Vec::with_capacity(request.lanes.len());
    for (index, host) in request.hosts.iter().enumerate() {
        let Some(destination) = host else {
            sites.push(Some(Site {
                endpoint_id: origin.place.endpoint_id.clone(),
                label: origin.place.label.clone(),
                host: origin.place.host.clone(),
                repository: request.repository.clone(),
                agents: None,
            }));
            continue;
        };
        let endpoint = &destination.place.endpoint_id;
        if let Some(known) = set_up.iter().find(|known| &known.endpoint_id == endpoint) {
            let site = known.result.as_ref().map(|(site, _)| site.clone());
            if site.is_none() {
                let _ = report(Report::Lane(
                    index,
                    Progress::Failed(Error::HostUnavailable {
                        host: destination.place.label.clone(),
                    }),
                ));
            }
            sites.push(site);
            continue;
        }
        let _ = report(Report::Lane(index, Progress::SettingUp));
        let result = match set_up_host(origin, destination, commit, cancelled) {
            Ok(result) => Some(result),
            Err(error) => {
                let _ = report(Report::Lane(index, Progress::Failed(error)));
                None
            }
        };
        sites.push(result.as_ref().map(|(site, _)| site.clone()));
        set_up.push(SetUp {
            endpoint_id: endpoint.clone(),
            result,
        });
    }
    (sites, set_up)
}

/// The repository and the base commit on `destination`, and the agents
/// installed there.
fn set_up_host(
    origin: &FreshOrigin,
    destination: &HostRepositories,
    commit: &str,
    cancelled: &AtomicBool,
) -> Result<(Site, Prepared)> {
    let host = &destination.place.host;
    let agents = installed(host, cancelled)?;
    let prepared = teleport::prepare(origin, destination, commit, &mut |_| {}, cancelled)
        .map_err(Error::Dispatch)?;
    let site = Site {
        endpoint_id: destination.place.endpoint_id.clone(),
        label: destination.place.label.clone(),
        host: host.clone(),
        repository: work::Repository::Herdr1 {
            workspace_id: prepared.repository.workspace_id.clone(),
        },
        agents: Some(agents),
    };
    Ok((site, prepared))
}

/// Resolve the base commit, set up every host, then create every lane's
/// worktree one at a time, since concurrent `git worktree add` runs contend
/// for a repository's ref locks, and start the agents side by side.
pub(crate) fn launch(
    request: &Request,
    report: &(impl Fn(Report) -> Result<()> + Sync),
    cancelled: &AtomicBool,
) {
    let commit = match teleport::base_commit(&request.origin, &request.base, cancelled) {
        Ok(commit) => commit,
        Err(error) => {
            let _ = report(Report::Lane(0, Progress::Failed(Error::Dispatch(error))));
            for index in 1..request.lanes.len() {
                let _ = report(Report::Lane(index, Progress::Failed(Error::NoBase)));
            }
            return;
        }
    };
    let _ = report(Report::Base(commit.clone()));
    let (sites, set_up) = sites(request, &commit, report, cancelled);
    let mut created = Vec::new();
    for (index, (lane, site)) in request.lanes.iter().zip(&sites).enumerate() {
        let Some(site) = site else {
            continue;
        };
        if cancelled.load(Ordering::Acquire) {
            let _ = report(Report::Lane(index, Progress::Failed(Error::Cancelled)));
            continue;
        }
        if site
            .agents
            .as_ref()
            .is_some_and(|agents| !agents.contains(&lane.kind))
        {
            let _ = report(Report::Lane(
                index,
                Progress::Failed(Error::AgentMissing {
                    agent: lane.kind.name(),
                    host: site.label.clone(),
                }),
            ));
            continue;
        }
        // A step not saved could be offered again after a crash, so its
        // script does not run.
        if let Err(error) = report(Report::Lane(index, Progress::CreatingWorktree)) {
            let _ = report(Report::Lane(index, Progress::Failed(error)));
            continue;
        }
        match create(site, lane, &commit, cancelled) {
            Ok(checkout) => {
                let _ = report(Report::Lane(
                    index,
                    Progress::Created(Box::new(checkout.clone())),
                ));
                created.push((index, lane, checkout));
            }
            Err(error) => {
                let _ = report(Report::Lane(index, Progress::Failed(error)));
            }
        }
    }
    // The new branches keep the shipped commit; its references can go.
    for (_, prepared) in set_up.into_iter().filter_map(|set_up| set_up.result) {
        prepared.finish(&AtomicBool::new(false));
    }
    std::thread::scope(|scope| {
        for (index, lane, checkout) in &created {
            let run = move || {
                let started = start(*index, lane, checkout, &request.prompt, report, cancelled);
                let progress = match started {
                    Ok(()) => Progress::Running,
                    Err(error) => Progress::Failed(error),
                };
                let _ = report(Report::Lane(*index, progress));
            };
            let spawned = std::thread::Builder::new()
                .name("herdr-fan-out-lane".into())
                .spawn_scoped(scope, run);
            if let Err(error) = spawned {
                tracing::warn!(%error, "could not start a fan-out lane worker");
                let _ = report(Report::Lane(
                    *index,
                    Progress::Failed(Error::Script {
                        step: Step::StartAgent,
                        source: sheprd_client::Error::ScriptSpawn(error),
                    }),
                ));
            }
        }
    });
}

fn create(site: &Site, lane: &Lane, commit: &str, cancelled: &AtomicBool) -> Result<Checkout> {
    let new = work::NewWorktree {
        branch: &lane.branch,
        base: commit,
        label: None,
    };
    let worktree = work::create(&site.host, &site.repository, &new, cancelled).map_err(failed)?;
    Ok(Checkout {
        endpoint_id: site.endpoint_id.clone(),
        host: site.host.clone(),
        worktree,
        terminal: None,
    })
}

/// A cancelled script is the fan-out's own cancellation.
fn failed(error: work::Error) -> Error {
    match error {
        work::Error::Cancelled => Error::Cancelled,
        error => Error::Work(Box::new(error)),
    }
}

/// Start the lane's agent in its first pane, or on Herdr 2 a herdr-core
/// shell in its folder, and once it is ready send it the prompt.
fn start(
    index: usize,
    lane: &Lane,
    checkout: &Checkout,
    prompt: &str,
    report: &impl Fn(Report) -> Result<()>,
    cancelled: &AtomicBool,
) -> Result<()> {
    // Unsaved, a crash after the agent starts would offer to start it again.
    report(Report::Lane(index, Progress::StartingAgent))?;
    let agent = work::Agent {
        kind: lane.kind,
        name: &lane.agent,
        model: None,
        prompt: None,
    };
    let started =
        work::start_agent(&checkout.host, &checkout.worktree, &agent, cancelled).map_err(failed)?;
    if let work::Started::Herdr2 { terminal } = &started {
        // The shell is kept even if this cannot be saved: it is started.
        let _ = report(Report::Lane(index, Progress::Terminal(terminal.clone())));
    }
    send_prompt(index, checkout, &started, prompt, report, cancelled)
}

fn send_prompt(
    index: usize,
    checkout: &Checkout,
    started: &work::Started,
    prompt: &str,
    report: &impl Fn(Report) -> Result<()>,
    cancelled: &AtomicBool,
) -> Result<()> {
    report(Report::Lane(index, Progress::Prompting))?;
    work::prompt(&checkout.host, started, prompt, cancelled).map_err(failed)
}

/// Finish lane `index` of an interrupted fan-out, which the user asked for:
/// start its agent first when `from` says so, then send the prompt.
pub(crate) fn resume(
    from: Resume,
    index: usize,
    lane: &Lane,
    checkout: &Checkout,
    prompt: &str,
    report: &impl Fn(Report) -> Result<()>,
    cancelled: &AtomicBool,
) {
    let result = match (from, checkout.started()) {
        (Resume::Prompt, Some(started)) => {
            send_prompt(index, checkout, &started, prompt, report, cancelled)
        }
        // Missing persistence is not evidence that the agent never ran.
        (Resume::Prompt, None) => Err(Error::AgentUncertain),
        (Resume::Start, _) => start(index, lane, checkout, prompt, report, cancelled),
    };
    let progress = match result {
        Ok(()) => Progress::Running,
        Err(error) => Progress::Failed(error),
    };
    let _ = report(Report::Lane(index, progress));
}

/// The agent kinds installed on `host`, in [`AgentKind::ALL`] order.
pub(crate) fn installed(host: &Host, cancelled: &AtomicBool) -> Result<Vec<AgentKind>> {
    let binaries = AgentKind::ALL.map(AgentKind::binary);
    let found = host
        .installed_programs(&binaries, cancelled)
        .map_err(script(Step::Detect))?;
    Ok(AgentKind::ALL
        .into_iter()
        .filter(|kind| found.iter().any(|binary| binary == kind.binary()))
        .collect())
}

/// One host's part of a comparison: the lanes on it, and their changes
/// since the base, `None` for a checkout that is gone, in the same order.
#[derive(Debug)]
pub(crate) struct HostStats {
    pub(crate) lanes: Vec<usize>,
    pub(crate) result: Result<Vec<Option<DiffStat>>>,
}

/// Each lane's changes since `base`, reading every host once. A host that
/// cannot be read fails only its own lanes.
pub(crate) fn stats(
    checkouts: &[Option<(Host, String)>],
    base: &str,
    cancelled: &AtomicBool,
) -> Vec<HostStats> {
    let mut hosts: Vec<(&Host, Vec<usize>)> = Vec::new();
    for (index, (host, _)) in checkouts
        .iter()
        .enumerate()
        .filter_map(|(index, checkout)| Some((index, checkout.as_ref()?)))
    {
        match hosts.iter_mut().find(|(known, _)| *known == host) {
            Some((_, lanes)) => lanes.push(index),
            None => hosts.push((host, vec![index])),
        }
    }
    hosts
        .into_iter()
        .map(|(host, lanes)| {
            let paths: Vec<String> = lanes
                .iter()
                .filter_map(|&index| Some(checkouts[index].as_ref()?.1.clone()))
                .collect();
            let result = host
                .capture(&stats_script(&paths, base), cancelled)
                .map_err(script(Step::Compare))
                .map(|output| parse_stats(&String::from_utf8_lossy(&output), paths.len()));
            HostStats { lanes, result }
        })
        .collect()
}

/// Remove a lane's worktree, and Herdr 1's workspace with it, discarding
/// uncommitted changes. The branch stays, so committed work stays
/// reachable; the app closes a Herdr 2 lane's kept workspace itself.
pub(crate) fn remove(
    host: &Host,
    worktree: &work::Worktree,
    repo_key: &str,
    branch: &str,
    cancelled: &AtomicBool,
) -> Result<()> {
    if matches!(worktree.opened, work::Opened::Herdr2) {
        if !host.is_local() {
            return Err(Error::HostUnavailable {
                host: host.identity(),
            });
        }
        // A saved folder may now belong to another branch or repository.
        // Check Git itself on this worker before any forced removal, even
        // for a live lane: cached UI metadata cannot authorize discarding it.
        let input = crate::pull_request::Input {
            checkout: Some(worktree.path.clone()),
            repo_key: Some(repo_key.to_owned()),
            branch: branch.to_owned(),
        };
        crate::pull_request::local_checkout(
            &input,
            std::time::Instant::now() + std::time::Duration::from_secs(15),
            &|| cancelled.load(Ordering::Acquire),
        )?;
    }
    work::discard(host, worktree, cancelled).map_err(failed)
}
