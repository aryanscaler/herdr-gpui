//! A workspace's changes worked out by the daemon with Git, when Sheprd
//! cannot answer: it is closed, or it does not list the workspace (a
//! Herdr 2 workspace, or any on a host Sheprd does not run on). Read-only,
//! like Sheprd's answers, and shaped by the same bounds. A phone names only
//! a workspace, whose listed folder is read, and a file its list gave.
//! Blocking: runs on the blocking pool.
//!
//! Pull requests and their checks need GitHub, which only Sheprd signs in
//! to: without it, a workspace has no pull request a phone can see.

use crate::control::changes::{
    Answer, ChangedFile, DiffLine, Failure, FileDiff, FileList, FileStatus, LineKind,
    MAX_DIFF_BYTES, MAX_PATH, Scope, Shown, Wanted,
};
use std::{
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};

/// How long one Git command may run: a slow checkout or a stuck hook must
/// not hold a blocking thread after the phone gave up.
const GIT_TIMEOUT: Duration = Duration::from_secs(5);
/// The most Git output read for a listing; more is unreadable here.
const MAX_LISTING: u64 = 8 * 1024 * 1024;
/// The largest untracked file read to count or show its lines.
const MAX_UNTRACKED: u64 = 512 * 1024;
/// Branches a branch's changes are compared with, after `origin/HEAD`.
const BASES: &[&str] = &["origin/main", "origin/master", "main", "master"];

/// The answer to `wanted` about the checkout at `cwd`.
pub(crate) fn answer(cwd: &Path, wanted: &Wanted) -> Answer {
    let answered = match wanted {
        Wanted::Files { scope } => files(cwd, *scope).map(Answer::Files),
        Wanted::Diff { scope, path } => diff(cwd, *scope, path).map(Answer::Diff),
        Wanted::Checks => Ok(Answer::Checks(Default::default())),
        Wanted::CheckLog { .. } => Err(Failure::NoLog),
    };
    answered.unwrap_or_else(Answer::Failed)
}

/// What the changes are compared with, and the branch it stands for.
struct Compared {
    rev: String,
    base: Option<String>,
}

fn compared(cwd: &Path, scope: Scope) -> Result<Compared, Failure> {
    let head = git(cwd, &["rev-parse", "--verify", "--quiet", "HEAD"]).ok();
    if scope == Scope::Uncommitted {
        let rev = match head {
            Some(head) => head.trim().to_owned(),
            // Before a first commit, the empty tree in the repository's own
            // object format (SHA-1 or SHA-256), never written.
            None => git(cwd, &["hash-object", "-t", "tree", "--stdin"])
                .map_err(|_| Failure::Unreadable)?
                .trim()
                .to_owned(),
        };
        return Ok(Compared { rev, base: None });
    }
    head.ok_or(Failure::NoBase)?;
    let origin = git(
        cwd,
        &[
            "symbolic-ref",
            "--quiet",
            "--short",
            "refs/remotes/origin/HEAD",
        ],
    )
    .ok()
    .map(|name| name.trim().to_owned());
    let base = origin
        .into_iter()
        .chain(BASES.iter().map(|base| (*base).to_owned()))
        .find(|base| git(cwd, &["rev-parse", "--verify", "--quiet", base]).is_ok())
        .ok_or(Failure::NoBase)?;
    let rev = git(cwd, &["merge-base", "HEAD", &base]).map_err(|_| Failure::NoBase)?;
    let shown = base.strip_prefix("origin/").unwrap_or(&base).to_owned();
    Ok(Compared {
        rev: rev.trim().to_owned(),
        base: Some(shown),
    })
}

/// The checkout `cwd` is in, by its top folder: every path below is
/// relative to it, as `git diff` gives them, whatever folder in it the
/// workspace names.
fn root(cwd: &Path) -> Result<PathBuf, Failure> {
    let top = git(cwd, &["rev-parse", "--show-toplevel"]).map_err(|_| Failure::NoRepository)?;
    let top = top.trim_end_matches(['\n', '\r']);
    if top.is_empty() {
        return Err(Failure::NoRepository);
    }
    Ok(PathBuf::from(top))
}

fn files(cwd: &Path, scope: Scope) -> Result<FileList, Failure> {
    let root = root(cwd)?;
    let compared = compared(&root, scope)?;
    Ok(bounded(
        &root,
        compared.base,
        changed(&root, &compared.rev)?,
    ))
}

