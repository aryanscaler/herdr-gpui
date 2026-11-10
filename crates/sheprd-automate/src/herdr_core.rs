//! Driving herdr-core (Herdr 2) over its socket, for hosts whose terminals
//! it runs. It keeps no workspaces or worktrees, so an agent step makes its
//! checkout where Herdr 1 would and starts the agent there; the agent's
//! terminal is its "pane". The daemon joins a running core and starts one
//! only when none answers, as it starts Herdr 1's server; it never stops or
//! upgrades it.

use sheprd_client::{
    herdr::{AgentKind, worktree},
    login_env,
};
use sheprd_core_client::{
    Core, Endpoint,
    agent::{self, Until},
    wire::{Run, Terminal},
};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, PoisonError},
    time::{Duration, Instant},
};

/// How long a core the daemon started has to listen.
const START_TIMEOUT: Duration = Duration::from_secs(20);
/// How long a just-started core may refuse its first dial.
const DIAL_RETRY: Duration = Duration::from_secs(5);
/// The label naming the automation run that started a terminal, so the app
/// can tell its agents apart from the user's own.
pub const RUN_LABEL: &str = "sheprd.automation";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Core(#[from] sheprd_core_client::Error),
    #[error(
        "herdr-core is not running and no herdr-core program was found; open Sheprd once, or set herdr_core in config.toml"
    )]
    NotFound,
    #[error("this host has no home folder for new worktrees")]
    NoHome,
    #[error("herdr-core lists no terminal {0}")]
    NoTerminal(String),
    #[error(transparent)]
    Worktree(#[from] worktree::Error),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// One host's herdr-core, joined on first use and again after a loss.
#[derive(Clone, Default)]
pub struct HerdrCore {
    /// The `herdr-core` program to start; found on the usual paths if not.
    program: Option<PathBuf>,
    joined: Arc<Mutex<Option<Arc<Core>>>>,
}

impl std::fmt::Debug for HerdrCore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HerdrCore")
            .field("program", &self.program)
            .finish_non_exhaustive()
    }
}

/// An agent to start in a new worktree.
pub struct AgentStart<'a> {
    pub repo: &'a Path,
    pub branch: &'a str,
    pub base: Option<&'a str>,
    pub harness: &'a str,
    /// The run it belongs to, kept as [`RUN_LABEL`].
    pub label: &'a str,
}

/// A started agent: its checkout and its terminal's address.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Started {
    pub checkout: PathBuf,
    pub terminal: String,
}

impl HerdrCore {
    pub fn new(program: Option<PathBuf>) -> Self {
        Self {
            program,
            joined: Arc::default(),
        }
    }

    /// The core this host runs, started first if none answers. Blocking.
    pub fn core(&self) -> Result<Arc<Core>> {
        let mut joined = self.joined.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(core) = joined.as_ref().filter(|core| !core.is_lost()) {
            return Ok(core.clone());
        }
        // The app reads `HC_SESSION` and `HC_SOCKET` from the login shell, and
        // a service keeps few of its variables, so the daemon reads them the
        // same way to join the core whose terminals the app opens.
        let endpoint = Endpoint::resolve(login_env::var)?;
        let core = match Core::connect(&endpoint) {
            Ok(core) => core,
            Err(sheprd_core_client::Error::Connect { .. }) => {
                let program = self.program.clone().or_else(find).ok_or(Error::NotFound)?;
                sheprd_core_client::start(&program, login_env::apply, START_TIMEOUT)?;
                dial(&endpoint)?
            }
            Err(error) => return Err(error.into()),
        };
        let core = Arc::new(core);
        *joined = Some(core.clone());
        Ok(core)
    }

    /// A new worktree of `repo` on `branch`, from `base` or what it has
    /// checked out. Answers the checkout. Git is killed at the step's stop
    /// or deadline, so a hook that hangs cannot hold a stopped run.
    pub fn create_worktree(
        repo: &Path,
        branch: &str,
        base: Option<&str>,
        until: Until<'_>,
    ) -> Result<PathBuf> {
        let root = worktree::root().ok_or(Error::NoHome)?;
        let give_up = || until.check().is_err();
        Ok(worktree::add(
            repo,
            branch,
            base.unwrap_or("HEAD"),
            &root,
            give_up,
        )?)
    }

