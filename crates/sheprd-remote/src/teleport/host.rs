//! One host Teleport acts on: where its scripts run and which Herdr session
//! its `herdr` CLI addresses.

use super::error::{Error, Result, Step, decode, script};
use serde::de::DeserializeOwned;
use sheprd_client::{ConnectTarget, ScriptHost, ScriptLimits, SshLane, run_script, shell_quote};
use std::{
    io::{Read, Write},
    path::PathBuf,
    sync::atomic::AtomicBool,
    time::Duration,
};

mod local;

/// Output bound for queries; bulk transfers pass their own limits.
const QUERY_OUTPUT: u64 = 32 * 1024 * 1024;
const QUERY_IDLE: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, PartialEq, Eq)]
enum Route {
    Local,
    Ssh(String),
    Wsl(String),
    #[cfg(feature = "cloud")]
    Cloud {
        provider: sheprd_client::CloudProvider,
        account: String,
        id: String,
    },
}

/// What herdr-core reads to choose the session and socket its CLI talks to,
/// as `sheprd_core_client::Endpoint` resolves them.
const CORE_ENV: &[&str] = &[
    // keep-sorted start
    "HC_SESSION",
    "HC_SOCKET",
    "XDG_CONFIG_HOME",
    "XDG_RUNTIME_DIR",
    "XDG_STATE_HOME",
    // keep-sorted end
];

/// Largest bundle, session, or archive Teleport will carry.
pub const TRANSFER_OUTPUT: u64 = 4 * 1024 * 1024 * 1024;
/// Bulk Git work (bundling, fetching) can be quiet for a while.
pub const TRANSFER_IDLE: Duration = Duration::from_secs(300);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Host {
    route: Route,
    session: Option<String>,
    /// The session belongs to the development installation, which
    /// `herdr --session NAME` cannot address: NAME would resolve to the
    /// release session that shares it. Scripts still run (Git, `bd`), but
    /// `herdr_cli` refuses.
    development: bool,
    /// Tests run "remote" hosts locally in their own environment (home,
    /// daemon socket), exported at the start of every script.
    #[cfg(any(test, feature = "test-support"))]
    pub env: Vec<(String, String)>,
}

impl Host {
    /// How long a query may go without output.
    pub const QUERY_IDLE: Duration = QUERY_IDLE;
    /// Most a query may print.
    pub const QUERY_OUTPUT: u64 = QUERY_OUTPUT;

    /// Names the daemon this host scripts: its route, session, and
    /// installation, so a saved reference is never applied to another
    /// daemon behind the same endpoint ID.
    pub fn identity(&self) -> String {
        let route = match &self.route {
            Route::Local => "local".to_owned(),
            Route::Ssh(target) => format!("ssh:{target}"),
            Route::Wsl(distro) => format!("wsl:{distro}"),
            #[cfg(feature = "cloud")]
            Route::Cloud {
                provider,
                account,
                id,
            } => format!("cloud:{provider:?}:{account}:{id}"),
        };
        let session = self.session.as_deref().unwrap_or_default();
        let installation = if self.development { "dev" } else { "release" };
        format!("{route}|{session}|{installation}")
    }

    /// A host for `target`. Custom sockets are refused: the CLI could not be
    /// pointed at them without also trusting that socket's daemon identity.
    pub fn new(target: &ConnectTarget) -> Result<Self> {
        match target {
            ConnectTarget::Local => Ok(Self {
                route: Route::Local,
                session: None,
                development: false,
                #[cfg(any(test, feature = "test-support"))]
                env: Vec::new(),
            }),
            ConnectTarget::Session { name, development } => Ok(Self {
                route: Route::Local,
                session: Some(name.clone()),
                development: *development,
                #[cfg(any(test, feature = "test-support"))]
                env: Vec::new(),
            }),
            ConnectTarget::Ssh { target, session } => Ok(Self {
                route: Route::Ssh(target.clone()),
                session: Some(session.clone()),
                development: false,
                #[cfg(any(test, feature = "test-support"))]
                env: Vec::new(),
            }),
            ConnectTarget::Socket(_) => Err(Error::UnsupportedHost),
            ConnectTarget::Wsl { distro, session } => Ok(Self {
                route: Route::Wsl(distro.clone()),
                session: Some(session.clone()),
                development: false,
                #[cfg(any(test, feature = "test-support"))]
                env: Vec::new(),
            }),
            #[cfg(feature = "cloud")]
            ConnectTarget::Cloud {
                provider,
                account,
                id,
                session,
                ..
            } => Ok(Self {
                route: Route::Cloud {
                    provider: *provider,
                    account: account.clone(),
                    id: id.clone(),
                },
                session: Some(session.clone()),
                development: false,
                #[cfg(any(test, feature = "test-support"))]
                env: Vec::new(),
            }),
        }
    }

