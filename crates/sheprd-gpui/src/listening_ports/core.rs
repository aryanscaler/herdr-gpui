//! Ports of herdr-core terminals, listed under the workspace whose tab shows
//! each terminal: this computer's, and a saved host's, read from that host's
//! own scan. A scan names only the terminal a listener was started in; which
//! workspace that is, only the window's tabs know, so the window tells it
//! and the ports are gathered again when either changes. Terminal addresses
//! are only unique within one host, so each host's are placed apart.

use super::{Daemon, Listed, ListeningPorts, Port, scan};
use crate::{browser::Scope, usage::Host};
use std::collections::HashMap;

/// A workspace by the daemon whose tabs hold it, as tabs remember it: two
/// local daemons may each have a workspace of the same ID.
pub(crate) type Place = (Scope, String);

type Scanned = HashMap<scan::Terminal, Vec<Port>>;

/// Each workspace's herdr-core ports, and what they were gathered from.
#[derive(Default)]
pub(super) struct Placed {
    /// Each host's terminals' workspaces, as the window last told them,
    /// sorted by terminal.
    places: HashMap<Host, Vec<(String, Place)>>,
    /// Each host's terminals' ports as the scan they were gathered from
    /// found them.
    scanned: HashMap<Host, Scanned>,
    /// Each workspace's ports, with the host whose scan found them.
    workspaces: HashMap<Place, (Host, Vec<Port>)>,
}

impl ListeningPorts {
    /// Tells which workspace each herdr-core terminal is in, by its host and
    /// its address there such as `t7`, the first given for each when several
    /// are; whether the ports listed changed.
    pub fn place_terminals(&mut self, places: Vec<(Host, String, Place)>) -> bool {
        let mut by_host: HashMap<Host, Vec<(String, Place)>> = HashMap::new();
        for (host, terminal, place) in places {
            by_host.entry(host).or_default().push((terminal, place));
        }
        for places in by_host.values_mut() {
            // Stable, so a terminal keeps the first workspace it was given.
            places.sort_by(|a, b| a.0.cmp(&b.0));
            places.dedup_by(|a, b| a.0 == b.0);
        }
        let scanned: HashMap<Host, Scanned> = by_host
            .keys()
            .filter_map(|host| {
                let monitor = self.monitors.get(host)?;
                Some((host.clone(), monitor.reading.scan.terminals.clone()))
            })
            .collect();
        if by_host == self.core.places && scanned == self.core.scanned {
            return false;
        }
        let mut workspaces = HashMap::new();
        for (host, places) in &by_host {
            let Some(scanned) = scanned.get(host) else {
                continue;
            };
            for (place, ports) in gather(places, scanned) {
                workspaces.insert(place, (host.clone(), ports));
            }
        }
        self.core.scanned = scanned;
        self.core.places = by_host;
        let changed = workspaces != self.core.workspaces;
        self.core.workspaces = workspaces;
        changed
    }

    /// The workspace of `daemon` whose herdr-core terminal started the
    /// listening process `pid` on this computer, as the latest scan found it.
    pub(super) fn core_workspace_of(&self, daemon: &Daemon, pid: u32) -> Option<&str> {
        let owner = self
            .monitors
            .get(&Host::Local)?
            .reading
            .scan
            .terminal_pids
            .get(&pid)?;
        let places = self.core.places.get(&Host::Local)?;
        let at = places
            .binary_search_by(|(terminal, _)| terminal.as_str().cmp(&owner.terminal))
            .ok()?;
        let (scope, workspace) = &places[at].1;
        (Some(scope) == daemon.scope().as_ref()).then_some(workspace.as_str())
    }

    /// The ports the herdr-core terminals of `workspace` of the daemon
    /// `scope` names listen on, lowest first, and where they open; None
    /// while they listen on none.
    pub fn core_ports(&self, scope: &Scope, workspace: &str) -> Option<Listed<'_>> {
        let (host, ports) = self
            .core
            .workspaces
            .get(&(scope.clone(), workspace.to_owned()))?;
        let origin = &self.monitors.get(host)?.reading.origin;
        (!ports.is_empty()).then_some(Listed { ports, origin })
    }
}

/// The ports of each workspace's terminals in `places`, one entry per number.
fn gather(places: &[(String, Place)], scanned: &Scanned) -> HashMap<Place, Vec<Port>> {
    let mut workspaces: HashMap<Place, Vec<Port>> = HashMap::new();
    for (owner, ports) in scanned {
        let Ok(at) =
            places.binary_search_by(|(terminal, _)| terminal.as_str().cmp(&owner.terminal))
        else {
            continue;
        };
        let list = workspaces.entry(places[at].1.clone()).or_default();
        for port in ports {
            scan::add(list, port.clone());
        }
    }
    for list in workspaces.values_mut() {
        scan::sorted(list);
    }
    workspaces
}
