//! One host's workbench. A worker thread owns an `ssh` that runs `serve.sh`
//! there, reads the port the script reports, forwards it to this computer
//! with a [`PortForward`], and checks that the workbench answers. Windows
//! read only its latest state. Closing the script's input, or ending the
//! `ssh`, stops a workbench the script started.
use super::{super::server, Error};
use crate::{
    browser::WebUrl,
    owned_process as process,
    px0::{Error as Px0Error, State},
};
use sheprd_client::{ForwardEvent, PortForward};
use std::{
    io::{BufRead, BufReader, Read},
    num::NonZeroU16,
    process::{Child, ChildStdin, ChildStdout, ExitStatus, Stdio},
    sync::{
        Arc, Mutex, MutexGuard, PoisonError,
        mpsc::{self, Receiver, SyncSender, TryRecvError},
    },
    thread,
    time::{Duration, Instant},
};

/// The script `ssh` runs on the host.
pub(super) const SCRIPT: &str = include_str!("serve.sh");
/// SSH through a proxy such as Coder's, and px0's first start, are slow.
const PORT_WAIT: Duration = Duration::from_secs(90);
/// From the forward listening to the workbench answering through it.
const READY_WAIT: Duration = Duration::from_secs(30);
const TICK: Duration = Duration::from_millis(100);
/// How often a ready workbench is asked whether it still answers.
const CHECK: Duration = Duration::from_secs(3);
/// How long the script gets to stop px0 before its `ssh` is killed.
const GRACE: Duration = Duration::from_millis(500);
/// `port 65535` and a line break fit with room to spare; a longer line is
/// not the script's.
const LINE: u64 = 64;

pub(super) struct Worker {
    state: Arc<Mutex<State>>,
    stop: SyncSender<()>,
    owned: Arc<Mutex<Owned>>,
}