    /// A new worktree, and `harness` started in it, unless the step was
    /// stopped or ran out of time first. A worktree whose agent did not
    /// start stays, as Herdr 1's does: the user can see what happened there.
    pub fn start_agent(&self, start: &AgentStart<'_>, until: Until<'_>) -> Result<Started> {
        let core = self.core()?;
        let checkout = Self::create_worktree(start.repo, start.branch, start.base, until)?;
        until.check()?;
        let argv = argv(start.harness);
        let cwd = checkout.to_string_lossy();
        let run = Run {
            argv: &argv,
            cwd: Some(&cwd),
            // A lost agent's terminal stays with its output, for the user to read.
            keep: true,
            labels: BTreeMap::from([(RUN_LABEL, start.label)]),
            ..Run::default()
        };
        let terminal = core.run(&run)?.address;
        Ok(Started { checkout, terminal })
    }

    /// Sends `prompt` once the agent in `terminal` is ready; with `wait`,
    /// returns once it is done with it. Stopping ends the wait, not the
    /// agent, which keeps its terminal as any agent the daemon started does.
    pub fn prompt(&self, terminal: &str, prompt: &str, wait: bool, until: Until<'_>) -> Result<()> {
        let core = self.core()?;
        let sent = agent::prompt(&core, terminal, prompt, until)?;
        if wait {
            agent::finish(&core, terminal, sent.turn, until)?;
        }
        Ok(())
    }

    /// Every terminal the core keeps, finished ones too, for sampling.
    pub fn terminals(&self) -> Result<Vec<Terminal>> {
        Ok(self.core()?.list(true)?.terminals)
    }
}

/// The agent the herdr-core terminal `terminal` runs, as Sheprd names it,
/// asked of the core it runs in (`HC_TERMINAL_SOCKET`, else the one `hc`
/// reaches); `None` for a shell or another program.
pub fn terminal_agent(terminal: &str) -> Result<Option<String>> {
    let endpoint = Endpoint::resolve(|name| {
        let terminal_socket =
            std::env::var_os("HC_TERMINAL_SOCKET").filter(|socket| !socket.is_empty());
        match name {
            "HC_SOCKET" => terminal_socket.or_else(|| std::env::var_os(name)),
            _ => std::env::var_os(name),
        }
    })?;
    let listed = Core::connect(&endpoint)?.list(false)?;
    let found = listed
        .terminals
        .iter()
        .find(|known| known.address == terminal)
        .ok_or_else(|| Error::NoTerminal(terminal.to_owned()))?;
    let info = sheprd_core_client::terminal_info(found)?;
    Ok(sheprd_core_client::agent_name(&info))
}

/// How a step's harness starts: a known agent's own program, else the
/// harness as a command.
fn argv(harness: &str) -> Vec<String> {
    AgentKind::parse(harness).map_or_else(|| vec![harness.to_owned()], |kind| kind.start_argv(None))
}

/// Dials a core that `server start` just reported listening, which a slow
/// file system can still refuse for a moment.
fn dial(endpoint: &Endpoint) -> Result<Core> {
    let deadline = Instant::now() + DIAL_RETRY;
    loop {
        match Core::connect(endpoint) {
            Ok(core) => return Ok(core),
            Err(sheprd_core_client::Error::Connect { .. }) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(error) => return Err(error.into()),
        }
    }
}

/// The first `herdr-core` on `PATH`, where installers put it, or beside
/// this executable, as a release bundles it.
fn find() -> Option<PathBuf> {
    let name = sheprd_core_client::PROGRAM;
    let path = login_env::path()
        .map(|path| std::env::split_paths(&path).collect::<Vec<_>>())
        .unwrap_or_default();
    let home = sheprd_client::local_path::home_dir();
    let usual = home
        .iter()
        .flat_map(|home| [home.join(".local/bin"), home.join(".cargo/bin")])
        .chain(
            [
                "/opt/homebrew/bin",
                "/usr/local/bin",
                "/home/linuxbrew/.linuxbrew/bin",
            ]
            .map(PathBuf::from),
        );
    let beside = std::env::current_exe()
        .ok()
        .and_then(|exe| std::fs::canonicalize(exe).ok())
        .and_then(|exe| exe.parent().map(Path::to_owned));
    path.into_iter()
        .filter(|dir| dir.is_absolute())
        .chain(usual)
        .chain(beside)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

#[cfg(all(test, unix))]
#[allow(clippy::unwrap_used)]
mod tests;
