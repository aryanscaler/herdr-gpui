//! One host's workbench. A worker thread reads px0's `workbench.lock` on the
//! host and reuses the workbench it names when that answers; otherwise an
//! `ssh` it owns runs `serve.sh`, which starts px0 there, and the worker waits
//! for px0's new lock. Either way it forwards the workbench's port to this
//! computer with a [`PortForward`] and checks that it answers. Windows read
//! only its latest state. Closing the script's input, or ending the `ssh`,
//! stops a workbench the script started.
use super::{
    super::server::{self, Discovery},
    Error,
};
use crate::{
    browser::WebUrl,
    owned_process as process,
    px0::{Error as Px0Error, State},
};
use sheprd_client::{ForwardEvent, PortForward, ScriptLimits, SshLane};
use std::{
    num::NonZeroU16,
    process::{Child, ChildStdin, ExitStatus, Stdio},
    sync::{
        Arc, Mutex, MutexGuard, PoisonError,
        atomic::AtomicBool,
        mpsc::{self, Receiver, SyncSender},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

/// The script `ssh` runs on the host.
pub(super) const SCRIPT: &str = include_str!("serve.sh");
/// Prints the host's `workbench.lock`, or nothing when there is none.
const READ_LOCK: &str =
    r#"cat "$HOME/.herdr-gpui/px0-state/px0/workbench.lock" 2>/dev/null; exit 0"#;
const READ_LIMITS: ScriptLimits = ScriptLimits {
    output: server::LOCK_LIMIT,
    idle: Duration::from_secs(20),
};
/// SSH through a proxy such as Coder's, and px0's first start, are slow.
const PORT_WAIT: Duration = Duration::from_secs(90);
/// How often the host's lock is read while px0 starts.
const LOCK_POLL: Duration = Duration::from_millis(500);
/// From the forward listening to the workbench answering through it.
const READY_WAIT: Duration = Duration::from_secs(30);
/// The same for a workbench found running: one that does not answer by
/// then is a stale lock, and a new px0 replaces it.
const REUSE_WAIT: Duration = Duration::from_secs(5);
const TICK: Duration = Duration::from_millis(100);
/// How often a ready workbench is asked whether it still answers.
const CHECK: Duration = Duration::from_secs(3);
/// How long the script gets to stop px0 before its `ssh` is killed, as the
/// app quits.
const GRACE: Duration = Duration::from_millis(500);
/// The same off the app's quit: a replacement waits for this, so it never
/// finds the old workbench still stopping and reuses it.
const STOP_GRACE: Duration = Duration::from_secs(10);

pub(super) struct Worker {
    state: Arc<Mutex<State>>,
    stop: SyncSender<()>,
    owned: Arc<Mutex<Owned>>,
    thread: Option<JoinHandle<()>>,
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
    /// Starts `target`'s workbench once `previous`, the one it replaces,
    /// has stopped, so the new script never reuses a workbench being stopped.
    pub(super) fn start(target: String, previous: Option<Self>) -> Result<Self, Px0Error> {
        let state = Arc::new(Mutex::new(State::Starting));
        let (stop, stopped) = mpsc::sync_channel(1);
        let owned = Arc::new(Mutex::new(Owned::default()));
        let (output, slot) = (state.clone(), owned.clone());
        let thread = thread::Builder::new()
            .name("herdr-px0-host".into())
            .spawn(move || {
                if let Some(previous) = previous {
                    previous.finish();
                }
                if let Err(error) = run(&target, &stopped, &output, &slot) {
                    tracing::warn!(%error, target, "px0 on a host stopped");
                    *lock(&output) = State::Failed(Arc::new(error));
                }
                cleanup(&slot, STOP_GRACE);
            })
            .map_err(Px0Error::Worker)?;
        Ok(Self {
            state,
            stop,
            owned,
            thread: Some(thread),
        })
    }

    pub(super) fn state(&self) -> State {
        lock(&self.state).clone()
    }

    /// Stops the worker and ends what it started, on the calling thread:
    /// for the app's quit, which runs it off the UI thread.
    pub(super) fn shutdown(self) {
        let _ = self.stop.try_send(());
        cleanup(&self.owned, GRACE);
    }

    /// Asks the worker to stop; its thread ends what it started.
    pub(super) fn stop(&self) {
        let _ = self.stop.try_send(());
    }

    /// Stops the worker and waits until its script and `ssh` have ended,
    /// on a replacement's thread.
    fn finish(mut self) {
        self.stop();
        cleanup(&self.owned, STOP_GRACE);
        // Whichever of the two took the `ssh` waits out its grace; the
        // thread's end covers the case where it was the worker's own.
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for Worker {
    /// Only asks: the worker's thread ends what it started.
    fn drop(&mut self) {
        let _ = self.stop.try_send(());
    }
}

fn cancelled(stop: &Receiver<()>, wait: Duration) -> bool {
    !matches!(
        stop.recv_timeout(wait),
        Err(mpsc::RecvTimeoutError::Timeout)
    )
}

fn cleanup(owned: &Mutex<Owned>, grace: Duration) {
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
    let deadline = Instant::now() + grace;
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

/// The workbench the host's `workbench.lock` names, if any, over the host's
/// shared connection.
fn read_lock(target: &str) -> Result<Option<Discovery>, Error> {
    // Held until the read's child is reaped below.
    let (command, _channel) =
        sheprd_client::shared_script_command(target, READ_LOCK, SshLane::Quick)
            .map_err(Error::Command)?;
    let mut output = Vec::new();
    sheprd_client::run_script_command(
        command,
        std::io::empty(),
        &mut output,
        READ_LIMITS,
        &AtomicBool::new(false),
    )
    .map_err(Error::Read)?;
    Ok(server::parse(&output))
}

/// How reaching a workbench through a forward went.
enum Reach {
    Ready(WebUrl),
    /// It did not answer in time.
    Silent,
    Stopped,
}

fn run(
    target: &str,
    stop: &Receiver<()>,
    output: &Mutex<State>,
    owned: &Mutex<Owned>,
) -> Result<(), Px0Error> {
    let before = read_lock(target)?;
    let reused = match &before {
        Some(found) => reach(target, found, REUSE_WAIT, stop, owned)?,
        None => Reach::Silent,
    };
    let url = match reused {
        Reach::Ready(url) => url,
        Reach::Stopped => return Ok(()),
        // No workbench, or a stale lock: px0 starts and replaces it.
        Reach::Silent => {
            if let Some(forward) = lock(owned).forward.take() {
                forward.stop();
            }
            let Some(found) = launch(target, before.as_ref(), stop, owned)? else {
                return Ok(());
            };
            match reach(target, &found, READY_WAIT, stop, owned)? {
                Reach::Ready(url) => url,
                Reach::Silent => return Err(Error::Timeout.into()),
                Reach::Stopped => return Ok(()),
            }
        }
    };
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

/// Starts px0 on the host and returns the workbench its new lock names: one
/// other than `before`, the stale lock px0 replaces. `None` when stopped.
fn launch(
    target: &str,
    before: Option<&Discovery>,
    stop: &Receiver<()>,
    owned: &Mutex<Owned>,
) -> Result<Option<Discovery>, Px0Error> {
    let mut command = sheprd_client::script_command(target, SCRIPT).map_err(Error::Command)?;
    process::isolate(&mut command);
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    {
        let mut slot = lock(owned);
        if slot.stopped {
            return Ok(None);
        }
        let mut child = command.spawn().map_err(Error::Spawn)?;
        slot.input = child.stdin.take();
        slot.child = Some(child);
    }
    let deadline = Instant::now() + PORT_WAIT;
    loop {
        if cancelled(stop, LOCK_POLL) {
            return Ok(None);
        }
        if let Some(error) = failure(owned) {
            return Err(error);
        }
        if let Some(found) = read_lock(target)?
            && before != Some(&found)
        {
            return Ok(Some(found));
        }
        if Instant::now() >= deadline {
            return Err(Error::Address.into());
        }
    }
}

/// Forwards `found`'s port here and waits up to `wait` for the workbench to
/// answer through it. The forward stays in `owned`.
fn reach(
    target: &str,
    found: &Discovery,
    wait: Duration,
    stop: &Receiver<()>,
    owned: &Mutex<Owned>,
) -> Result<Reach, Px0Error> {
    let remote = server::base(&found.url)?
        .port()
        .and_then(NonZeroU16::new)
        .ok_or(Px0Error::Address)?;
    let forward = PortForward::start(target, remote).map_err(Error::Forward)?;
    {
        let mut slot = lock(owned);
        if slot.stopped {
            forward.stop();
            return Ok(Reach::Stopped);
        }
        slot.forward = Some(forward);
    }
    let Some(local) = wait_listening(stop, owned)? else {
        return Ok(Reach::Stopped);
    };
    let url = WebUrl::try_from(format!("http://127.0.0.1:{local}/").as_str())
        .map_err(|_| Px0Error::Address)?;
    let deadline = Instant::now() + wait;
    loop {
        match server::probe(&url) {
            Ok(()) => return Ok(Reach::Ready(url)),
            Err(_) if Instant::now() >= deadline => return Ok(Reach::Silent),
            Err(_) => {}
        }
        if let Some(error) = failure(owned) {
            return Err(error);
        }
        if cancelled(stop, Duration::from_millis(500)) {
            return Ok(Reach::Stopped);
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
