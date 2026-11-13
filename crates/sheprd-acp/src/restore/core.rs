//! Explicit recovery in the shell core kept for this chat. Never types a
//! command into another process, and never searches for a likely session.

use std::{
    fs::{File, OpenOptions},
    path::{Path, PathBuf},
};

use anyhow::{Context as _, ensure};
use sheprd_chat_protocol::{ENDPOINT_ENV, EndpointId, restore};

use crate::cli::{AgentChoice, Options};

use sheprd_core_client::{Core, Endpoint, chat_recovery};

fn connect(socket: PathBuf) -> sheprd_core_client::Result<Core> {
    Core::connect(&Endpoint {
        socket,
        session: "default".to_owned(),
    })
}

/// The recovery generation a replacement launch claimed. Only a launch that
/// holds one registers its agent child, so an explicit restore in a shell that
/// inherited a replacement's environment never touches the recovery state.
#[derive(Debug)]
pub(crate) struct Claimed {
    generation: u32,
}

/// A replacement started by a desktop/phone worker must claim its durable
/// generation before any agent starts. Inherited caller HC_SOCKET is ignored.
pub(crate) fn claim_launch() -> anyhow::Result<Option<Claimed>> {
    if crate::herdr::Target::from_env().is_some() {
        return Ok(None);
    }
    let Some(generation) = std::env::var_os(chat_recovery::GENERATION_ENV) else {
        return Ok(None);
    };
    if generation.is_empty() {
        return Ok(None);
    }
    let generation = generation
        .to_str()
        .context("invalid recovery generation")?
        .parse::<u32>()?;
    let terminal = std::env::var("HC_TERMINAL_ID")?;
    let socket = std::env::var_os("HC_TERMINAL_SOCKET").context("no terminal socket")?;
    let endpoint = std::env::var(ENDPOINT_ENV)?.parse::<EndpointId>()?;
    chat_recovery::claim(
        &connect(socket.into())?,
        &endpoint,
        &terminal,
        generation,
        old_child_gone,
    )?;
    Ok(Some(Claimed { generation }))
}

/// Proof, without signalling anything, that the previous chat's agent and its
/// process group are gone. Doubt refuses the claim, and a refused generation
/// is never relaunched: the chat stays unrecovered rather than risk two agents
/// on one session. Nothing here kills or adopts a process from a stored PID.
fn old_child_gone(
    child: &sheprd_client::herdr::sessions::Identity,
) -> sheprd_core_client::Result<bool> {
    agent_gone(child.pid, &child.started)
}

fn child_identity(
    core: &Core,
    terminal: &str,
    pid: u32,
) -> anyhow::Result<sheprd_client::herdr::sessions::Identity> {
    let started = sheprd_core_client::sessions::process_birth(pid)?
        .context("agent child identity is unavailable")?;
    Ok(sheprd_client::herdr::sessions::Identity {
        core: core.hello().core.id.clone(),
        terminal: terminal.to_owned(),
        pid,
        started,
    })
}

/// Records the agent child of the launch that claimed `claimed`.
pub(crate) fn child_started(claimed: &Claimed, pid: u32) -> anyhow::Result<()> {
    let terminal = std::env::var("HC_TERMINAL_ID")?;
    let socket = std::env::var_os("HC_TERMINAL_SOCKET").context("no terminal socket")?;
    let endpoint = std::env::var(ENDPOINT_ENV)?.parse::<EndpointId>()?;
    let core = connect(socket.into())?;
    chat_recovery::child_started(
        &core,
        &endpoint,
        claimed.generation,
        child_identity(&core, &terminal, pid)?,
    )?;
    Ok(())
}

pub(super) fn opened(keeper: &super::Keeper, session: &str) -> anyhow::Result<()> {
    let core = connect(keeper.socket.clone())?;
    let mut env: std::collections::BTreeMap<String, String> =
        match std::env::var(chat_recovery::OVERRIDES_ENV) {
            Ok(encoded) if encoded.len() <= 16 * 1024 => serde_json::from_str(&encoded)?,
            Ok(_) => anyhow::bail!("chat environment overrides exceed the recovery bound"),
            Err(std::env::VarError::NotPresent) => std::collections::BTreeMap::new(),
            Err(error) => return Err(error.into()),
        };
    if let Some(agent) = sheprd_client::herdr::AgentKind::parse(keeper.agent.name()) {
        for key in sheprd_client::herdr::sessions::environment_keys(agent) {
            if let Ok(value) = std::env::var(key) {
                env.insert((*key).to_owned(), value);
            }
        }
    }
    chat_recovery::opened(
        &core,
        &keeper.endpoint,
        &keeper.pane,
        chat_recovery::Session {
            adapter: keeper.adapter,
            args: keeper.args.clone(),
            agent: keeper.agent,
            child: child_identity(
                &core,
                &keeper.pane,
                keeper
                    .child
                    .context("agent child identity is unavailable")?,
            )?,
            cwd: keeper
                .cwd
                .to_str()
                .context("chat cwd is not UTF-8")?
                .to_owned(),
            env,
            program: std::env::current_exe()?
                .to_str()
                .context("chat program path is not UTF-8")?
                .to_owned(),
            session: session.to_owned(),
        },
    )?;
    Ok(())
}

pub(super) fn closed(keeper: &super::Keeper) -> anyhow::Result<()> {
    chat_recovery::closed(&connect(keeper.socket.clone())?, &keeper.endpoint)?;
    Ok(())
}

