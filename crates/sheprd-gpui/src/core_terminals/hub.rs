//! The window's links to herdr-core: this computer's, found running or
//! started from the copy bundled beside the app when the window is allowed
//! to, and each other host's, reached over the bridge the app runs to it
//! (`sheprd_client::core_bridge`). Each dials on a thread of its own; the
//! window reads the outcome on its tick.

use super::Error;
use sheprd_client::ConnectTarget;
use sheprd_core_client::{Core, CoreBackend, Endpoint, PROGRAM};
use std::{
    sync::{Arc, atomic::AtomicBool},
    time::{Duration, Instant},
};

/// Scrollback lines each herdr-core terminal keeps in the app.
const SCROLLBACK: usize = 10_000;
/// How long a started core has to listen.
const START_TIMEOUT: Duration = Duration::from_secs(20);
/// How long a failed join waits before restored tabs try again, so a core
/// that is not running costs one attempt now and then, not one per tick.
const JOIN_RETRY: Duration = Duration::from_secs(2);
/// [`JOIN_RETRY`] for another host, where each attempt opens an SSH
/// connection and a host that is down takes seconds to say so.
const REMOTE_RETRY: Duration = Duration::from_secs(15);

/// Where the window's herdr-core connection stands.
#[derive(Default)]
pub(crate) enum State {
    #[default]
    Idle,
    Connecting(std::sync::mpsc::Receiver<Result<CoreBackend, Error>>),
    Ready(CoreBackend),
    /// Shared so the window can show it without moving it.
    Unavailable(Arc<Error>),
}

/// Whether the window may start a core when none is running.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Start {
    /// Only join a core that already runs.
    Join,
    /// Start the user's `herdr-core`, else the bundled one. The window asks
    /// for this only when no Herdr 1 is installed, or the user did.
    IfMissing,
}

/// How an attempt joins: the real one dials herdr-core; tests stand in.
type Join = Arc<dyn Fn(Start) -> Result<CoreBackend, Error> + Send + Sync>;

pub(crate) struct Hub {
    /// What the attempt in flight may do.
    attempt: Option<Start>,
    /// How many attempts it made, for tests that count dials.
    #[cfg(test)]
    dials: usize,
    /// When the last attempt failed.
    failed: Option<Instant>,
    join: Join,
    /// How long a failed join waits before a join-only ask dials again.
    retry: Duration,
    state: State,
    /// A start asked for while a join-only attempt was in flight; it runs if
    /// that attempt fails, rather than being dropped.
    wants_start: bool,
}

impl Default for Hub {
    fn default() -> Self {
        Self::with_join(join)
    }
}

impl Hub {
    pub(crate) fn with_join(
        join: impl Fn(Start) -> Result<CoreBackend, Error> + Send + Sync + 'static,
    ) -> Self {
        Self {
            attempt: None,
            #[cfg(test)]
            dials: 0,
            failed: None,
            join: Arc::new(join),
            retry: JOIN_RETRY,
            state: State::Idle,
            wants_start: false,
        }
    }

    /// The core on `target`, another host, which its bridge starts there
    /// when none is running. A restored tab and a new one dial alike.
    pub(crate) fn remote(target: ConnectTarget) -> Self {
        Self {
            retry: REMOTE_RETRY,
            ..Self::with_join(move |_| join_remote(&target))
        }
    }

    /// How many attempts it made.
    // Only the lifecycle tests, which need Unix sockets, count them.
    #[cfg(all(test, unix))]
    pub(crate) fn dials(&self) -> usize {
        self.dials
    }

    pub(crate) fn state(&self) -> &State {
        &self.state
    }

    pub(crate) fn backend(&self) -> Option<&CoreBackend> {
        match &self.state {
            State::Ready(backend) => Some(backend),
            _ => None,
        }
    }