    /// Whether `herdr_cli` can reach this host's session; see `development`.
    pub fn scripts_herdr(&self) -> bool {
        !self.development
    }

    /// The prelude every script starts with: strict mode, a `PATH` that finds
    /// user-installed tools in a non-interactive SSH shell, and `herdr_cli`
    /// bound to this host's session, or refusing when it cannot be reached.
    fn prelude(&self) -> String {
        let session = self
            .session
            .as_deref()
            .map(|session| format!(" --session {}", shell_quote(session)))
            .unwrap_or_default();
        #[cfg(any(test, feature = "test-support"))]
        let home = self
            .env
            .iter()
            .map(|(key, value)| format!("{key}={}\nexport {key}\n", shell_quote(value)))
            .collect::<String>();
        #[cfg(not(any(test, feature = "test-support")))]
        let home = "";
        let herdr_cli = if self.development {
            r#"herdr_cli() { echo "herdr: a development (--dev) session cannot be scripted" >&2; return 97; }"#
                .to_owned()
        } else {
            format!(r#"herdr_cli() {{ herdr{session} "$@"; }}"#)
        };
        let core_cli = self.core_cli();
        format!(
            r#"set -eu
{home}herdr_linuxbrew=
case "$(uname -s)" in Linux) herdr_linuxbrew=/home/linuxbrew/.linuxbrew/bin:;; esac
PATH="$HOME/.local/bin:$HOME/.local/share/mise/shims:$HOME/.bun/bin:$HOME/.cargo/bin:/opt/homebrew/bin:/usr/local/bin:$herdr_linuxbrew$HOME/.nix-profile/bin:$PATH"
export PATH
{herdr_cli}
{core_cli}
"#
        )
    }

