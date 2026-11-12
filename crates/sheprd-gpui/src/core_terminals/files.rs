//! Files prepared away from the UI, with the destination captured before any
//! clipboard read or copy. No Herdr 1 connection participates in delivery.

use crate::{
    Result,
    window::{
        file_drop::quote_paths,
        image_source::{self, Source},
    },
};
use sheprd_client::ConnectTarget;
use std::{
    io::Write,
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
};

pub(super) enum Request {
    Clipboard { image_only: bool },
    Paths(Vec<PathBuf>),
}

/// Unclaimed copies are removed on a worker, including when the pane closes
/// before the background result arrives. Accepted files live until OS/user cleanup.
pub(super) struct Prepared {
    pub(super) text: String,
    cleanup: Option<(ConnectTarget, Vec<String>)>,
}

impl Prepared {
    fn text(text: String) -> Self {
        Self {
            text,
            cleanup: None,
        }
    }

    pub(super) fn accepted(&mut self) {
        self.cleanup = None;
    }
}

impl Drop for Prepared {
    fn drop(&mut self) {
        if let Some((target, paths)) = self.cleanup.take() {
            let _ = std::thread::Builder::new()
                .name("core-paste-cleanup".into())
                .spawn(move || {
                    if let Err(error) = cleanup(&target, &paths) {
                        tracing::warn!(%error, "Could not remove an unclaimed file paste");
                    }
                });
        }
    }
}

pub(super) fn prepare(
    target: &ConnectTarget,
    request: Request,
    cancelled: &AtomicBool,
) -> Result<Prepared> {
    if cancelled.load(Ordering::Acquire) {
        return Err(sheprd_client::Error::UploadCancelled.into());
    }
    match request {
        Request::Paths(paths) => paths_for(target, paths, cancelled),
        Request::Clipboard { image_only } => {
            let Some(item) = crate::window::clipboard::read(image_only)? else {
                return Ok(Prepared::text(String::new()));
            };
            if let Some(paths) = crate::window::external_paths(&item) {
                return paths_for(target, paths, cancelled);
            }
            if let Some(text) = item.text() {
                if target.is_remote()
                    && let Some(Source::File { path, .. }) = image_source::from_paste(&text)
                {
                    return paths_for(target, vec![path], cancelled);
                }
                return Ok(Prepared::text(text));
            }
            for entry in item.entries {
                if let gpui::ClipboardEntry::Image(image) = entry {
                    return image_for(target, Source::Clipboard(image), cancelled);
                }
            }
            Ok(Prepared::text(String::new()))
        }
    }
}

fn paths_for(
    target: &ConnectTarget,
    paths: Vec<PathBuf>,
    cancelled: &AtomicBool,
) -> Result<Prepared> {
    let quoted = quote_paths(&paths)?;
    if let Some(source) = image_source::temporary_drag_image(&paths) {
        return image_for(target, source, cancelled);
    }
    if !target.is_remote() {
        return Ok(Prepared::text(quoted));
    }
    if !matches!(target, ConnectTarget::Wsl { .. })
        && let [path] = paths.as_slice()
        && let Some(source) = image_source::from_path(path)
    {
        return image_for(target, source, cancelled);
    }
    copied_paths(target, &paths, cancelled)
}

fn copied_paths(
    target: &ConnectTarget,
    paths: &[PathBuf],
    cancelled: &AtomicBool,
) -> Result<Prepared> {
    let remote = upload(target, paths, cancelled)?;
    let mut prepared = Prepared {
        text: String::new(),
        cleanup: (!matches!(target, ConnectTarget::Wsl { .. }))
            .then(|| (target.clone(), remote.clone())),
    };
    prepared.text = quote_paths(&remote.iter().map(PathBuf::from).collect::<Vec<_>>())?;
    Ok(prepared)
}