/// The list a phone is sent of `files` in the checkout at `root`, with the
/// untracked ones it holds read. Only those sent are read: a checkout may
/// hold many untracked files.
fn bounded(root: &Path, base: Option<String>, files: Vec<ChangedFile>) -> FileList {
    let mut list = FileList::bounded(base, files);
    for file in list
        .files
        .iter_mut()
        .filter(|file| file.status == FileStatus::Untracked && file.shown == Shown::Text)
    {
        match read_untracked(&root.join(&file.path)) {
            Ok(text) => file.added = u32::try_from(text.lines().count()).ok(),
            Err(shown) => file.shown = shown,
        }
    }
    list
}

/// Every file changed since `rev`, in the working tree as it is now.
fn changed(cwd: &Path, rev: &str) -> Result<Vec<ChangedFile>, Failure> {
    let statuses = git_bytes(
        cwd,
        &[
            "diff",
            "--no-ext-diff",
            "--name-status",
            "-z",
            "-M",
            rev,
            "--",
        ],
    )
    .map_err(|_| Failure::Unreadable)?;
    let counts = git(
        cwd,
        &[
            "diff",
            "--no-ext-diff",
            "--no-textconv",
            "--numstat",
            "-z",
            "-M",
            rev,
            "--",
        ],
    )
    .map_err(|_| Failure::Unreadable)?;
    let untracked = git_bytes(cwd, &["ls-files", "--others", "--exclude-standard", "-z"])
        .map_err(|_| Failure::Unreadable)?;
    // Names that are not UTF-8, as they read with their bytes replaced:
    // such a name would name no file back to Git.
    let undecodable: std::collections::HashSet<String> = [&statuses, &untracked]
        .into_iter()
        .flat_map(|listed| listed.split(|byte| *byte == 0))
        .filter(|name| std::str::from_utf8(name).is_err())
        .map(|name| String::from_utf8_lossy(name).into_owned())
        .collect();
    let (statuses, untracked) = (
        String::from_utf8_lossy(&statuses),
        String::from_utf8_lossy(&untracked),
    );
    let mut files = parse_name_status(&statuses);
    let named = |file: &ChangedFile| !undecodable.contains(&file.path);
    for (path, added, removed) in parse_numstat(&counts) {
        if let Some(file) = files.iter_mut().find(|file| file.path == path) {
            file.added = added;
            file.removed = removed;
            if added.is_none() {
                file.shown = Shown::Binary;
            }
        }
    }
    // Listed, not read: their counts are filled in only for those sent.
    files.extend(
        untracked
            .split('\0')
            .filter(|path| !path.is_empty())
            .map(|path| ChangedFile {
                path: path.to_owned(),
                status: FileStatus::Untracked,
                ..ChangedFile::default()
            }),
    );
    // Listed, but not read.
    for file in files.iter_mut().filter(|file| !named(file)) {
        file.shown = Shown::Unreadable;
        (file.added, file.removed) = (None, None);
    }
    Ok(files)
}

fn diff(cwd: &Path, scope: Scope, path: &str) -> Result<FileDiff, Failure> {
    if path.is_empty() || path.len() > MAX_PATH {
        return Err(Failure::NoFile);
    }
    let root = root(cwd)?;
    let cwd = root.as_path();
    let compared = compared(cwd, scope)?;
    // Only a file the list gives: a phone does not get to read any path.
    let file = changed(cwd, &compared.rev)?
        .into_iter()
        .find(|file| file.path == path)
        .ok_or(Failure::NoFile)?;
    if file.shown != Shown::Text {
        return Ok(FileDiff::note(path, file.shown));
    }
    if file.status == FileStatus::Untracked {
        let text = match read_untracked(&cwd.join(path)) {
            Ok(text) => text,
            Err(shown) => return Ok(FileDiff::note(path, shown)),
        };
        let count = text.lines().count();
        let hunk = DiffLine {
            kind: LineKind::Hunk,
            old: None,
            new: None,
            text: format!("@@ -0,0 +1,{count} @@"),
        };
        let added = text.lines().zip(1..).map(|(line, number)| DiffLine {
            kind: LineKind::Added,
            old: None,
            new: Some(number),
            text: line.to_owned(),
        });
        return Ok(FileDiff::bounded(path, std::iter::once(hunk).chain(added)));
    }
    let mut args = vec![
        "diff",
        "--no-color",
        "--no-ext-diff",
        "--no-textconv",
        "-M",
        compared.rev.as_str(),
        "--",
    ];
    args.extend(file.old_path.as_deref());
    args.push(path);
    let (text, cut) =
        git_bounded(cwd, &args, MAX_DIFF_BYTES as u64 * 2).map_err(|_| Failure::Unreadable)?;
    let text = String::from_utf8_lossy(&text);
    let mut diff = FileDiff::bounded(path, parse_diff(section(&text, path)));
    diff.truncated |= cut;
    Ok(diff)
}

