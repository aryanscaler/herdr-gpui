//! Starts the agent and wires the session, the attach socket, Herdr reports,
//! and the in-pane view together.

use std::{path::PathBuf, thread};

use anyhow::Context as _;
use crossbeam_channel::{Sender, bounded};
use sheprd_chat_protocol::{EndpointId, NoticeLevel};

use crate::{
    agents::Launch,
    cli::{AgentChoice, Options},
    harness::{Acp, Pipes, Reach},
    herdr::{Reporter, Target},
    process,
    session::{Config, Flow, Input, Session, Update},
};

/// Inputs waiting for the session thread.
const INPUTS: usize = 1024;

/// Asks for the ACP adapter, as `--acp` does.
const ADAPTER_VARIABLE: &str = "HERDR_ACP_ADAPTER";

struct Agent {
    args: Vec<std::ffi::OsString>,
    display: String,
    /// Added to what the agent inherits.
    env: Vec<(&'static str, &'static str)>,
    /// What the agent's own log says of a session it will not replay.
    history: Option<crate::history::Result<sheprd_chat_protocol::Transcript>>,
    login_hint: Option<String>,
    program: std::ffi::OsString,
    reach: Box<dyn Reach>,
}

fn agent(options: &Options, cwd: &std::path::Path) -> Agent {
    let name = options.name.clone();
    match &options.agent {
        AgentChoice::Builtin { agent, args: extra } => {
            let agent = *agent;
            let path = crate::agents::search_path(
                std::env::var_os("PATH").as_deref(),
                std::env::home_dir().as_deref(),
            );
            let adapter = adapter(options);
            let mut env = Vec::new();
            let mut history = None;
            let (program, mut args, reach): (_, _, Box<dyn Reach>) =
                match crate::agents::launch(agent, Some(&path), adapter) {
                    Launch::Native(program) => {
                        let (args, reach) = native(agent, options.load.as_deref());
                        history = native_history(agent, options.load.as_deref(), cwd);
                        if matches!(agent, crate::agents::Agent::Claude) {
                            let set = std::env::var_os(crate::claude::TODO_TOOLS);
                            env = crate::claude::env(set.as_deref());
                        }
                        (program.into_os_string(), args, reach)
                    }
                    // An ACP agent replays its own history on `session/load`.
                    Launch::Acp(program, args) => (program, args, Box::new(Acp)),
                };
            args.extend(extra.iter().cloned());
            Agent {
                args,
                display: name.unwrap_or_else(|| agent.display_name().to_owned()),
                env,
                history,
                login_hint: Some(agent.login_hint().to_owned()),
                program,
                reach,
            }
        }
        AgentChoice::Command { program, args } => Agent {
            display: name.unwrap_or_else(|| {
                PathBuf::from(&program)
                    .file_name()
                    .map_or_else(|| "Agent".to_owned(), |n| n.to_string_lossy().into_owned())
            }),
            args: args.clone(),
            env: Vec::new(),
            history: None,
            login_hint: None,
            program: program.clone(),
            reach: Box::new(Acp),
        },
    }
}

/// Whether the chat reaches a built-in agent through its ACP adapter.
fn adapter(options: &Options) -> bool {
    options.adapter || std::env::var_os(ADAPTER_VARIABLE).is_some_and(|value| value == "1")
}

/// The arguments that start `agent`'s own program, and the translator that
/// reaches it, continuing session `load` when set.
fn native(
    agent: crate::agents::Agent,
    load: Option<&str>,
) -> (Vec<std::ffi::OsString>, Box<dyn Reach>) {
    use crate::agents::Agent;
    match agent {
        Agent::Codex => (crate::codex::args(), crate::codex::reach()),
        Agent::Pi => (crate::pi::args(load), crate::pi::reach()),
        // `launch` reaches no other agent natively; Claude is the default.
        _ => {
            let session = load.map_or_else(|| uuid::Uuid::new_v4().to_string(), str::to_owned);
            (
                crate::claude::args(&session, load.is_some()),
                crate::claude::reach(session),
            )
        }
    }
}

/// What a natively reached agent's log says of session `load`, for an agent
/// whose log this program can read.
fn native_history(
    agent: crate::agents::Agent,
    load: Option<&str>,
    cwd: &std::path::Path,
) -> Option<crate::history::Result<sheprd_chat_protocol::Transcript>> {
    let agent = crate::mirror::transcript::TranscriptAgent::try_from(agent).ok()?;
    Some(crate::history::load(agent, load?, cwd))
}

pub(crate) fn run(options: Options) -> anyhow::Result<()> {
    let _owner = crate::restore::core::lock()?;
    let claimed = crate::restore::core::claim_launch()?;
    run_owned(claimed, options)
}