/// Held for the entire lifetime of a core chat, including agent startup.
/// Unlike a timed claim, an OS lock cannot expire while a slow starter is
/// still running. The lock file is never unlinked (which would split owners).
pub(crate) fn lock() -> anyhow::Result<Option<File>> {
    if crate::herdr::Target::from_env().is_some() || std::env::var_os("HC_TERMINAL_ID").is_none() {
        return Ok(None);
    }
    let Some(endpoint) = std::env::var_os(ENDPOINT_ENV) else {
        return Ok(None);
    };
    let endpoint: EndpointId = endpoint
        .to_str()
        .context("chat endpoint is not UTF-8")?
        .parse()
        .context("invalid chat endpoint")?;
    let dir = restore::restore_dir()
        .context("no chat state directory")?
        .join("core");
    lock_in(&dir, &endpoint).map(Some)
}

fn lock_in(dir: &Path, endpoint: &EndpointId) -> anyhow::Result<File> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(dir)?;
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let file = options.open(dir.join(format!("{endpoint}.lock")))?;
    file.try_lock().context("this chat already has an owner")?;
    Ok(file)
}

pub(crate) fn run(endpoint: EndpointId) -> anyhow::Result<()> {
    let _owner = lock()?.context("restore must run inside the chat's kept core terminal")?;
    let dir = restore::restore_dir()
        .context("no chat state directory")?
        .join("core");
    let record = restore::Record::read(&restore::record_path(&dir, &endpoint))?;
    let cwd = std::env::current_dir()?.canonicalize()?;
    ensure!(
        matches_context(&record, &endpoint, &cwd, |name| std::env::var_os(name)),
        "this is not the terminal, endpoint, socket and folder that kept the chat"
    );
    // The endpoint lock proves the old chat program gone, not the agent it
    // started: that runs in a process group of its own, as may its tools.
    ensure!(
        previous_agent_gone(&dir, &endpoint)?,
        "the chat's previous agent, or a tool it started, is still running; end it before restoring"
    );
    // Never a replacement child: the kept shell may still carry the
    // generation of the replacement whose agent was lost.
    crate::app::run_owned(
        None,
        Options {
            adapter: record.adapter,
            agent: AgentChoice::Builtin {
                agent: record.agent,
                args: Vec::new(),
            },
            attach: true,
            cwd: Some(record.cwd),
            headless: false,
            load: Some(record.session),
            name: None,
            require_load: true,
        },
    )
}

/// The agent a core chat started, kept beside its restore record so an
/// explicit restore can refuse while it, or a tool in its group, lives on.
#[derive(Debug, PartialEq, serde::Deserialize, serde::Serialize)]
struct Started {
    pid: u32,
    /// The OS birth token, so a reused PID is never mistaken for it.
    started: String,
}

fn started_path(dir: &Path, endpoint: &EndpointId) -> PathBuf {
    dir.join(format!("{endpoint}.agent"))
}

/// Records the agent `pid` a core chat just started, readable only by this
/// user. Without it a later restore refuses, so a failure is only reported.
pub(crate) fn agent_started(dir: &Path, endpoint: &EndpointId, pid: u32) -> anyhow::Result<()> {
    let started = Started {
        pid,
        started: sheprd_core_client::sessions::process_birth(pid)?
            .context("the agent ended as it started")?,
    };
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    serde_json::to_writer(options.open(started_path(dir, endpoint))?, &started)?;
    Ok(())
}

/// Forgets the agent once its chat ended on its own.
pub(crate) fn agent_ended(dir: &Path, endpoint: &EndpointId) {
    let _ = std::fs::remove_file(started_path(dir, endpoint));
}

/// Whether the agent the chat last started is provably gone. An unknown
/// agent is not: doubt refuses rather than risk two agents on one session.
fn previous_agent_gone(dir: &Path, endpoint: &EndpointId) -> anyhow::Result<bool> {
    use std::io::Read as _;
    let mut bytes = Vec::new();
    File::open(started_path(dir, endpoint))
        .context("the chat's previous agent is unknown")?
        .take(4096)
        .read_to_end(&mut bytes)?;
    let started: Started = serde_json::from_slice(&bytes)?;
    Ok(agent_gone(started.pid, &started.started)?)
}

/// Proof, without signalling anything, that the agent `pid` born at
/// `started` and its process group are gone. Nothing here kills or adopts a
/// process from a stored PID.
pub(crate) fn agent_gone(pid: u32, started: &str) -> sheprd_core_client::Result<bool> {
    if sheprd_core_client::sessions::process_birth(pid)?.as_deref() == Some(started) {
        return Ok(false);
    }
    #[cfg(unix)]
    {
        // spawn() made the agent the leader of its own group. Probe signal
        // zero only: an exited leader may still have live tools in that group.
        let Some(group) = i32::try_from(pid)
            .ok()
            .and_then(rustix::process::Pid::from_raw)
        else {
            return Ok(false);
        };
        match rustix::process::test_kill_process_group(group) {
            Err(rustix::io::Errno::SRCH) => Ok(true),
            Ok(()) | Err(rustix::io::Errno::PERM) => Ok(false),
            Err(error) => Err(std::io::Error::from(error).into()),
        }
    }
    // Windows has no process group to probe, and stop() there ends only the
    // agent itself, so a tool it started may outlive it. The exact agent is
    // gone; anything it left is the same gap a normal Windows close has.
    #[cfg(not(unix))]
    {
        Ok(true)
    }
}

fn matches_context(
    record: &restore::Record,
    endpoint: &EndpointId,
    cwd: &Path,
    var: impl Fn(&str) -> Option<std::ffi::OsString>,
) -> bool {
    record.endpoint == *endpoint
        && record.cwd == cwd
        && var(ENDPOINT_ENV).as_deref() == Some(std::ffi::OsStr::new(endpoint.as_str()))
        && var("HC_TERMINAL_ID").as_deref() == Some(std::ffi::OsStr::new(&record.pane))
        && var("HC_TERMINAL_SOCKET").as_deref() == Some(record.socket.as_os_str())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests;