    /// `core_cli`, the host's herdr-core (Herdr 2) CLI: the user's own
    /// `herdr-core`, else, on this machine, the copy bundled beside the app,
    /// which is how the app itself starts a core when none runs.
    ///
    /// On this machine it runs with the login shell's values of what picks
    /// a core session, as the app resolves the core it joins, so a launch
    /// from the desktop never reaches another session's core.
    fn core_cli(&self) -> String {
        if !self.is_local() {
            // Found where the app's own bridge finds it, so a core that
            // opens Herdr 2 tabs there also takes the moved ones. `$USER`
            // may be unset in a non-interactive shell.
            let locate = sheprd_client::core_bridge::locate_script();
            return format!(
                r#"core_cli() (
set +u
{locate}
[ -n "$herdr_core" ] || {{ echo "herdr-core: not installed" >&2; exit 127; }}
exec "$herdr_core" "$@"
)"#
            );
        }
        let bundled = std::env::current_exe()
            .ok()
            .and_then(|exe| Some(exe.parent()?.join("herdr-core")))
            .filter(|path| path.is_file())
            .map(|path| shell_quote(&path.to_string_lossy()));
        let program = match bundled {
            Some(bundled) => format!(
                r#"if command -v herdr-core >/dev/null 2>&1; then herdr-core "$@"; else {bundled} "$@"; fi"#
            ),
            None => r#"herdr-core "$@""#.to_owned(),
        };
        let exports = self.core_exports();
        // A subshell keeps the exports to the core's CLI.
        format!("core_cli() ( {exports}{program} )")
    }

    fn core_exports(&self) -> String {
        if !self.is_local() {
            return String::new();
        }
        CORE_ENV
            .iter()
            .filter(|name| !self.sets(name))
            .filter_map(|name| {
                let value = sheprd_client::login_env::var(name)?;
                Some(format!(
                    "{name}={}; export {name}; ",
                    shell_quote(&value.to_string_lossy())
                ))
            })
            .collect()
    }

    /// Read-only discovery runs beside the core it queries, with exactly the
    /// same local socket/session overrides as `core_cli`. Never installs a
    /// helper or starts Herdr 1 to obtain session evidence.
    pub(super) fn session_discovery(&self, terminal: &str, agent: &str) -> String {
        let bundled = self
            .is_local()
            .then(|| std::env::current_exe().ok())
            .flatten()
            .and_then(|exe| Some(exe.parent()?.join("sheprd-acp")))
            .filter(|path| path.is_file())
            .map(|path| shell_quote(&path.to_string_lossy()));
        let bundled = bundled.unwrap_or_else(|| "''".into());
        format!(
            "( {exports}bundled={bundled}\n\
             helper=\n\
             if command -v sheprd-acp >/dev/null 2>&1; then helper=$(command -v sheprd-acp)\n\
             elif [ -n \"$bundled\" ] && [ -x \"$bundled\" ]; then helper=$bundled\n\
             else\n\
               daemon=$(command -v sheprd-remote-control || command -v herdr-remote-control || :)\n\
               links=0\n\
               while [ -n \"$daemon\" ] && [ -L \"$daemon\" ]; do\n\
                 links=$((links + 1)); [ \"$links\" -le 16 ] || exit 1\n\
                 link=$(readlink \"$daemon\") || exit 1\n\
                 case \"$link\" in /*) daemon=$link;; *) daemon=$(dirname \"$daemon\")/$link;; esac\n\
               done\n\
               if [ -n \"$daemon\" ] && [ -x \"$(dirname \"$daemon\")/sheprd-acp\" ]; then\n\
                 helper=$(dirname \"$daemon\")/sheprd-acp\n\
               else\n\
                 count=0\n\
                 for candidate in \"$HOME/.local/share/herdr-remote-control/bin/\"*/sheprd-acp; do\n\
                   [ -x \"$candidate\" ] || continue\n\
                   count=$((count + 1)); [ \"$count\" -le 64 ] || exit 1\n\
                   if \"$candidate\" --help 2>/dev/null | grep -Fq session-discover; then\n\
                     [ -z \"$helper\" ] || {{ echo 'Multiple session helpers; select sheprd-acp on PATH' >&2; exit 1; }}\n\
                     helper=$candidate\n\
                   fi\n\
                 done\n\
               fi\n\
             fi\n\
             [ -x \"$helper\" ] || exit 127\n\
             exec \"$helper\" session-discover {terminal} {agent}\n)\n",
            exports = self.core_exports(),
            terminal = shell_quote(terminal),
            agent = shell_quote(agent),
        )
    }

    /// Whether a test host's environment sets `name` itself.
    #[cfg(any(test, feature = "test-support"))]
    fn sets(&self, name: &str) -> bool {
        self.env.iter().any(|(key, _)| key == name)
    }

    #[cfg(not(any(test, feature = "test-support")))]
    fn sets(&self, _: &str) -> bool {
        false
    }

    /// Run `body` after the prelude, streaming `input` and `output`.
    pub fn run(
        &self,
        step: Step,
        body: &str,
        input: impl Read + Send,
        output: impl Write + Send,
        limits: ScriptLimits,
        cancelled: &AtomicBool,
    ) -> Result<u64> {
        // Teleport's steps move checkouts and archives, so they share the
        // host's bulk connection and never delay the small reads.
        self.stream_on(SshLane::Bulk, body, input, output, limits, cancelled)
            .map_err(script(step))
    }

    /// [`Host::run`] without Teleport's step context, for queries and other
    /// small scripts. Whatever the script printed stays in `output` when it
    /// fails, for callers whose programs explain a failure on stdout.
    pub fn stream(
        &self,
        body: &str,
        input: impl Read + Send,
        output: impl Write + Send,
        limits: ScriptLimits,
        cancelled: &AtomicBool,
    ) -> sheprd_client::Result<u64> {
        self.stream_on(SshLane::Quick, body, input, output, limits, cancelled)
    }

    fn stream_on(
        &self,
        lane: SshLane,
        body: &str,
        input: impl Read + Send,
        output: impl Write + Send,
        limits: ScriptLimits,
        cancelled: &AtomicBool,
    ) -> sheprd_client::Result<u64> {
        let script_text = format!("{}{body}", self.prelude());
        let host = match &self.route {
            Route::Local => ScriptHost::Local,
            Route::Ssh(target) => ScriptHost::Ssh(target, lane),
            Route::Wsl(distro) => ScriptHost::Wsl(distro),
            #[cfg(feature = "cloud")]
            Route::Cloud {
                provider,
                account,
                id,
            } => {
                if !cfg!(any(target_os = "linux", target_os = "macos")) {
                    return Err(sheprd_client::Error::ScriptUnsupported);
                }
                return crate::cloud::with_command(
                    *provider,
                    account,
                    id,
                    cancelled,
                    |mut command| {
                        command.arg(format!("/bin/sh -c {}", shell_quote(&script_text)));
                        sheprd_client::run_script_command(command, input, output, limits, cancelled)
                    },
                )
                .map_err(|error| {
                    if cancelled.load(std::sync::atomic::Ordering::Acquire) {
                        sheprd_client::Error::ScriptCancelled
                    } else {
                        sheprd_client::Error::ScriptConnect(error)
                    }
                })?;
            }
        };
        run_script(host, &script_text, input, output, limits, cancelled)
    }

    /// Run `body` with no input and collect its bounded stdout. Untied to
    /// Teleport's steps, for other features that script a host and keep
    /// their own typed errors.
    pub fn capture(&self, body: &str, cancelled: &AtomicBool) -> sheprd_client::Result<Vec<u8>> {
        self.capture_within(body, QUERY_IDLE, cancelled)
    }

    /// [`Host::capture`] for callers on a deadline shorter than the default
    /// query idle timeout.
    pub fn capture_within(
        &self,
        body: &str,
        idle: Duration,
        cancelled: &AtomicBool,
    ) -> sheprd_client::Result<Vec<u8>> {
        let mut output = Vec::new();
        self.stream(
            body,
            &[][..],
            &mut output,
            ScriptLimits {
                output: QUERY_OUTPUT,
                idle: idle.min(QUERY_IDLE),
            },
            cancelled,
        )?;
        Ok(output)
    }

    /// Run `program` with `args` and collect its bounded stdout. On this
    /// machine the program starts directly, found where a script's prelude
    /// would find it, so no shell is needed and every client platform runs
    /// it; elsewhere it is one script line. A failing exit is
    /// [`sheprd_client::Error::ScriptExit`] either way.
    pub fn exec(
        &self,
        program: &str,
        args: &[&str],
        idle: Duration,
        cancelled: &AtomicBool,
    ) -> sheprd_client::Result<Vec<u8>> {
        if !self.is_local() {
            let line = std::iter::once(program)
                .chain(args.iter().copied())
                .map(shell_quote)
                .collect::<Vec<_>>()
                .join(" ");
            return self.capture_within(&format!("{line}\n"), idle, cancelled);
        }
        let mut command = self.local_command(program);
        command.args(args);
        Self::capture_command(command, idle, cancelled)
    }

    /// Whether this host is this machine, where [`Host::local_command`]
    /// starts programs without a shell.
    pub fn is_local(&self) -> bool {
        matches!(self.route, Route::Local)
    }

    /// `program` on this machine, found as [`Host::exec`] finds it, with the
    /// user's login environment. Only meaningful when [`Host::is_local`].
    pub fn local_command(&self, program: &str) -> std::process::Command {
        let dirs = self.local_search_path();
        let program = local::find(program, &dirs).unwrap_or_else(|| PathBuf::from(program));
        let mut command = sheprd_client::new_command(program);
        sheprd_client::login_env::apply(&mut command);
        #[cfg(any(test, feature = "test-support"))]
        command.envs(self.env.iter().map(|(key, value)| (key, value)));
        // What the program was found on, as the prelude exports it, so a
        // wrapper's interpreter or helper resolves there too.
        if let Ok(path) = std::env::join_paths(&dirs) {
            command.env("PATH", path);
        }
        command
    }

    /// Run a [`Host::local_command`] under the query bounds and collect its
    /// stdout.
    pub fn capture_command(
        command: std::process::Command,
        idle: Duration,
        cancelled: &AtomicBool,
    ) -> sheprd_client::Result<Vec<u8>> {
        let mut output = Vec::new();
        sheprd_client::run_script_command(
            command,
            &[][..],
            &mut output,
            ScriptLimits {
                output: QUERY_OUTPUT,
                idle: idle.min(QUERY_IDLE),
            },
            cancelled,
        )?;
        Ok(output)
    }

    /// Whether this machine has `program`, as [`Host::exec`] finds it.
    pub fn has_local(&self, program: &str) -> bool {
        self.local_program(program).is_some()
    }

    /// [`Host::exec`] of this host's `herdr` CLI, bound to its session as
    /// `herdr_cli` is.
    pub fn herdr_exec(
        &self,
        args: &[&str],
        idle: Duration,
        cancelled: &AtomicBool,
    ) -> sheprd_client::Result<Vec<u8>> {
        if !self.is_local() {
            return self.capture_within(&Self::herdr_line(args), idle, cancelled);
        }
        if self.development {
            return Err(sheprd_client::Error::ScriptSpawn(std::io::Error::other(
                "a development (--dev) session cannot be scripted",
            )));
        }
        let args: Vec<&str> = self
            .session
            .iter()
            .flat_map(|session| ["--session", session.as_str()])
            .chain(args.iter().copied())
            .collect();
        self.exec("herdr", &args, idle, cancelled)
    }

    /// Where `program` is on this machine, looked up as [`Host::exec`] does.
    fn local_program(&self, program: &str) -> Option<PathBuf> {
        local::find(program, &self.local_search_path())
    }

    /// Where this machine's programs are looked up, and the `PATH` they get.
    fn local_search_path(&self) -> Vec<PathBuf> {
        // Tests point a "remote" host's home and `PATH` at their fixtures.
        #[cfg(any(test, feature = "test-support"))]
        let var = |name: &str| {
            self.env
                .iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.clone())
        };
        #[cfg(not(any(test, feature = "test-support")))]
        let var = |_: &str| None::<String>;
        let home = var("HOME")
            .map(PathBuf::from)
            .or_else(sheprd_client::local_path::home_dir);
        let path = var("PATH")
            .map(std::ffi::OsString::from)
            .or_else(sheprd_client::login_env::path);
        local::search_path(home.as_deref(), path.as_deref())
    }

    /// The script line running `herdr_cli` with `args`, each quoted.
    pub fn herdr_line(args: &[&str]) -> String {
        let command = args
            .iter()
            .map(|arg| shell_quote(arg))
            .collect::<Vec<_>>()
            .join(" ");
        format!("herdr_cli {command}\n")
    }

    /// Run `body` and collect its bounded stdout.
    pub fn query(
        &self,
        step: Step,
        body: &str,
        input: &[u8],
        cancelled: &AtomicBool,
    ) -> Result<Vec<u8>> {
        let mut output = Vec::new();
        self.run(
            step,
            body,
            input,
            &mut output,
            ScriptLimits {
                output: QUERY_OUTPUT,
                idle: QUERY_IDLE,
            },
            cancelled,
        )?;
        Ok(output)
    }

    /// Run `herdr_cli` with `args` and decode the result of its envelope.
    pub fn herdr<T: DeserializeOwned>(
        &self,
        step: Step,
        args: &[&str],
        cancelled: &AtomicBool,
    ) -> Result<T> {
        let output = self.query(step, &Self::herdr_line(args), &[], cancelled)?;
        serde_json::from_slice::<super::snapshot::Envelope<T>>(&output)
            .map(|envelope| envelope.result)
            .map_err(decode(step))
    }

    /// Run `herdr_cli` with `args`, ignoring its output.
    pub fn herdr_ok(&self, step: Step, args: &[&str], cancelled: &AtomicBool) -> Result<()> {
        let line = Self::herdr_line(args);
        let line = line.trim_end();
        self.query(step, &format!("{line} >/dev/null\n"), &[], cancelled)
            .map(drop)
    }

    /// Which of `programs` the user's login shell can find on this host.
    pub fn installed(
        &self,
        step: Step,
        programs: &[&str],
        cancelled: &AtomicBool,
    ) -> Result<Vec<String>> {
        self.installed_programs(programs, cancelled)
            .map_err(script(step))
    }

    /// [`Host::installed`] without Teleport's step context.
    pub fn installed_programs(
        &self,
        programs: &[&str],
        cancelled: &AtomicBool,
    ) -> sheprd_client::Result<Vec<String>> {
        if programs.is_empty() {
            return Ok(Vec::new());
        }
        if self.is_local() {
            return Ok(programs
                .iter()
                .filter(|program| self.has_local(program))
                .map(|program| (*program).to_owned())
                .collect());
        }
        let body = installed_programs_script(programs);
        let output = self.capture(&body, cancelled)?;
        Ok(String::from_utf8_lossy(&output)
            .lines()
            .filter(|line| programs.contains(line))
            .map(str::to_owned)
            .collect())
    }
}

