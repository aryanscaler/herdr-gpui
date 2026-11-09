//! Ports of herdr-core terminals, placed under the workspace whose tab
//! shows each terminal.

use super::*;
use crate::browser::Scope;

#[test]
fn a_listener_in_a_core_terminal_names_its_terminal_unless_a_workspace_claims_it() {
    let scan = parse(
        "L 10 *:5173 node\n\
         L 11 127.0.0.1:3000 rails\n\
         L 12 *:8080 python3\n\
         L 13 *:9000 rogue\n\
         C 10 t7 /run/hc/default.sock\n\
         E 11 w7V\n\
         C 11 t8\n\
         C 12 box/t2\n\
         C 13 t=9\n",
    )
    .unwrap();
    // A Herdr workspace wins: a core tab in it names the workspace itself.
    assert_eq!(of(&scan, "w7V"), [port(3000, Bind::Loopback, "rails")]);
    let mut terminals: Vec<(&str, &str, Vec<u16>)> = scan
        .terminals
        .iter()
        .map(|(owner, ports)| {
            (
                owner.terminal.as_str(),
                owner.socket.as_str(),
                ports.iter().map(|port| port.number).collect(),
            )
        })
        .collect();
    terminals.sort();
    assert_eq!(
        terminals,
        [
            ("box/t2", "", vec![8080]),
            ("t7", "/run/hc/default.sock", vec![5173]),
        ]
    );
}

/// The local daemon the fixtures' core terminals belong to.
const SOCKET: &str = "/run/h/herdr-client.sock";

fn local(socket: &str) -> Daemon {
    Daemon::of(&ConnectTarget::Socket(socket.into())).unwrap()
}

fn scope() -> Scope {
    Scope::local(std::path::Path::new(SOCKET))
}

/// Terminal `terminal` of this computer's core in `workspace` of the
/// fixtures' daemon.
fn at(terminal: &str, workspace: &str) -> (Host, String, (Scope, String)) {
    (
        Host::Local,
        terminal.to_owned(),
        (scope(), workspace.to_owned()),
    )
}

#[test]
fn core_ports_follow_the_workspace_each_terminal_is_shown_in() {
    let mut ports = ListeningPorts::default();
    ports.seed(
        Host::Local,
        parse(
            "L 10 *:5173 node\nL 10 [::1]:5173 node\nL 11 *:3000 rails\nL 12 *:4000 x\n\
             C 10 t1\nC 11 t2\nC 12 t3\n",
        )
        .unwrap(),
    );
    assert!(ports.core_ports(&scope(), "herdr-2").is_none());
    let places = vec![at("t2", "herdr-2"), at("t1", "herdr-2"), at("t3", "other")];
    assert!(ports.place_terminals(places.clone()));
    let listed = ports.core_ports(&scope(), "herdr-2").unwrap();
    assert_eq!(
        listed.ports,
        [
            port(3000, Bind::Any, "rails"),
            port(5173, Bind::Any, "node")
        ]
    );
    assert_eq!(*listed.origin, Origin::Local);
    assert_eq!(ports.core_ports(&scope(), "other").unwrap().ports.len(), 1);
    // The same places and scan change nothing; a moved tab does.
    assert!(!ports.place_terminals(places));
    assert!(ports.place_terminals(vec![at("t1", "other")]));
    assert!(ports.core_ports(&scope(), "herdr-2").is_none());
    assert_eq!(ports.core_ports(&scope(), "other").unwrap().ports.len(), 1);
}

#[test]
fn shared_lookups_find_core_ports_where_herdr_names_none() {
    let mut ports = ListeningPorts::default();
    ports.seed(
        Host::Local,
        parse("L 10 *:5173 node\nL 11 *:3000 rails\nC 10 t1\nE 11 w1\n").unwrap(),
    );
    ports.place_terminals(vec![at("t1", "herdr-2")]);
    let local = local(SOCKET);
    // The workspace's chips and the Ports section agree on the listener.
    assert_eq!(
        ports.get(&local, "herdr-2").unwrap().ports,
        [port(5173, Bind::Any, "node")]
    );
    assert_eq!(ports.workspace_of(&local, 10), Some("herdr-2"));
    // A saved host's daemon never takes this computer's terminals.
    let remote = remote("default");
    assert!(ports.get(&remote, "herdr-2").is_none());
    assert_eq!(ports.workspace_of(&remote, 10), None);
    // A pid no terminal is placed for has no workspace.
    assert_eq!(ports.workspace_of(&local, 99), None);
}

#[test]
fn core_ports_stay_with_the_local_daemon_whose_tab_holds_them() {
    let mut ports = ListeningPorts::default();
    ports.seed(Host::Local, parse("L 10 *:5173 node\nC 10 t1\n").unwrap());
    ports.place_terminals(vec![at("t1", "w1")]);
    // Another daemon on this computer with a workspace of the same ID.
    let other = local("/run/other/herdr-client.sock");
    assert!(ports.get(&other, "w1").is_none());
    assert_eq!(ports.workspace_of(&other, 10), None);
    let own = local(SOCKET);
    assert!(ports.get(&own, "w1").is_some());
    assert_eq!(ports.workspace_of(&own, 10), Some("w1"));
}

#[test]
fn a_saved_hosts_core_ports_come_from_that_hosts_scan() {
    let mut ports = ListeningPorts::default();
    let host = Host::Ssh("box".into());
    ports.seed(Host::Local, parse("L 10 *:3000 rails\nC 10 t1\n").unwrap());
    ports.seed(host.clone(), parse("L 20 *:5173 node\nC 20 t1\n").unwrap());
    let kept = (Scope::endpoint("box"), "kept-1".to_owned());
    // The same address on two cores: each workspace takes its own host's.
    assert!(ports.place_terminals(vec![
        at("t1", "w1"),
        (host.clone(), "t1".to_owned(), kept.clone()),
    ]));
    let listed = ports.core_ports(&kept.0, &kept.1).unwrap();
    assert_eq!(listed.ports, [port(5173, Bind::Any, "node")]);
    assert_eq!(*listed.origin, Origin::new(&host));
    assert_eq!(
        ports.core_ports(&scope(), "w1").unwrap().ports,
        [port(3000, Bind::Any, "rails")]
    );
    // A terminal placed on a host never scanned lists nothing.
    ports.place_terminals(vec![(
        Host::Ssh("gone".into()),
        "t1".to_owned(),
        kept.clone(),
    )]);
    assert!(ports.core_ports(&kept.0, &kept.1).is_none());
}
