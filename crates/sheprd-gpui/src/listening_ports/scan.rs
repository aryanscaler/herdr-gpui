//! The step a host's shell runs to list TCP listeners, and the text it prints.
//!
//! Herdr exports `HERDR_WORKSPACE_ID` into every pane it starts, and a server
//! launched from a pane inherits it, so a listener's own environment names
//! its workspace exactly; no process tree or working directory is guessed.
//! herdr-core exports `HC_TERMINAL_ID` the same way, so a listener started in
//! a herdr-core terminal names its terminal, which the app maps to the
//! workspace whose tab shows it. Listeners whose process names neither
//! (system services, containers, anything started outside both) belong to no
//! workspace and are dropped.

use super::Error;
use super::tunnel::Key;
use crate::{Result, browser::WebUrl, usage::Host};
use sheprd_client::herdr::AgentKind;
use std::{collections::HashMap, net::IpAddr};

/// Listener lines read from one answer; the rest are ignored.
const MAX_LISTENERS: usize = 1024;
/// Command-line words read per listener, enough to find an agent's script.
const MAX_ARGS: usize = 16;
/// Ports kept per workspace, lowest first.
pub(crate) const MAX_PORTS: usize = 32;
/// Process names are display text only.
const MAX_PROCESS: usize = 32;
/// Workspace ids are short daemon tokens such as `w7V`.
const MAX_WORKSPACE_ID: usize = 64;
/// Longer socket paths than any Unix socket can have are not trusted.
const MAX_SOCKET: usize = 1024;

/// Prints `L <pid> <address:port> <process>` per listening socket, then
/// `E <pid> <workspace> <api socket>` for each listener started inside a
/// Herdr pane, the socket naming which daemon's pane that was,
/// `C <pid> <terminal> <core socket>` for one started in a herdr-core
/// terminal outside any Herdr workspace, each followed by
/// `A <pid> <command line>`, which tells a coding agent's own listener from
/// a server it starts; or `N` when the host has neither `ss` nor `lsof`.
/// Linux prefers `ss`, which is quicker; macOS has only `lsof`. Both list
/// only this user's processes with their owners, so another user's servers
/// never show. The step runs in a long-lived shell, so it must not `exit`,
/// and it adds each `PATH` entry only when missing: appending on every scan
/// grows `PATH` without bound and slows every command lookup (issue #754).
// i18n: data
pub(crate) const COMMAND: &str = r#"for herdr_dir in /usr/sbin /sbin; do
    case ":$PATH:" in
        *":$herdr_dir:"*) ;;
        *) PATH="$PATH:$herdr_dir" ;;
    esac