/// The caller holds the core endpoint lock before reading recovery state or
/// spawning any agent. Herdr 1 chats retain their existing ownership policy.
/// `claimed` is the recovery generation this launch claimed, if any.
pub(crate) fn run_owned(
    claimed: Option<crate::restore::core::Claimed>,
    options: Options,
) -> anyhow::Result<()> {
    let cwd = match options.cwd.clone() {
        Some(cwd) => cwd,
        None => std::env::current_dir().context("could not read the current directory")?,
    };
    let cwd = cwd
        .canonicalize()
        .with_context(|| format!("no directory {}", cwd.display()))?;
    let agent = agent(&options, &cwd);
    let spawned = process::spawn(&agent.program, &agent.args, &agent.env, &cwd)?;
    let agent_process = spawned.process.clone();
    let owned_agent = agent_process.owned();
    if let Some(claimed) = &claimed {
        crate::restore::core::child_started(claimed, agent_process.id())?;
    }
    let (inputs, receiver) = bounded::<Input>(INPUTS);

    let to_session = inputs.clone();
    let outbox = agent.reach.connect(
        Pipes {
            input: Box::new(spawned.input),
            output: Box::new(spawned.output),
        },
        Box::new(move |inbound| to_session.send(Input::Agent(inbound)).is_ok()),
    )?;
    let waiter = inputs.clone();
    let watched = agent_process.clone();
    thread::Builder::new()
        .name("acp-wait".to_owned())
        .spawn(move || {
            let code = watched.wait().ok().and_then(|status| status.code());
            let stderr = watched.stderr_tail();
            let _ = waiter.send(Input::AgentExited { code, stderr });
        })?;

    let mut notices = Vec::new();
    let history = agent.history.and_then(|history| match history {
        Ok(history) => Some(history),
        // The agent still has its session; only the view misses it.
        Err(error) => {
            notices.push((
                NoticeLevel::Warning,
                format!("Earlier messages cannot be shown: {error}"),
            ));
            None
        }
    });
    // Held to the end: dropping the server removes its socket.
    let (_server, endpoint) = if options.attach {
        listen(inputs.clone(), &mut notices)
    } else {
        (None, None)
    };
    let target = Target::from_env();
    let mut keeper = if target.is_none() && std::env::var_os("HC_TERMINAL_ID").is_some() {
        crate::restore::Keeper::core(
            sheprd_chat_protocol::restore::restore_dir(),
            endpoint.as_ref(),
            &options.agent,
            adapter(&options),
            &cwd,
            |name| std::env::var_os(name),
        )
    } else {
        crate::restore::Keeper::new(
            sheprd_chat_protocol::restore::restore_dir(),
            target.as_ref(),
            endpoint.as_ref(),
            &options.agent,
            adapter(&options),
            &cwd,
        )
    };
    if let Some(keeper) = &mut keeper {
        keeper.child = Some(agent_process.id());
    }
    if target.is_none()
        && let Some(keeper) = &keeper
        && let Err(error) = keeper.agent_started(agent_process.id())
    {
        // A later restore then refuses rather than guessing the agent ended.
        notices.push((
            NoticeLevel::Warning,
            format!("This chat cannot be restored if it is lost: {error:#}"),
        ));
    }
    let reporter = match &target {
        Some(target) => Some(Reporter::start(
            target.clone(),
            agent.display.clone(),
            endpoint.map(|id| id.to_string()),
        )?),
        None => None,
    };
    let report = reporter.as_ref().map(Reporter::handle);
    let session_keeper = keeper.clone();

    let config = Config {
        agent: agent.display,
        cwd,
        exit_with_agent: options.headless,
        history,
        load: options.load,
        login_hint: agent.login_hint,
        pane: target.map(|target| target.pane),
        require_load: options.require_load,
    };
    let session_thread = thread::Builder::new()
        .name("acp-session".to_owned())
        .spawn(move || {
            let mut session = Session::new(config, outbox, move |update| match update {
                Update::Opened(id) => {
                    if let Some(keeper) = &session_keeper {
                        keeper.opened(&id);
                    }
                }
                Update::Activity(activity) => {
                    if let Some(report) = &report {
                        report.report(activity);
                    }
                }
                Update::Prompt(prompt) => {
                    if let Some(report) = &report {
                        report.prompt(prompt);
                    }
                }
            });
            for (level, text) in notices {
                session.notice(level, text);
            }
            session.start();
            for input in receiver {
                let expected = crate::restore::expected_end(&input);
                if session.handle(input) == Flow::Exit {
                    return expected;
                }
            }
            false
        })?;

    // Headless, the session ends when the agent does; otherwise when the view quits.
    let result = if options.headless {
        Ok(())
    } else {
        let result = crate::tui::run(&inputs);
        let _ = inputs.send(Input::Shutdown);
        result
    };
    let ended = session_thread.join().unwrap_or(false);
    let expected = ended && result.is_ok();
    // Only an intentional close or successful agent exit forgets continuity.
    // A failed view, failed agent, or panicked session keeps its record.
    if expected && let Some(keeper) = &keeper {
        keeper.ended();
    }
    drop(owned_agent);
    if let Some(reporter) = reporter {
        reporter.close();
    }
    if options.headless {
        return headless_result(ended);
    }
    result
}

/// Headless, the chat program's exit status is the agent's. A failed or
/// crashed agent must not look like a chat that finished: core keeps a
/// recovery shell only for a program that did not end normally.
fn headless_result(expected: bool) -> anyhow::Result<()> {
    if expected {
        Ok(())
    } else {
        Err(crate::Error::AgentEnded.into())
    }
}

fn listen(
    inputs: Sender<Input>,
    notices: &mut Vec<(NoticeLevel, String)>,
) -> (Option<crate::attach::Server>, Option<EndpointId>) {
    let listening = crate::attach::address().and_then(|(address, id)| {
        crate::attach::Server::listen(&address, inputs).map(|server| (server, id))
    });
    match listening {
        Ok((server, id)) => (Some(server), Some(id)),
        Err(error) => {
            notices.push((
                NoticeLevel::Warning,
                format!("Sheprd cannot attach: {:#}", anyhow::Error::from(error)),
            ));
            (None, None)
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests;