/// An untracked file's text, or why it is not shown. Only a regular file
/// is read: a link could lead anywhere outside the checkout.
fn read_untracked(path: &Path) -> Result<String, Shown> {
    let kind = std::fs::symlink_metadata(path).map_err(|_| Shown::Unreadable)?;
    if !kind.file_type().is_file() {
        return Err(Shown::Unreadable);
    }
    let file = std::fs::File::open(path).map_err(|_| Shown::Unreadable)?;
    let mut bytes = Vec::new();
    file.take(MAX_UNTRACKED + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Shown::Unreadable)?;
    if bytes.len() as u64 > MAX_UNTRACKED {
        return Err(Shown::TooLarge);
    }
    if bytes.contains(&0) {
        return Err(Shown::Binary);
    }
    String::from_utf8(bytes).map_err(|_| Shown::Binary)
}

/// `git diff --name-status -z` as files.
fn parse_name_status(text: &str) -> Vec<ChangedFile> {
    let mut fields = text.split('\0').filter(|field| !field.is_empty());
    let mut files = Vec::new();
    while let Some(code) = fields.next() {
        let status = match code.chars().next() {
            Some('A') => FileStatus::Added,
            Some('D') => FileStatus::Deleted,
            Some('R') => FileStatus::Renamed,
            _ => FileStatus::Modified,
        };
        let old_path = if status == FileStatus::Renamed || code.starts_with('C') {
            fields.next().map(str::to_owned)
        } else {
            None
        };
        let Some(path) = fields.next() else {
            break;
        };
        files.push(ChangedFile {
            path: path.to_owned(),
            old_path: old_path.filter(|_| status == FileStatus::Renamed),
            status,
            ..ChangedFile::default()
        });
    }
    files
}

/// `git diff --numstat -z` as each file's path and counts; `None` counts
/// for a binary file.
fn parse_numstat(text: &str) -> Vec<(String, Option<u32>, Option<u32>)> {
    let mut fields = text.split('\0');
    let mut counts = Vec::new();
    while let Some(field) = fields.next() {
        let mut parts = field.splitn(3, '\t');
        let (Some(added), Some(removed), Some(path)) = (parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        // A rename names no path here: the old and new ones follow.
        let path = if path.is_empty() {
            let _old = fields.next();
            fields.next().unwrap_or_default()
        } else {
            path
        };
        counts.push((path.to_owned(), added.parse().ok(), removed.parse().ok()));
    }
    counts
}

/// The part of a diff of several files that is `path`'s: naming a
/// rename's old path can bring in a new file now at that path. A section
/// is `path`'s when its header gives it as the new or renamed name, or as
/// the deleted one. One section whose names are quoted is taken as is.
fn section<'a>(text: &'a str, path: &str) -> &'a str {
    let mut sections = Vec::new();
    let mut start = None;
    for (at, _) in text.match_indices("diff --git ") {
        if at != 0 && text.as_bytes()[at - 1] != b'\n' {
            continue;
        }
        if let Some(from) = start.replace(at) {
            sections.push(&text[from..at]);
        }
    }
    if let Some(from) = start {
        sections.push(&text[from..]);
    }
    let (new, old) = (format!("b/{path}"), format!("a/{path}"));
    let found = sections.iter().find(|section| {
        let header = section.split("\n@@").next().unwrap_or_default();
        let named = |prefix: &str, name: &str| {
            header
                .lines()
                .filter_map(|line| line.strip_prefix(prefix))
                .any(|rest| unquoted(rest) == name)
        };
        named("+++ ", &new)
            || named("rename to ", path)
            || named("copy to ", path)
            || (named("--- ", &old) && header.contains("\n+++ /dev/null"))
    });
    match (found, sections.as_slice()) {
        (Some(found), _) => found,
        (None, [only]) => only,
        (None, _) => "",
    }
}

/// A name in a diff header as Git wrote it: quoted, with C escapes, when
/// it holds a quote, a backslash, or a control character.
fn unquoted(name: &str) -> std::borrow::Cow<'_, str> {
    let Some(inner) = name
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
    else {
        return name.into();
    };
    let mut bytes = Vec::with_capacity(inner.len());
    let mut rest = inner.bytes();
    while let Some(byte) = rest.next() {
        if byte != b'\\' {
            bytes.push(byte);
            continue;
        }
        let Some(escaped) = rest.next() else {
            break;
        };
        bytes.push(match escaped {
            b'a' => 0x07,
            b'b' => 0x08,
            b'f' => 0x0c,
            b'n' => b'\n',
            b'r' => b'\r',
            b't' => b'\t',
            b'v' => 0x0b,
            b'0'..=b'7' => {
                let digits = [Some(escaped), rest.next(), rest.next()];
                digits.into_iter().flatten().fold(0_u8, |value, digit| {
                    value.wrapping_mul(8).wrapping_add(digit - b'0')
                })
            }
            other => other,
        });
    }
    String::from_utf8_lossy(&bytes).into_owned().into()
}