done
if command -v ss >/dev/null 2>&1; then
    herdr_ports=$(ss -ltnp 2>/dev/null | awk '{
        rest = $0
        while (match(rest, /\("[^"]*",pid=[0-9]+/)) {
            entry = substr(rest, RSTART + 2, RLENGTH - 2)
            rest = substr(rest, RSTART + RLENGTH)
            split(entry, part, "\",pid=")
            print "L", part[2], $4, part[1]
        }
    }')
elif command -v lsof >/dev/null 2>&1; then
    herdr_ports=$(lsof -nP -iTCP -sTCP:LISTEN -Fpcn 2>/dev/null | awk '
        /^p/ { pid = substr($0, 2) }
        /^c/ { name = substr($0, 2) }
        /^n/ { print "L", pid, substr($0, 2), name }')
else
    herdr_ports=N
fi
printf '%s\n' "$herdr_ports"
for herdr_pid in $(printf '%s\n' "$herdr_ports" | awk '$1 == "L" { print $2 }' | sort -un | head -n 128); do
    if [ -r "/proc/$herdr_pid/environ" ]; then
        herdr_env=$(tr '\0' '\n' < "/proc/$herdr_pid/environ")
    else
        herdr_env=$(ps -E -ww -o command= -p "$herdr_pid" 2>/dev/null | tr ' ' '\n')
    fi
    herdr_workspace=$(printf '%s\n' "$herdr_env" | sed -n 's/^HERDR_WORKSPACE_ID=//p' | head -n 1)
    herdr_socket=$(printf '%s\n' "$herdr_env" | sed -n 's/^HERDR_SOCKET_PATH=//p' | head -n 1)
    herdr_terminal=$(printf '%s\n' "$herdr_env" | sed -n 's/^HC_TERMINAL_ID=//p' | head -n 1)
    herdr_core=$(printf '%s\n' "$herdr_env" | sed -n 's/^HC_TERMINAL_SOCKET=//p' | head -n 1)
    if [ -n "$herdr_workspace" ]; then
        printf 'E %s %s %s\n' "$herdr_pid" "$herdr_workspace" "$herdr_socket"
    elif [ -n "$herdr_terminal" ]; then
        printf 'C %s %s %s\n' "$herdr_pid" "$herdr_terminal" "$herdr_core"
    else
        continue
    fi
    if [ -r "/proc/$herdr_pid/cmdline" ]; then
        herdr_args=$(tr '\0\n' '  ' < "/proc/$herdr_pid/cmdline")
    else
        herdr_args=$(ps -ww -o args= -p "$herdr_pid" 2>/dev/null | tr '\n' ' ')
    fi
    printf 'A %s %s\n' "$herdr_pid" "$(printf '%s' "$herdr_args" | cut -c 1-1024)"
done
true"#;

/// The address a socket listens on, which decides how it can be reached.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Bind {
    /// One specific, non-loopback address.
    Address(IpAddr),
    /// The host's own loopback only.
    Loopback,
    /// Every address, `*`, `0.0.0.0`, or `::`.
    Any,
}

impl Bind {
    pub(crate) fn parse(host: &str) -> Option<Self> {
        let host = host.trim_start_matches('[').trim_end_matches(']');
        // `ss` names the interface of a link-scoped address: `fe80::1%eth0`.
        let host = host.split('%').next().unwrap_or(host);
        if host == "*" {
            return Some(Self::Any);
        }
        let address: IpAddr = host.parse().ok()?;
        Some(if address.is_unspecified() {
            Self::Any
        } else if address.is_loopback() {
            Self::Loopback
        } else {
            Self::Address(address)
        })
    }

    /// Which of two sockets on one port to keep: the one reachable from more
    /// places.
    pub(crate) fn reach(self) -> u8 {
        match self {
            Self::Address(_) => 0,
            Self::Loopback => 1,
            Self::Any => 2,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Port {
    pub number: u16,
    pub bind: Bind,
    /// The listening process's short name, bounded display text.
    pub process: String,
}

impl Port {
    /// Where a browser on this machine reaches a port of this machine.
    fn url(&self) -> Option<WebUrl> {
        let name = match self.bind {
            Bind::Address(IpAddr::V6(address)) => format!("[{address}]"),
            Bind::Address(IpAddr::V4(address)) => address.to_string(),
            Bind::Any | Bind::Loopback => "localhost".into(),
        };
        WebUrl::try_from(format!("http://{name}:{}/", self.number)).ok()
    }

    /// What clicking the port does: open its page, or first tunnel to a
    /// remote one. A remote server always opens through SSH on this
    /// machine's loopback: the address SSH dials, perhaps a VPN's, says
    /// nothing about whether a browser here may reach the server directly.
    pub(crate) fn link(&self, origin: &Origin) -> Option<Link> {
        match origin {
            Origin::Local => self.url().map(Link::Page),
            Origin::Remote { target } => Some(Link::Tunnel(Key {
                target: target.clone(),
                address: match self.bind {
                    Bind::Address(address) => Some(address),
                    Bind::Any | Bind::Loopback => None,
                },
                port: self.number,
            })),
        }
    }

    /// `*:3000`, `127.0.0.1:3000`, as a tooltip names the socket.
    pub(crate) fn address(&self) -> String {
        match self.bind {
            Bind::Any => format!("*:{}", self.number),
            Bind::Loopback => format!("localhost:{}", self.number),
            Bind::Address(IpAddr::V6(address)) => format!("[{address}]:{}", self.number),
            Bind::Address(IpAddr::V4(address)) => format!("{address}:{}", self.number),
        }
    }
}

/// Where clicking a port leads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Link {
    Page(WebUrl),
    Tunnel(Key),
}

impl Link {
    /// Where it leads, as a menu names it: `localhost:5173`, or `:5173`
    /// for a remote port reached through a tunnel opened on demand.
    pub(crate) fn label(&self) -> String {
        match self {
            Self::Page(url) => {
                let url = url.as_str();
                let url = url.split_once("://").map_or(url, |(_, rest)| rest);
                url.trim_end_matches('/').to_owned()
            }
            Self::Tunnel(key) => format!(":{}", key.port),
        }
    }
}

/// Where a browser on this machine finds the scanned host.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) enum Origin {
    #[default]
    Local,
    /// The SSH target, which a tunnel to the host dials.
    Remote { target: String },
}

impl Origin {
    pub(crate) fn new(host: &Host) -> Self {
        match host {
            // WSL forwards a distribution's localhost ports to this machine.
            Host::Local | Host::Wsl(_) => Self::Local,
            // Never scanned: `open` refuses it.
            #[cfg(feature = "azure")]
            Host::Azure { .. } => Self::Local,
            Host::Ssh(target) => Self::Remote {
                target: target.clone(),
            },
        }
    }
}

/// The pane a listener was started in: its workspace, and the JSON API
/// socket of the daemon that owns it, empty when the process did not say.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) struct Owner {
    pub workspace: String,
    pub socket: String,
}

/// Each workspace's ports, by owner, lowest port first.
pub(crate) type Ports = HashMap<Owner, Vec<Port>>;

/// The herdr-core terminal a listener was started in, by its address such
/// as `t7`, and its core's socket, empty when the process did not say.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) struct Terminal {
    pub terminal: String,
    pub socket: String,
}