/// A script printing each of `programs` the user's login shell can find on
/// its host, one per line, for [`Host::installed_programs`] or a caller that
/// reads it as part of a larger script. It changes `PATH`, so a caller that
/// goes on afterwards runs it in a subshell.
pub fn installed_programs_script(programs: &[&str]) -> String {
    let names = programs
        .iter()
        .map(|p| shell_quote(p))
        .collect::<Vec<_>>()
        .join(" ");
    // The login shell sees version managers configured in the user's
    // profile; the prelude PATH covers the common install locations.
    format!(
        r#"login=$("${{SHELL:-/bin/sh}}" -lc 'printf %s "$PATH"' </dev/null 2>/dev/null || :)
PATH="$PATH:$login"
for program in {names}; do
    if command -v "$program" >/dev/null 2>&1; then printf '%s\n' "$program"; fi
done
"#
    )
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[cfg(feature = "cloud")]
    mod cloud;

    #[test]
    fn hosts_follow_their_endpoint_target() {
        assert_eq!(
            Host::new(&ConnectTarget::Local).unwrap(),
            Host {
                route: Route::Local,
                session: None,
                development: false,
                env: Vec::new(),
            }
        );
        let ssh = Host::new(&ConnectTarget::Ssh {
            target: "me@box".into(),
            session: "work".into(),
        })
        .unwrap();
        assert_eq!(ssh.route, Route::Ssh("me@box".into()));
        assert!(ssh.prelude().contains("herdr --session 'work' \"$@\""));
        assert!(matches!(
            Host::new(&ConnectTarget::Socket("/tmp/s".into())),
            Err(Error::UnsupportedHost)
        ));
        // A `--dev` window's session lives in the development installation,
        // which `herdr --session` cannot name: its host still runs scripts,
        // but Teleport, fan-out, and dispatch are not offered there.
        let development = ConnectTarget::Session {
            name: "default".into(),
            development: true,
        };
        let host = Host::new(&development).unwrap();
        assert!(!host.scripts_herdr());
        assert!(!host.prelude().contains("herdr --session"));
        assert!(matches!(
            crate::teleport::host_for(&development),
            Err(Error::DevelopmentSession)
        ));
        let release = ConnectTarget::Session {
            name: "default".into(),
            development: false,
        };
        assert!(Host::new(&release).unwrap().scripts_herdr());
        assert!(crate::teleport::host_for(&release).is_ok());
    }

    /// Git and `bd` scripts still run on a development host (the orchestrator
    /// reads its repository that way); only `herdr_cli` refuses, so nothing
    /// reaches the release session of the same name.
    #[test]
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn development_hosts_run_scripts_but_not_herdr() {
        let host = Host::new(&ConnectTarget::Session {
            name: "default".into(),
            development: true,
        })
        .unwrap();
        let cancelled = AtomicBool::new(false);
        assert_eq!(host.capture("printf ok", &cancelled).unwrap(), b"ok");
        let refused = host
            .capture(&Host::herdr_line(&["workspace", "list"]), &cancelled)
            .unwrap_err();
        assert!(
            matches!(
                &refused,
                sheprd_client::Error::ScriptExit { status, stderr }
                    if status.code() == Some(97) && stderr.contains("--dev")
            ),
            "{refused:?}"
        );
    }

    #[test]
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn local_scripts_run_under_the_prelude() {
        let host = Host::new(&ConnectTarget::Local).unwrap();
        let cancelled = AtomicBool::new(false);
        let output = host.query(
            Step::Review,
            "printf '%s' \"$1-${PATH%%:*}\" || :\n",
            &[],
            &cancelled,
        );
        // `set -u` makes the unset positional an error: the prelude is active.
        assert!(matches!(
            output,
            Err(Error::Script {
                step: Step::Review,
                ..
            })
        ));
        let output = host
            .query(Step::Review, "cat\n", b"through", &cancelled)
            .unwrap();
        assert_eq!(output, b"through");
        assert_eq!(
            host.installed(
                Step::Review,
                &["sh", "definitely-not-installed-x"],
                &cancelled
            )
            .unwrap(),
            ["sh"]
        );
    }

    /// A local program gets the directories it was looked up in as its
    /// `PATH`, tool directories first, as the script prelude exports them.
    #[test]
    fn local_programs_get_the_path_they_were_found_on() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let bin = dir.path().join("bin");
        let mut host = Host::new(&ConnectTarget::Local).unwrap();
        host.env = vec![
            ("HOME".into(), home.to_string_lossy().into_owned()),
            (
                "PATH".into(),
                std::env::join_paths([&bin])
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
            ),
        ];
        let command = host.local_command("git");
        let path = command
            .get_envs()
            .find(|(key, _)| *key == "PATH")
            .and_then(|(_, value)| value)
            .unwrap();
        let dirs: Vec<_> = std::env::split_paths(path).collect();
        assert_eq!(dirs.first(), Some(&home.join(".local/bin")));
        assert_eq!(dirs.last(), Some(&bin));
    }
}
