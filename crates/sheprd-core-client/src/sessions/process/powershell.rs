//! Runs the Windows process probe under one wall-clock limit, outside the
//! shared script runner. That runner starts each script in a job object, and
//! under it PowerShell's CIM query never returned on CI runners, past any idle
//! limit. The probe needs only one short-lived child, its first kilobyte of
//! output, and a wait that always ends.

use sheprd_client::Error;
use std::{
    io::Read,
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

/// A cold PowerShell start on a busy machine can take well over five seconds
/// before its CIM query answers. Past this, the probe is doubt, never absence.
const LIMIT: Duration = Duration::from_secs(30);
const OUTPUT: u64 = 1024;
const POLL: Duration = Duration::from_millis(20);

/// The probe's stdout once it exits successfully within `LIMIT`.
pub(super) fn run(mut command: Command) -> Result<Vec<u8>, Error> {
    let deadline = Instant::now() + LIMIT;
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().map_err(Error::ScriptSpawn)?;
    let pipes = (child.stdout.take(), child.stderr.take());
    let readers = match pipes {
        (Some(stdout), Some(stderr)) => read(stdout).and_then(|out| Ok((out, read(stderr)?))),
        _ => Err(Error::ScriptIo(std::io::ErrorKind::BrokenPipe.into())),
    };
    let (stdout, stderr) = match readers {
        Ok(readers) => readers,
        Err(error) => {
            stop(&mut child);
            return Err(error);
        }
    };
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => thread::sleep(POLL),
            Ok(None) => {
                stop(&mut child);
                return Err(Error::ScriptTimeout);
            }
            Err(error) => {
                stop(&mut child);
                return Err(Error::ScriptIo(error));
            }
        }
    };
    let output = collect(&stdout, deadline)?;
    if output.len() as u64 > OUTPUT {
        return Err(Error::ScriptOutputLimit);
    }
    if !status.success() {
        let stderr = collect(&stderr, deadline).unwrap_or_default();
        return Err(Error::ScriptExit {
            status,
            stderr: String::from_utf8_lossy(&stderr).trim().to_owned(),
        });
    }
    Ok(output)
}

fn stop(child: &mut std::process::Child) {
    let _ = child.kill();
    let _ = child.wait();
}

/// Reads at most one byte past `OUTPUT` on a detached thread, so a pipe a
/// descendant still holds can never hold the probe past its deadline.
fn read(
    pipe: impl Read + Send + 'static,
) -> Result<mpsc::Receiver<std::io::Result<Vec<u8>>>, Error> {
    let (sent, received) = mpsc::sync_channel(1);
    thread::Builder::new()
        .name("process-probe".to_owned())
        .spawn(move || {
            let mut output = Vec::new();
            let read = pipe.take(OUTPUT + 1).read_to_end(&mut output);
            let _ = sent.send(read.map(|_| output));
        })
        .map_err(Error::ScriptSpawn)?;
    Ok(received)
}

fn collect(
    received: &mpsc::Receiver<std::io::Result<Vec<u8>>>,
    deadline: Instant,
) -> Result<Vec<u8>, Error> {
    match received.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
        Ok(Ok(output)) => Ok(output),
        Ok(Err(error)) => Err(Error::ScriptIo(error)),
        Err(_) => Err(Error::ScriptTimeout),
    }
}
