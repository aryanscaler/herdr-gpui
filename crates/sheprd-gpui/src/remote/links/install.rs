//! Getting `herdr-remote-control` onto a host before a phone is enrolled
//! there. The build comes from the first of these that fits the host:
//! - for a Linux host and a release, the release's static build for the
//!   host's architecture, checked against the release's signed host
//!   manifest (`sheprd_updater::download_host_build`) and kept in this
//!   app's state, so each version downloads once. It comes first even when
//!   this computer runs Linux on the same architecture: this app's own
//!   build links glibc, which Alpine or an older distribution lacks, and
//!   `uname` cannot tell them apart;
//! - this app's own, when the host is this kind of computer, or any Mac
//!   when this is a release, whose build is universal;
//! - for a development build, this checkout's source as it is, built on
//!   the host with the host's own Rust toolchain.
//!
//! Every copy goes through SSH, so the host needs no internet. A copy lands
//! in `~/.local/share/herdr-remote-control/bin/VERSION/` and is skipped when
//! the host holds the same bytes there; a source build is skipped when that
//! snapshot was built there before.
//!
//! So the phone's chats work there, `sheprd-acp` goes beside it: this app's
//! own when that is what the host gets (a Mac from a Mac release, or a host
//! like this computer from a development build), the release's static one
//! for a Linux host from a release (`sheprd_updater::download_host_chat`,
//! checked against the signed manifest packed with it), and a source build
//! builds it too. It is never linked into `~/.local/bin`, where it would
//! replace one the user installed; the daemon starts the one beside it
//! first.

use super::{Error, PROGRAM};
use crate::{
    host_build::{self, Platform},
    teleport::Host,
};
use sheprd_client::{ScriptLimits, shell_quote};
use std::{
    io::Read,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        Mutex, PoisonError,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};

/// A slow link can take a while to carry a few megabytes.
const COPY_IDLE: Duration = Duration::from_secs(300);
const COPY_OUTPUT: u64 = 64 * 1024;
/// A release build of a large crate can print nothing for minutes.
const BUILD_IDLE: Duration = Duration::from_secs(20 * 60);
const BUILD_OUTPUT: u64 = 64 * 1024;
/// The script's exit status when the host has no Rust toolchain.
const NO_RUST_EXIT: i32 = 4;

/// One download at a time: pairing a phone with two hosts of the same
/// kind starts two jobs for the same build, and the second then finds it
/// in place instead of racing the first.
static DOWNLOADS: Mutex<()> = Mutex::new(());