fn image_for(target: &ConnectTarget, source: Source, cancelled: &AtomicBool) -> Result<Prepared> {
    let image = source.prepare()?;
    let mut file = tempfile::Builder::new()
        .prefix("sheprd-clipboard-")
        .suffix(&format!(".{}", image.extension))
        .tempfile()
        .map_err(|source| crate::Error::ImageFile {
            operation: "create",
            source,
        })?;
    file.write_all(&image.bytes)
        .map_err(|source| crate::Error::ImageFile {
            operation: "write",
            source,
        })?;
    if cancelled.load(Ordering::Acquire) {
        return Err(sheprd_client::Error::UploadCancelled.into());
    }
    // WSL reads the Windows staging file through wslpath; keep it as for a
    // local pane, rather than deleting it when the mapping finishes.
    if !target.is_remote() || matches!(target, ConnectTarget::Wsl { .. }) {
        let path = file
            .into_temp_path()
            .keep()
            .map_err(|error| crate::Error::ImageFile {
                operation: "keep",
                source: error.error,
            })?;
        let mut prepared = Prepared {
            text: String::new(),
            cleanup: Some((
                ConnectTarget::Local,
                vec![path.to_string_lossy().into_owned()],
            )),
        };
        prepared.text = if let ConnectTarget::Wsl { distro, .. } = target {
            let paths = sheprd_client::wsl_paths(distro, &[path], cancelled)?;
            quote_paths(&paths.iter().map(PathBuf::from).collect::<Vec<_>>())?
        } else {
            quote_paths(&[path])?
        };
        return Ok(prepared);
    }
    copied_paths(target, &[file.path().to_owned()], cancelled)
}

/// Copy to the exact saved host, or map into the exact WSL distribution.
/// Blocking; callers must retain the returned paths for cancellation cleanup.
pub(crate) fn upload(
    target: &ConnectTarget,
    paths: &[PathBuf],
    cancelled: &AtomicBool,
) -> Result<Vec<String>> {
    quote_paths(paths)?;
    match target {
        ConnectTarget::Ssh { target, .. } => Ok(sheprd_client::upload_files(
            target,
            paths,
            cancelled,
            |_, _| {},
        )?),
        ConnectTarget::Wsl { distro, .. } => {
            Ok(sheprd_client::wsl_paths(distro, paths, cancelled)?)
        }
        #[cfg(feature = "cloud")]
        ConnectTarget::Cloud {
            provider,
            account,
            id,
            ..
        } => sheprd_remote::cloud::with_command(*provider, account, id, cancelled, |command| {
            sheprd_client::upload_files_with(
                paths,
                cancelled,
                |_, _| {},
                |script| Ok(command_for(&command, script)),
            )
        })
        .map_err(sheprd_client::Error::UploadIo)?
        .map_err(Into::into),
        _ => Err(crate::window::file_drop::Error::OtherHost.into()),
    }
}

pub(crate) fn cleanup(target: &ConnectTarget, paths: &[String]) -> Result<()> {
    match target {
        ConnectTarget::Local => {
            for path in paths {
                std::fs::remove_file(path).map_err(|source| crate::Error::ImageFile {
                    operation: "remove",
                    source,
                })?;
            }
            Ok(())
        }
        ConnectTarget::Ssh { target, .. } => {
            Ok(sheprd_client::remove_uploaded_files(target, paths)?)
        }
        ConnectTarget::Wsl { .. } => Ok(()),
        #[cfg(feature = "cloud")]
        ConnectTarget::Cloud {
            provider,
            account,
            id,
            ..
        } => sheprd_remote::cloud::with_command(
            *provider,
            account,
            id,
            &AtomicBool::new(false),
            |command| {
                sheprd_client::remove_uploaded_files_with(paths, |script| {
                    Ok(command_for(&command, script))
                })
            },
        )
        .map_err(sheprd_client::Error::UploadIo)?
        .map_err(Into::into),
        _ => Err(crate::window::file_drop::Error::OtherHost.into()),
    }
}

#[cfg(feature = "cloud")]
fn command_for(template: &std::process::Command, script: &str) -> std::process::Command {
    let mut command = sheprd_client::new_command(template.get_program());
    command.args(template.get_args());
    for (key, value) in template.get_envs() {
        if let Some(value) = value {
            command.env(key, value);
        } else {
            command.env_remove(key);
        }
    }
    if let Some(directory) = template.get_current_dir() {
        command.current_dir(directory);
    }
    command.arg(script);
    command
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests;
