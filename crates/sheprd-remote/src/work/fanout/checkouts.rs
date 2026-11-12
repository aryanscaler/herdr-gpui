//! A creation intent names an exact host-side path before Git is changed.
//! Only a successful creation writes the ownership receipt; an uncertain
//! creation without that receipt is retained for inspection, never adopted.

pub use super::{Error, Result};
use crate::work::{Host, NewWorktree, Opened, Step, Worktree, script};
use sheprd_client::{herdr::worktree, shell_quote};
use std::{path::Path, sync::atomic::AtomicBool};

pub fn plan(host: &Host, checkout: &str, branch: &str, cancelled: &AtomicBool) -> Result<Worktree> {
    crate::work::inspect::validate_path(host, checkout, Step::CreateWorktree)?;
    let common = host
        .capture(&super::super::common_dir_line(checkout), cancelled)
        .map_err(script(Step::CreateWorktree))?;
    let common = String::from_utf8_lossy(&common);
    let common = common.trim_end_matches('\n');
    crate::work::inspect::validate_path(host, common, Step::CreateWorktree)?;
    let folder = worktree::folder(Path::new(common), branch).map_err(crate::work::Error::from)?;
    let relative = relative(&folder);
    let output = host
        .capture(
            &format!("printf '%s/%s\\n' \"$HOME\" {}\n", shell_quote(&relative)),
            cancelled,
        )
        .map_err(script(Step::CreateWorktree))?;
    let path = String::from_utf8_lossy(&output).trim().to_owned();
    if path.is_empty() {
        return Err(Error::Uncertain);
    }
    Ok(Worktree {
        path,
        opened: Opened::Herdr2,
    })
}

/// The script runs on a POSIX host, even from a Windows client.
fn relative(folder: &Path) -> String {
    std::iter::once(worktree::ROOT.to_owned())
        .chain(
            folder
                .components()
                .map(|part| part.as_os_str().to_string_lossy().into_owned()),
        )
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests;

pub fn create(
    host: &Host,
    checkout: &str,
    tree: &Worktree,
    new: &NewWorktree<'_>,
    token: &str,
    cancelled: &AtomicBool,
) -> Result<()> {
    let line = format!(
        "git -C {} worktree add -b {} {} --end-of-options {} >&2\ngitdir=$(git -C {} rev-parse --absolute-git-dir)\nprintf '%s\\n' {} > \"$gitdir/sheprd-fanout-owner\"\n",
        shell_quote(checkout),
        shell_quote(new.branch),
        shell_quote(&tree.path),
        shell_quote(new.base),
        shell_quote(&tree.path),
        shell_quote(token),
    );
    host.capture(&line, cancelled)
        .map_err(script(Step::CreateWorktree))?;
    Ok(())
}

/// Verify branch, exact checkout root and creation receipt on its own host.
/// A replacement repository or branch never inherits ownership by path.
pub fn verify(
    host: &Host,
    tree: &Worktree,
    branch: &str,
    token: &str,
    cancelled: &AtomicBool,
) -> Result<()> {
    let output = host
        .capture(&verification(tree, branch, token), cancelled)
        .map_err(script(Step::List))?;
    if output.trim_ascii() != b"owned" {
        return Err(Error::Uncertain);
    }
    Ok(())
}

fn verification(tree: &Worktree, branch: &str, token: &str) -> String {
    format!(
        "path={}\nroot=$(git -C \"$path\" rev-parse --show-toplevel)\n[ \"$(cd \"$path\" && pwd -P)\" = \"$root\" ]\n[ \"$(git -C \"$path\" symbolic-ref --short HEAD)\" = {} ]\ngitdir=$(git -C \"$path\" rev-parse --absolute-git-dir)\ncommon=$(git -C \"$path\" rev-parse --path-format=absolute --git-common-dir)\n[ \"$gitdir\" != \"$common\" ]\n[ \"$(cat \"$gitdir/sheprd-fanout-owner\")\" = {} ]\nprintf 'owned\\n'\n",
        shell_quote(&tree.path),
        shell_quote(branch),
        shell_quote(token),
    )
}

/// Verify and remove in one host script. The branch remains reachable.
pub fn discard(
    host: &Host,
    tree: &Worktree,
    branch: &str,
    token: &str,
    cancelled: &AtomicBool,
) -> Result<()> {
    let line = format!(
        "{}git --git-dir=\"$common\" worktree remove --force \"$root\" >&2\n",
        verification(tree, branch, token)
    );
    host.capture(&line, cancelled)
        .map_err(script(Step::Remove))?;
    Ok(())
}