/// A name no other job in this process uses, for its temporary files.
fn unique(stem: &str) -> String {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    format!(
        "{stem}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}

/// Where the build for a host comes from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Choice {
    /// This app's own build.
    Bundled(PathBuf),
    /// The release's static build for this target.
    Download(String),
    /// This checkout's source, built on the host.
    Source,
}

/// Which build fits a host on `platform`. `universal` says whether
/// `bundled` runs on both Mac architectures.
pub(crate) fn choose(
    platform: &Platform,
    release: bool,
    bundled: Option<PathBuf>,
    universal: bool,
) -> Option<Choice> {
    if release && platform.os == "linux" {
        let target = format!("{}-unknown-linux-musl", platform.arch);
        if sheprd_updater::HOST_TARGETS.contains(&target.as_str()) {
            return Some(Choice::Download(target));
        }
    }
    let fits = |_: &PathBuf| {
        *platform == Platform::local()
            || (platform.os == "macos" && Platform::local().os == "macos" && universal)
    };
    if let Some(bundled) = bundled.filter(fits) {
        return Some(Choice::Bundled(bundled));
    }
    (!release).then_some(Choice::Source)
}

/// Puts a build on `host` unless it is there already; the quoted path to
/// run it by. Blocks.
pub(super) fn ensure(host: &Host, cancelled: &AtomicBool) -> Result<String, Error> {
    let uname = host.capture("uname -sm\n", cancelled).map_err(Error::Run)?;
    let platform = Platform::parse(&String::from_utf8_lossy(&uname))?;
    let bundled = crate::bundled::locate(crate::remote::service::PROGRAM);
    let universal = bundled.as_deref().is_some_and(is_universal);
    match choose(&platform, crate::RELEASE_BUILD, bundled, universal) {
        Some(Choice::Bundled(path)) => {
            let program = copy(host, &path, crate::APP_VERSION, cancelled)?;
            add_chats(host, &platform, crate::APP_VERSION, cancelled);
            Ok(program)
        }
        Some(Choice::Download(target)) => {
            let path = download(&target, Build::Daemon, cancelled)?;
            let program = copy(host, &path, crate::APP_VERSION, cancelled)?;
            // Pairing goes on without it: chats then need one the user installed.
            let chats = download(&target, Build::Chats, cancelled)
                .and_then(|chats| copy_chats(host, &chats, crate::APP_VERSION, cancelled));
            if let Err(error) = chats {
                tracing::warn!(%error, "Could not put the release's sheprd-acp on a Linux host");
            }
            Ok(program)
        }
        Some(Choice::Source) => build_from_source(host, cancelled),
        None => Err(Error::NoBuild {
            platform: platform.to_string(),
        }),
    }
}

/// Puts this app's `sheprd-acp` beside the daemon's build of `version`,
/// when it fits the host. Pairing goes on without it: chats then need one
/// the user installed, as before.
fn add_chats(host: &Host, platform: &Platform, version: &str, cancelled: &AtomicBool) {
    let Some(path) = crate::bundled::locate(CHAT_PROGRAM) else {
        return;
    };
    let fits = *platform == Platform::local()
        || (platform.os == "macos" && Platform::local().os == "macos" && is_universal(&path));
    if !fits {
        return;
    }
    if let Err(error) = copy_chats(host, &path, version, cancelled) {
        tracing::warn!(%error, "Could not put sheprd-acp beside remote control on a host");
    }
}

fn copy_chats(
    host: &Host,
    path: &Path,
    version: &str,
    cancelled: &AtomicBool,
) -> Result<(), Error> {
    let (_, sha256) = host_build::digest(CHAT_PROGRAM, path)?;
    let installed = host
        .capture(&chat_probe_script(version), cancelled)
        .map_err(Error::Run)?;
    if String::from_utf8_lossy(&installed).trim() == sha256 {
        return Ok(());
    }
    let file = std::fs::File::open(path).map_err(|source| host_build::Error::Binary {
        program: CHAT_PROGRAM,
        path: path.to_owned(),
        source,
    })?;
    host.stream(
        &chat_copy_script(version, &sha256),
        file,
        Vec::new(),
        ScriptLimits {
            output: COPY_OUTPUT,
            idle: COPY_IDLE,
        },
        cancelled,
    )
    .map_err(Error::Run)?;
    Ok(())
}

/// The chat program the host's daemon starts, by the name it looks for
/// beside itself.
const CHAT_PROGRAM: &str = "sheprd-acp";

/// Where the daemon's build of `version` keeps `sheprd-acp` on the host.
pub(crate) fn chat_path(version: &str) -> String {
    format!(
        "\"$HOME/.local/share/{PROGRAM}/bin/\"{}\"/{CHAT_PROGRAM}\"", // i18n: data
        shell_quote(version)
    )
}

/// Prints the SHA-256 of the `sheprd-acp` beside the daemon of `version`.
pub(crate) fn chat_probe_script(version: &str) -> String {
    format!(
        // i18n: data
        "file={}\n\
         if [ -f \"$file\" ]; then\n\
         if command -v sha256sum >/dev/null 2>&1; then sum=$(sha256sum \"$file\"); else sum=$(shasum -a 256 \"$file\"); fi\n\
         echo \"${{sum%% *}}\"\n\
         fi\n",
        chat_path(version)
    )
}

/// Takes `sheprd-acp` on stdin, checks it against `sha256`, and puts it
/// beside the daemon of `version`, linking nothing.
pub(crate) fn chat_copy_script(version: &str, sha256: &str) -> String {
    format!(
        // i18n: data
        r#"file={file}
sum={sum}
mkdir -p "$(dirname "$file")"
part="$file.part.$$"
trap 'rm -f "$part"' EXIT
cat > "$part"
if command -v sha256sum >/dev/null 2>&1; then got=$(sha256sum "$part"); else got=$(shasum -a 256 "$part"); fi
got=${{got%% *}}
if [ "$got" != "$sum" ]; then echo "the copy does not match its checksum" >&2; exit {checksum}; fi
chmod 755 "$part"
mv -f "$part" "$file"
"#,
        file = chat_path(version),
        sum = shell_quote(sha256),
        checksum = host_build::CHECKSUM_EXIT,
    )
}

/// Whether the Mach-O at `path` holds several architectures.
fn is_universal(path: &Path) -> bool {
    let mut magic = [0; 4];
    std::fs::File::open(path)
        .and_then(|mut file| file.read_exact(&mut magic))
        .is_ok_and(|()| magic == [0xca, 0xfe, 0xba, 0xbe])
}

/// Copies `path` to the host as `version` unless the host holds the same
/// bytes there.
fn copy(host: &Host, path: &Path, version: &str, cancelled: &AtomicBool) -> Result<String, Error> {
    let (_, sha256) = host_build::digest(PROGRAM, path)?;
    let installed = host
        .capture(&probe_script(version), cancelled)
        .map_err(Error::Run)?;
    if String::from_utf8_lossy(&installed).trim() != sha256 {
        let file = std::fs::File::open(path).map_err(|source| host_build::Error::Binary {
            program: PROGRAM,
            path: path.to_owned(),
            source,
        })?;
        host.stream(
            &host_build::copy_script(PROGRAM, version, &sha256),
            file,
            Vec::new(),
            ScriptLimits {
                output: COPY_OUTPUT,
                idle: COPY_IDLE,
            },
            cancelled,
        )
        .map_err(|error| match &error {
            sheprd_client::Error::ScriptExit { status, .. }
                if status.code() == Some(host_build::CHECKSUM_EXIT) =>
            {
                Error::Build(host_build::Error::Checksum)
            }
            _ => Error::Run(error),
        })?;
    }
    Ok(installed_path(version))
}

/// Which of the release's static builds for another machine.
#[derive(Clone, Copy, Debug)]
enum Build {
    /// `sheprd-acp`, which chats there run.
    Chats,
    /// `sheprd-remote-control`.
    Daemon,
}

/// The release's static `build` for `target`, downloaded once per version
/// into this app's private state. Blocks.
fn download(target: &str, build: Build, cancelled: &AtomicBool) -> Result<PathBuf, Error> {
    let dir = crate::remote::state_dir()
        .map_err(|_| Error::NoStateDir)?
        .join("host-builds")
        .join(crate::APP_VERSION);
    let path = match build {
        Build::Chats => dir.join(format!("{CHAT_PROGRAM}-{target}")),
        Build::Daemon => dir.join(target),
    };
    let _one = DOWNLOADS.lock().unwrap_or_else(PoisonError::into_inner);
    if path.is_file() {
        return Ok(path);
    }
    let failed = |source| Error::Cache {
        path: dir.clone(),
        source,
    };
    std::fs::create_dir_all(&dir).map_err(failed)?;
    let part = dir.join(format!(".{}.part", unique(target)));
    let downloaded = match build {
        Build::Chats => sheprd_updater::download_host_chat(target, &part, cancelled),
        Build::Daemon => sheprd_updater::download_host_build(target, &part, cancelled),
    }
    .map_err(|source| Error::Download {
        target: target.to_owned(),
        source,
    });
    if let Err(error) = downloaded {
        let _ = std::fs::remove_file(&part);
        return Err(error);
    }
    std::fs::rename(&part, &path).map_err(failed)?;
    Ok(path)
}

/// Builds this checkout's source on the host, unless that snapshot was
/// built there already.
fn build_from_source(host: &Host, cancelled: &AtomicBool) -> Result<String, Error> {
    let root = source_root().ok_or(Error::NoSource)?;
    let tree = snapshot(&root)?;
    let version = format!("dev-{}", &tree[..12]);
    let built = host
        .capture(
            &format!(
                "[ -x {} ] && echo built || true\n", // i18n: data
                installed_path(&version)
            ),
            cancelled,
        )
        .map_err(Error::Run)?;
    if String::from_utf8_lossy(&built).trim() == "built" {
        return Ok(installed_path(&version));
    }
    let mut archive = sheprd_client::new_command("git")
        .arg("-C")
        .arg(&root)
        .args(["archive", "--format=tar.gz", &tree])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(Error::Git)?;
    let source = archive.stdout.take().ok_or(Error::NoSource)?;
    let built = host.stream(
        &build_script(&version),
        source,
        Vec::new(),
        ScriptLimits {
            output: BUILD_OUTPUT,
            idle: BUILD_IDLE,
        },
        cancelled,
    );
    // The archive is read to its end or the build failed; either way git
    // is done with.
    let _ = archive.kill();
    let _ = archive.wait();
    built.map_err(|error| match &error {
        sheprd_client::Error::ScriptExit { status, .. } if status.code() == Some(NO_RUST_EXIT) => {
            Error::NoRust
        }
        _ => Error::Run(error),
    })?;
    Ok(installed_path(&version))
}

/// The checkout this development build was made from, while it is there.
fn source_root() -> Option<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent()?.parent()?;
    (root.join("Cargo.toml").is_file() && root.join(".git").exists()).then(|| root.to_owned()) // i18n: data
}