/// What one scan of a host found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Scan {
    pub ports: Ports,
    /// The pane each listening process was started in. Several processes
    /// can share a number, so only the process names a listener's owner.
    pub pids: HashMap<u32, Owner>,
    /// Each herdr-core terminal's ports, lowest first, for listeners no
    /// Herdr workspace claims.
    pub terminals: HashMap<Terminal, Vec<Port>>,
    /// The herdr-core terminal each such listening process was started in.
    pub terminal_pids: HashMap<u32, Terminal>,
}

/// Parses what [`COMMAND`] printed.
pub(crate) fn parse(text: &str) -> Result<Scan> {
    let mut listeners: Vec<(u32, Port)> = Vec::new();
    let mut owners: HashMap<u32, Owner> = HashMap::new();
    let mut terminals: HashMap<u32, Terminal> = HashMap::new();
    let mut commands: HashMap<u32, Vec<String>> = HashMap::new();
    for line in text.lines() {
        let mut words = line.splitn(4, ' ');
        match words.next() {
            Some("N") => return Err(Error::Tool.into()),
            Some("L") if listeners.len() < MAX_LISTENERS => {
                let (Some(pid), Some(address)) = (words.next(), words.next()) else {
                    continue;
                };
                let (Ok(pid), Some(port)) = (pid.parse(), listener(address, words.next())) else {
                    continue;
                };
                listeners.push((pid, port));
            }
            Some("E") => {
                let (Some(pid), Some(workspace)) = (words.next(), words.next()) else {
                    continue;
                };
                let workspace = workspace.trim_end();
                let plain = plain(workspace, MAX_WORKSPACE_ID);
                let socket = socket(words.next());
                if let (Ok(pid), true) = (pid.parse(), plain) {
                    owners.insert(
                        pid,
                        Owner {
                            workspace: workspace.to_owned(),
                            socket: socket.to_owned(),
                        },
                    );
                }
            }
            Some("C") => {
                let (Some(pid), Some(terminal)) = (words.next(), words.next()) else {
                    continue;
                };
                let terminal = terminal.trim_end();
                let plain = plain(terminal, sheprd_backend::MAX_TERMINAL_ID);
                let socket = socket(words.next());
                if let (Ok(pid), true) = (pid.parse(), plain) {
                    terminals.insert(
                        pid,
                        Terminal {
                            terminal: terminal.to_owned(),
                            socket: socket.to_owned(),
                        },
                    );
                }
            }
            Some("A") => {
                let mut words = line.splitn(3, ' ').skip(1);
                let (Some(Ok(pid)), Some(args)) = (words.next().map(str::parse), words.next())
                else {
                    continue;
                };
                let args = args.split_whitespace().take(MAX_ARGS).map(str::to_owned);
                commands.insert(pid, args.collect());
            }
            _ => {}
        }
    }
    let mut scan = Scan::default();
    for (pid, port) in listeners {
        let args = commands.get(&pid).map_or(&[][..], Vec::as_slice);
        // A coding agent's own sockets are its transport, not a page to
        // open. The owner is still recorded, so the machine-wide list of
        // listeners can name the workspace that started it.
        let shown = AgentKind::identify(None, &port.process, args).is_none();
        if let Some(owner) = owners.get(&pid) {
            scan.pids.insert(pid, owner.clone());
            if shown {
                add(scan.ports.entry(owner.clone()).or_default(), port);
            }
        } else if let Some(terminal) = terminals.get(&pid) {
            scan.terminal_pids.insert(pid, terminal.clone());
            if shown {
                add(scan.terminals.entry(terminal.clone()).or_default(), port);
            }
        }
    }
    for list in scan.ports.values_mut().chain(scan.terminals.values_mut()) {
        sorted(list);
    }
    Ok(scan)
}

