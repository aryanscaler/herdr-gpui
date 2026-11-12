//! Read a checkout on the same host that will act on it. Paths returned by
//! Git are host paths; never canonicalize them on the client computer.

use super::{Error, Result, Step, script};
use crate::teleport::Host;
use std::sync::atomic::AtomicBool;

/// Bounded Git query on `host`, preserving each argument as one word.
pub fn git(host: &Host, checkout: &str, args: &[&str], cancelled: &AtomicBool) -> Result<String> {
    validate_path(host, checkout, Step::List)?;
    let args: Vec<_> = ["-C", checkout]
        .into_iter()
        .chain(args.iter().copied())
        .collect();
    let output = host
        .exec("git", &args, Host::QUERY_IDLE, cancelled)
        .map_err(script(Step::List))?;
    String::from_utf8(output)
        .map(|text| text.trim_end_matches('\n').to_owned())
        .map_err(|_| Error::Output { step: Step::List })
}

pub(super) fn validate_path(host: &Host, path: &str, step: Step) -> Result<()> {
    let absolute = if host.is_local() {
        std::path::Path::new(path).is_absolute()
    } else {
        path.starts_with('/')
    };
    if !absolute || path.chars().any(char::is_control) {
        return Err(Error::Output { step });
    }
    Ok(())
}

/// A folder's repository identity, read on its host. No daemon is needed.
pub fn identity(
    host: &Host,
    checkout: &str,
    cancelled: &AtomicBool,
) -> Result<sheprd_vcs::git::Identity> {
    let dirs = git(
        host,
        checkout,
        &[
            "rev-parse",
            "--path-format=absolute",
            "--git-common-dir",
            "--git-dir",
        ],
        cancelled,
    )?;
    let mut lines = dirs.lines();
    let (Some(common), Some(own), None) = (lines.next(), lines.next(), lines.next()) else {
        return Err(Error::Output { step: Step::List });
    };
    let branch = match git(
        host,
        checkout,
        &["symbolic-ref", "--quiet", "--short", "HEAD"],
        cancelled,
    ) {
        Ok(branch) => Some(branch),
        Err(Error::Script {
            source: sheprd_client::Error::ScriptExit { status, .. },
            ..
        }) if status.code() == Some(1) => None,
        Err(error) => return Err(error),
    };
    let label = common
        .trim_end_matches('/')
        .strip_suffix("/.git")
        .unwrap_or(common)
        .rsplit('/')
        .next()
        .unwrap_or_default()
        .trim_end_matches(".git")
        .to_owned();
    Ok(sheprd_vcs::git::Identity {
        branch,
        key: common.to_owned(),
        label,
        linked: common != own,
    })
}

/// Resolve an existing checkout and prove it belongs to the requested
/// repository. A stale picker cannot open another repository at that path.
pub fn open(host: &Host, repository: &str, path: &str, cancelled: &AtomicBool) -> Result<String> {
    let expected = identity(host, repository, cancelled)?;
    let actual = identity(host, path, cancelled)?;
    if expected.key != actual.key {
        return Err(Error::Output { step: Step::List });
    }
    git(host, path, &["rev-parse", "--show-toplevel"], cancelled)
}