/// A tree of the checkout as it is now: uncommitted changes and files
/// not yet added included, what `.gitignore` leaves out excluded. It is
/// built in a temporary index, so the checkout's own index, stash, and
/// working tree stay as they are.
fn snapshot(root: &Path) -> Result<String, Error> {
    // Its own index: two pairings may snapshot the checkout at once.
    let index =
        std::env::temp_dir().join(format!("{}.index", unique("herdr-remote-control-snapshot")));
    let git = |args: &[&str]| -> Result<String, Error> {
        let output = sheprd_client::new_command("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .env("GIT_INDEX_FILE", &index)
            .stdin(Stdio::null())
            .output()
            .map_err(Error::Git)?;
        if !output.status.success() {
            return Err(Error::NoSource);
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    };
    let tree = git(&["read-tree", "HEAD"])
        .and_then(|_| git(&["add", "--all"]))
        .and_then(|_| git(&["write-tree"]));
    let _ = std::fs::remove_file(&index);
    let tree = tree?;
    if tree.len() != 40 || !tree.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(Error::NoSource);
    }
    Ok(tree)
}

/// The host script that takes the source as a gzipped tar on stdin, builds
/// `herdr-remote-control` with the host's Rust (installing the checkout's
/// pinned toolchain through rustup when it can), and puts it in place as
/// `version`. Build outputs stay in `~/.cache`, so the next snapshot builds
/// incrementally.
pub(crate) fn build_script(version: &str) -> String {
    format!(
        // i18n: data
        r#"version={version}
cache="$HOME/.cache/{PROGRAM}"
# Each build unpacks into its own folder: two phones pairing this host at
# once must not unpack over each other. Cargo locks the shared target.
src="$cache/src-$$"
trap 'rm -rf "$src"' EXIT
rm -rf "$src"
mkdir -p "$src" "$cache/target"
tar -xzf - -C "$src"
if ! command -v cargo >/dev/null 2>&1; then
  echo "Rust is not installed on this machine" >&2
  exit {NO_RUST_EXIT}
fi
cd "$src"
if command -v rustup >/dev/null 2>&1; then rustup show >/dev/null 2>&1 || true; fi
CARGO_TARGET_DIR="$cache/target" cargo build --release --locked -p sheprd-remote-control >&2
dir="$HOME/.local/share/{PROGRAM}/bin/$version"
mkdir -p "$dir" "$HOME/.local/bin"
# The phone's chats run the sheprd-acp beside it; without one they need the
# user's own, so a failed build or copy of it fails nothing else. Each job
# copies through its own file: two pairings may run here at once.
chat_part="$dir/.{CHAT_PROGRAM}.part.$$"
if CARGO_TARGET_DIR="$cache/target" cargo build --release --locked -p sheprd-acp >&2 \
  && cp "$cache/target/release/sheprd-acp" "$chat_part" \
  && chmod 755 "$chat_part" \
  && mv -f "$chat_part" "$dir/{CHAT_PROGRAM}"; then
  :
else
  rm -f "$chat_part"
fi
cp "$cache/target/release/sheprd-remote-control" "$dir/.{PROGRAM}.part"
chmod 755 "$dir/.{PROGRAM}.part"
mv -f "$dir/.{PROGRAM}.part" "$dir/{PROGRAM}"
ln -sfn "$dir/{PROGRAM}" "$HOME/.local/bin/{PROGRAM}"
ln -sfn "$dir/{PROGRAM}" "$HOME/.local/bin/sheprd-remote-control"
"#,
        version = shell_quote(version),
    )
}

/// Prints the SHA-256 of the build of `version` the host holds, or nothing.
pub(crate) fn probe_script(version: &str) -> String {
    format!(
        // i18n: data
        "file={}\n\
         if [ -f \"$file\" ]; then\n\
         if command -v sha256sum >/dev/null 2>&1; then sum=$(sha256sum \"$file\"); else sum=$(shasum -a 256 \"$file\"); fi\n\
         echo \"${{sum%% *}}\"\n\
         fi\n",
        installed_path(version)
    )
}

/// Where a build of `version` lives on a host, as a shell word.
pub(crate) fn installed_path(version: &str) -> String {
    format!(
        "\"$HOME/.local/share/{PROGRAM}/bin/\"{}\"/{PROGRAM}\"",
        shell_quote(version)
    )
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests;