    /// Connects on a background thread unless already connected or trying.
    /// A join-only ask waits a moment after a failure; asking to start
    /// always tries, and upgrades a join-only attempt in flight.
    pub(crate) fn connect(&mut self, start: Start) {
        match &self.state {
            State::Ready(_) => return,
            State::Connecting(_) => {
                self.wants_start |= start == Start::IfMissing && self.attempt == Some(Start::Join);
                return;
            }
            State::Unavailable(_)
                if start == Start::Join
                    && self
                        .failed
                        .is_some_and(|failed| failed.elapsed() < self.retry) =>
            {
                return;
            }
            State::Idle | State::Unavailable(_) => {}
        }
        self.attempt = Some(start);
        #[cfg(test)]
        {
            self.dials += 1;
        }
        let join = Arc::clone(&self.join);
        let (send, receive) = std::sync::mpsc::sync_channel(1);
        let spawned = std::thread::Builder::new()
            .name("herdr-core-connect".into())
            .spawn(move || {
                let _ = send.send(join(start));
            });
        self.state = match spawned {
            Ok(_) => State::Connecting(receive),
            Err(error) => State::Unavailable(Arc::new(Error::Worker(error))),
        };
    }

    /// Forgets the last attempt and its outcome, so the next ask dials anew,
    /// as for a core just installed. An attempt in flight is left to end
    /// unread.
    pub(crate) fn forget(&mut self) {
        self.attempt = None;
        self.failed = None;
        self.state = State::Idle;
        self.wants_start = false;
    }

    /// Takes the connection's outcome; true when the state changed. A lost
    /// connection reads as idle again, so the next ask reconnects.
    pub(crate) fn poll(&mut self) -> bool {
        match &self.state {
            State::Connecting(outcome) => match outcome.try_recv() {
                Ok(Ok(backend)) => self.state = State::Ready(backend),
                Ok(Err(_)) if std::mem::take(&mut self.wants_start) => {
                    self.state = State::Idle;
                    self.connect(Start::IfMissing);
                    return false;
                }
                Ok(Err(error)) => {
                    self.failed = Some(Instant::now());
                    self.state = State::Unavailable(Arc::new(error));
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => return false,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.state = State::Idle,
            },
            State::Ready(backend) if backend.core().is_lost() => self.state = State::Idle,
            _ => return false,
        }
        true
    }
}

/// Blocking: joins the session's core, starting one first when allowed.
fn join(start: Start) -> Result<CoreBackend, Error> {
    let endpoint = Endpoint::resolve(sheprd_client::login_env::var)?;
    let first = match Core::connect(&endpoint) {
        Ok(core) => return Ok(CoreBackend::new(core, SCROLLBACK)),
        Err(error) => error,
    };
    if start == Start::Join || !matches!(first, sheprd_core_client::Error::Connect { .. }) {
        return Err(first.into());
    }
    let binary = crate::bundled::locate(PROGRAM).ok_or(Error::NotBundled)?;
    sheprd_core_client::start(&binary, sheprd_client::login_env::apply, START_TIMEOUT)?;
    // `server start` returns once the core listens; a slow file system can
    // still refuse the first dial.
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match Core::connect(&endpoint) {
            Ok(core) => return Ok(CoreBackend::new(core, SCROLLBACK)),
            Err(sheprd_core_client::Error::Connect { .. }) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(error) => return Err(error.into()),
        }
    }
}

/// Blocking: reaches the core on another host through the bridge the app
/// runs to it, which starts that host's core when none is running.
fn join_remote(target: &ConnectTarget) -> Result<CoreBackend, Error> {
    // The attempt runs to its end; the bridge's own deadlines bound it.
    let stop = AtomicBool::new(false);
    let bridge = match target {
        ConnectTarget::Ssh { target, .. } => sheprd_client::core_bridge::ssh(target, &stop)?,
        ConnectTarget::Wsl { distro, .. } => sheprd_client::core_bridge::wsl(distro, &stop)?,
        #[cfg(feature = "cloud")]
        ConnectTarget::Cloud { .. } => sheprd_remote::cloud::connect_core(target, &stop)
            .map_err(|error| Error::Bridge(error.into()))?,
        ConnectTarget::Local | ConnectTarget::Session { .. } | ConnectTarget::Socket(_) => {
            return Err(Error::NotRemote);
        }
    };
    Ok(CoreBackend::new(Core::bridged(bridge)?, SCROLLBACK))
}

#[cfg(test)]
mod tests;