/// A daemon token such as a workspace id or terminal address: printable,
/// bounded, and with no `=` an environment line could smuggle in.
fn plain(token: &str, max: usize) -> bool {
    !token.is_empty()
        && token.len() <= max
        && token.chars().all(|c| c.is_ascii_graphic() && c != '=')
}

/// A socket path a process named. Paths are compared, never opened; an
/// unusable one is unknown.
fn socket(word: Option<&str>) -> &str {
    word.map(str::trim_end)
        .filter(|socket| socket.len() <= MAX_SOCKET && !socket.chars().any(char::is_control))
        .unwrap_or_default()
}

/// Adds `port` to `list`, keeping one entry per number.
pub(crate) fn add(list: &mut Vec<Port>, port: Port) {
    match list.iter_mut().find(|known| known.number == port.number) {
        // IPv4 and IPv6 sockets of one server share its number.
        Some(known) => {
            if port.bind.reach() > known.bind.reach() {
                *known = port;
            }
        }
        None => list.push(port),
    }
}

/// Lowest port first, at most [`MAX_PORTS`].
pub(crate) fn sorted(list: &mut Vec<Port>) {
    list.sort_by_key(|port| port.number);
    list.truncate(MAX_PORTS);
}

/// `127.0.0.1:3000`, `[::1]:3000`, `*:3000`, `0.0.0.0:3000`.
pub(crate) fn listener(address: &str, process: Option<&str>) -> Option<Port> {
    let (host, number) = address.rsplit_once(':')?;
    let number: u16 = number.parse().ok().filter(|number| *number > 0)?;
    Some(Port {
        number,
        bind: Bind::parse(host)?,
        process: process
            .unwrap_or_default()
            .chars()
            .filter(|c| !c.is_control())
            .take(MAX_PROCESS)
            .collect::<String>()
            .trim()
            .to_owned(),
    })
}