/// What the worker started, so a stop from any thread can end it.
#[derive(Default)]
struct Owned {
    stopped: bool,
    child: Option<Child>,
    /// The script's input: closing it asks the script to stop px0.
    input: Option<ChildStdin>,
    forward: Option<PortForward>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Worker {
    pub(super) fn start(target: String) -> Result<Self, Px0Error> {
        let state = Arc::new(Mutex::new(State::Starting));
        let (stop, stopped) = mpsc::sync_channel(1);
        let owned = Arc::new(Mutex::new(Owned::default()));
        let (output, slot) = (state.clone(), owned.clone());
        thread::Builder::new()
            .name("herdr-px0-host".into())
            .spawn(move || {
                if let Err(error) = run(&target, &stopped, &output, &slot) {
                    tracing::warn!(%error, target, "px0 on a host stopped");
                    *lock(&output) = State::Failed(Arc::new(error));
                }
                cleanup(&slot);
            })
            .map_err(Px0Error::Worker)?;
        Ok(Self { state, stop, owned })
    }

    pub(super) fn state(&self) -> State {
        lock(&self.state).clone()
    }

    /// Stops the worker and ends what it started, on the calling thread:
    /// for the app's quit, which runs it off the UI thread.
    pub(super) fn shutdown(self) {
        let _ = self.stop.try_send(());
        cleanup(&self.owned);
    }
}

impl Drop for Worker {
    /// Only asks: the worker's thread ends what it started.
    fn drop(&mut self) {
        let _ = self.stop.try_send(());
    }
}

/// The port in the script's report, `port N`.
pub(super) fn parse(line: &str) -> Option<NonZeroU16> {
    line.trim_end().strip_prefix("port ")?.parse().ok()
}

fn cancelled(stop: &Receiver<()>, wait: Duration) -> bool {
    !matches!(
        stop.recv_timeout(wait),
        Err(mpsc::RecvTimeoutError::Timeout)
    )
}

fn cleanup(owned: &Mutex<Owned>) {
    let (child, input, forward) = {
        let mut slot = lock(owned);
        slot.stopped = true;
        (slot.child.take(), slot.input.take(), slot.forward.take())
    };
    if let Some(forward) = forward {
        forward.stop();
    }
    drop(input);
    let Some(mut child) = child else {
        return;
    };
    let deadline = Instant::now() + GRACE;
    while Instant::now() < deadline && !process::exited(&mut child).unwrap_or(true) {
        thread::sleep(Duration::from_millis(25));
    }
    // Unreaped until here, so its process group is still its own.
    process::kill(&mut child);
    let _ = child.wait();
}

/// The script's exit status once its `ssh` has ended, reaping it.
fn ended(owned: &Mutex<Owned>) -> Option<ExitStatus> {
    let mut slot = lock(owned);
    let child = slot.child.as_mut()?;
    if !process::exited(child).unwrap_or(false) {
        return None;
    }
    let mut child = slot.child.take()?;
    // `ssh` may leave a proxy command behind in its group.
    process::terminate(&child);
    child.wait().ok()
}

fn failure(owned: &Mutex<Owned>) -> Option<Px0Error> {
    ended(owned).map(|status| Error::exited(status).into())
}

/// Reads the script's report on a thread of its own: `Some` port, or `None`
/// when its output ends or says anything else first.
fn read_port(stdout: ChildStdout) -> Receiver<Option<NonZeroU16>> {
    let (send, port) = mpsc::sync_channel(1);
    let spawned = thread::Builder::new()
        .name("herdr-px0-host-port".into())
        .spawn(move || {
            let mut line = String::new();
            let read = BufReader::new(stdout).take(LINE).read_line(&mut line);
            let _ = send.send(read.ok().and_then(|_| parse(&line)));
        });
    if spawned.is_err() {
        tracing::warn!("Cannot read the host's px0 port");
    }
    port
}

fn run(
    target: &str,
    stop: &Receiver<()>,
    output: &Mutex<State>,
    owned: &Mutex<Owned>,
) -> Result<(), Px0Error> {
    let mut command = sheprd_client::script_command(target, SCRIPT).map_err(Error::Command)?;
    process::isolate(&mut command);
    command.stdin(Stdio::piped());
    let stdout = {
        let mut slot = lock(owned);
        if slot.stopped {
            return Ok(());
        }
        let mut child = command.spawn().map_err(Error::Spawn)?;
        let stdout = child.stdout.take();
        slot.input = child.stdin.take();
        slot.child = Some(child);
        stdout.ok_or(Error::Output)?
    };
    let Some(remote) = wait_port(&read_port(stdout), stop, owned)? else {
        return Ok(());
    };
    let forward = PortForward::start(target, remote).map_err(Error::Forward)?;
    {
        let mut slot = lock(owned);
        if slot.stopped {
            forward.stop();
            return Ok(());
        }
        slot.forward = Some(forward);
    }
    let Some(local) = wait_listening(stop, owned)? else {
        return Ok(());
    };
    let url = WebUrl::try_from(format!("http://127.0.0.1:{local}/").as_str())
        .map_err(|_| Px0Error::Address)?;
    if !wait_ready(&url, stop, owned)? {
        return Ok(());
    }
    *lock(output) = State::Ready(url.clone());
    while !cancelled(stop, CHECK) {
        if let Some(error) = failure(owned) {
            return Err(error);
        }
        let event = lock(owned)
            .forward
            .as_ref()
            .and_then(PortForward::try_event);
        if let Some(ForwardEvent::Ended(error)) = event {
            return Err(Error::Forward(error).into());
        }
        // As on this computer, a slow answer keeps the page and its edits.
        *lock(output) = match server::probe(&url) {
            Ok(()) => State::Ready(url.clone()),
            Err(error) => State::Failed(Arc::new(error)),
        };
    }
    Ok(())
}

fn wait_port(
    port: &Receiver<Option<NonZeroU16>>,
    stop: &Receiver<()>,
    owned: &Mutex<Owned>,
) -> Result<Option<NonZeroU16>, Px0Error> {
    let deadline = Instant::now() + PORT_WAIT;
    loop {
        if cancelled(stop, TICK) {
            return Ok(None);
        }
        match port.try_recv() {
            Ok(Some(port)) => return Ok(Some(port)),
            Ok(None) | Err(TryRecvError::Disconnected) => {
                // The output ended: the script's status says why.
                let wait = Instant::now() + GRACE;
                while Instant::now() < wait {
                    if let Some(error) = failure(owned) {
                        return Err(error);
                    }
                    thread::sleep(Duration::from_millis(25));
                }
                return Err(Error::Output.into());
            }
            Err(TryRecvError::Empty) => {}
        }
        if let Some(error) = failure(owned) {
            return Err(error);
        }
        if Instant::now() >= deadline {
            return Err(Error::Timeout.into());
        }
    }
}

fn wait_listening(stop: &Receiver<()>, owned: &Mutex<Owned>) -> Result<Option<u16>, Px0Error> {
    let deadline = Instant::now() + PORT_WAIT;
    loop {
        if cancelled(stop, TICK) {
            return Ok(None);
        }
        match lock(owned)
            .forward
            .as_ref()
            .and_then(PortForward::try_event)
        {
            Some(ForwardEvent::Listening { local_port }) => return Ok(Some(local_port)),
            Some(ForwardEvent::Ended(error)) => return Err(Error::Forward(error).into()),
            None => {}
        }
        if let Some(error) = failure(owned) {
            return Err(error);
        }
        if Instant::now() >= deadline {
            return Err(Error::Timeout.into());
        }
    }
}

/// Whether the workbench answered through the forward; `false` when stopped.
fn wait_ready(url: &WebUrl, stop: &Receiver<()>, owned: &Mutex<Owned>) -> Result<bool, Px0Error> {
    let deadline = Instant::now() + READY_WAIT;
    loop {
        match server::probe(url) {
            Ok(()) => return Ok(true),
            Err(error) if Instant::now() >= deadline => return Err(error),
            Err(_) => {}
        }
        if let Some(error) = failure(owned) {
            return Err(error);
        }
        if cancelled(stop, Duration::from_millis(500)) {
            return Ok(false);
        }
    }
}