/// A unified diff's lines from its first hunk on, numbered.
fn parse_diff(text: &str) -> Vec<DiffLine> {
    let mut lines = Vec::new();
    let (mut old, mut new) = (0_u32, 0_u32);
    let mut in_hunk = false;
    for line in text.lines() {
        if let Some((from, to)) = hunk_start(line) {
            (old, new, in_hunk) = (from, to, true);
            lines.push(DiffLine {
                kind: LineKind::Hunk,
                old: None,
                new: None,
                text: line.to_owned(),
            });
            continue;
        }
        if !in_hunk {
            continue;
        }
        let (kind, rest) = match line.split_at_checked(1) {
            Some(("+", rest)) => (LineKind::Added, rest),
            Some(("-", rest)) => (LineKind::Removed, rest),
            Some((" ", rest)) => (LineKind::Context, rest),
            Some(("\\", _)) => (LineKind::Meta, line),
            _ if line.is_empty() => (LineKind::Context, ""),
            // The next file's header: one file was asked for.
            _ => break,
        };
        let (number_old, number_new) = match kind {
            LineKind::Added => (None, Some(new)),
            LineKind::Removed => (Some(old), None),
            LineKind::Context => (Some(old), Some(new)),
            LineKind::Hunk | LineKind::Meta => (None, None),
        };
        match kind {
            LineKind::Added => new += 1,
            LineKind::Removed => old += 1,
            LineKind::Context => (old, new) = (old + 1, new + 1),
            LineKind::Hunk | LineKind::Meta => {}
        }
        lines.push(DiffLine {
            kind,
            old: number_old,
            new: number_new,
            text: rest.to_owned(),
        });
    }
    lines
}

/// Where a hunk header `@@ -a,b +c,d @@` starts on each side.
fn hunk_start(line: &str) -> Option<(u32, u32)> {
    let rest = line.strip_prefix("@@ -")?;
    let (old, rest) = rest.split_once(" +")?;
    let (new, _) = rest.split_once(" @@")?;
    let start = |side: &str| side.split(',').next()?.parse().ok();
    Some((start(old)?, start(new)?))
}

fn command(cwd: &Path, args: &[&str]) -> Command {
    let mut command = sheprd_client::new_command("git");
    command
        .arg("-C")
        .arg(cwd)
        // Paths are literal, never patterns, and the user's colors, diff
        // tools, and conversions stay out of what is parsed.
        .args(["-c", "core.quotepath=off", "-c", "color.ui=false"])
        .args(["--no-optional-locks", "--literal-pathspecs"])
        .args(args)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    command
}

/// Git's output for `args` in `cwd`, when it succeeded.
fn git(cwd: &Path, args: &[&str]) -> std::io::Result<String> {
    Ok(String::from_utf8_lossy(&git_bytes(cwd, args)?).into_owned())
}

/// Git's output for `args` in `cwd` as it wrote it, when it succeeded.
fn git_bytes(cwd: &Path, args: &[&str]) -> std::io::Result<Vec<u8>> {
    let (bytes, cut) = git_bounded(cwd, args, MAX_LISTING)?;
    if cut {
        return Err(std::io::Error::other("Git's answer is too long"));
    }
    Ok(bytes)
}

/// At most `limit` bytes of Git's output for `args`, and whether it was
/// cut there; Git stops once nothing reads the rest.
fn git_bounded(cwd: &Path, args: &[&str], limit: u64) -> std::io::Result<(Vec<u8>, bool)> {
    let mut child = command(cwd, args).spawn()?;
    let stdout = child.stdout.take();
    // Read beside the wait, so a Git that never ends is ended at the limit.
    let (sender, read) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("remote-git-read".into())
        .spawn(move || {
            let mut bytes = Vec::new();
            let result = stdout.map_or(Ok(0), |stdout| {
                stdout.take(limit + 1).read_to_end(&mut bytes)
            });
            let _ = sender.send(result.map(|_| bytes));
        })?;
    let mut bytes = match read.recv_timeout(GIT_TIMEOUT) {
        Ok(read) => read?,
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(std::io::Error::from(std::io::ErrorKind::TimedOut));
        }
    };
    let cut = bytes.len() as u64 > limit;
    if cut {
        bytes.truncate(usize::try_from(limit).unwrap_or(usize::MAX));
        let _ = child.kill();
    }
    let status = child.wait()?;
    if !cut && !status.success() {
        return Err(std::io::Error::other("Git failed"));
    }
    Ok((bytes, cut))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests;
