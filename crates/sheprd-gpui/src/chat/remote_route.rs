//! Host-side execution without moving endpoint paths or credentials across hosts.

use sheprd_client::ConnectTarget;
use std::{
    io,
    process::Command,
    sync::atomic::{AtomicBool, Ordering},
};

/// Runs on a worker. Provider credentials stay alive until `run` finishes.
pub(super) fn with_command<T>(
    target: &ConnectTarget,
    script: &str,
    stop: &AtomicBool,
    run: impl FnOnce(Command) -> T,
) -> io::Result<T> {
    if stop.load(Ordering::Acquire) {
        return Err(io::ErrorKind::Interrupted.into());
    }
    match target {
        ConnectTarget::Ssh { target, .. } => {
            let command =
                sheprd_client::script_command(target, script).map_err(io::Error::other)?;
            Ok(run(command))
        }
        ConnectTarget::Wsl { distro, .. } => {
            let command =
                sheprd_client::wsl::script_command(distro, script).map_err(io::Error::other)?;
            Ok(run(command))
        }
        #[cfg(feature = "cloud")]
        ConnectTarget::Cloud {
            provider,
            account,
            id,
            ..
        } => sheprd_remote::cloud::with_command(*provider, account, id, stop, |mut command| {
            command.arg(format!("/bin/sh -c {}", sheprd_client::shell_quote(script)));
            run(command)
        }),
        _ => Err(io::ErrorKind::Unsupported.into()),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests;
