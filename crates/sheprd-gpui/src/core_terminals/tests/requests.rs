//! Browser requests read from labels on another host's terminals.

use crate::{
    browser::{Scope, TabId},
    core_terminals::{CoreTerminals, Host, Hub, TabPanes, panes::Panes},
};
use sheprd_backend::{RunState, TerminalId, TerminalInfo};
use sheprd_client::{ConnectTarget, protocol::AgentStatus};

mod identity;

fn scope() -> Scope {
    Scope::endpoint("ssh:box")
}

fn id(name: &str) -> TerminalId {
    TerminalId::new(name).unwrap()
}

fn labelled(name: &str, labels: &[(&str, &str)]) -> TerminalInfo {
    TerminalInfo {
        agent: AgentStatus::Unknown,
        cwd: None,
        foreground: None,
        id: id(name),
        labels: labels
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect(),
        name: None,
        reporter: None,
        state: RunState::Running,
        title: None,
    }
}

fn terminals() -> CoreTerminals {
    let mut terminals = CoreTerminals::default();
    let target = ConnectTarget::Ssh {
        target: "box".into(),
        session: "default".into(),
    };
    terminals.set_remote_hub_for_test(
        scope(),
        target,
        Hub::with_join(|_| Err(crate::core_terminals::Error::NotBundled)),
    );
    let host = Host::Remote(scope());
    terminals.tabs.insert(
        TabId::test(1),
        TabPanes::new(host.clone(), Panes::single(id("t1")), false),
    );
    terminals.tabs.insert(
        TabId::test(2),
        TabPanes::new(host, Panes::single(id("t2")), true),
    );
    terminals
}

const OLD: &[(&str, &str)] = &[
    ("herdr-gpui-1-1", "open"),
    ("herdr-gpui-1-1_0", "https://old.example"),
];

#[test]
fn only_requests_after_the_first_list_on_a_shown_terminal_are_served() {
    let mut terminals = terminals();
    assert!(terminals.label_requests().is_empty(), "nothing listed yet");
    terminals.list_for_test(&scope(), vec![labelled("t1", OLD)]);
    // There before this window looked: never served.
    assert!(terminals.label_requests().is_empty());
    let new = [
        ("herdr-gpui-2-2", "open"),
        ("herdr-gpui-2-2_0", "https://new.example"),
        ("other", "kept"),
    ];
    let mut labels: Vec<(&str, &str)> = OLD.to_vec();
    labels.extend(new);
    terminals.list_for_test(
        &scope(),
        vec![
            labelled("t1", &labels),
            // A mirror only watches, and t9 is in no tab here.
            labelled("t2", &new),
            labelled("t9", &new),
        ],
    );
    let found = terminals.label_requests();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].tab, TabId::test(1));
    assert_eq!(found[0].terminal, id("t1"));
    let mut names: Vec<&str> = found[0]
        .tokens
        .iter()
        .map(|(name, _)| name.as_str())
        .collect();
    names.sort_unstable();
    assert_eq!(names, ["herdr-gpui-2-2", "herdr-gpui-2-2_0"]);
}

const NEW: &[(&str, &str)] = &[
    ("herdr-gpui-2-2", "open"),
    ("herdr-gpui-2-2_0", "https://new.example"),
];

#[test]
fn a_request_is_served_once_while_its_labels_are_being_removed() {
    let mut terminals = terminals();
    terminals.list_for_test(&scope(), vec![labelled("t1", &[])]);
    assert!(terminals.label_requests().is_empty());
    terminals.list_for_test(&scope(), vec![labelled("t1", NEW)]);
    assert_eq!(terminals.label_requests().len(), 1);
    // The window ticks again before the core lists the labels gone.
    assert!(terminals.label_requests().is_empty());
    terminals.list_for_test(&scope(), vec![labelled("t1", NEW)]);
    assert!(terminals.label_requests().is_empty());
}

#[test]
fn a_request_left_on_an_unshown_terminal_is_not_served_once_shown() {
    let mut terminals = terminals();
    terminals.list_for_test(&scope(), vec![labelled("t9", &[])]);
    assert!(terminals.label_requests().is_empty());
    // An agent in a workspace this window has not opened asks for a page.
    terminals.list_for_test(&scope(), vec![labelled("t9", NEW)]);
    assert!(terminals.label_requests().is_empty());
    // Opening that workspace later shows the terminal.
    terminals.tabs.insert(
        TabId::test(9),
        TabPanes::new(Host::Remote(scope()), Panes::single(id("t9")), false),
    );
    assert!(terminals.label_requests().is_empty());
    // A request made while it is shown is served.
    let mut labels = NEW.to_vec();
    labels.extend([
        ("herdr-gpui-3-3", "open"),
        ("herdr-gpui-3-3_0", "https://x.example"),
    ]);
    terminals.list_for_test(&scope(), vec![labelled("t9", &labels)]);
    let found = terminals.label_requests();
    assert_eq!(found.len(), 1);
    let mut names: Vec<&str> = found[0]
        .tokens
        .iter()
        .map(|(name, _)| name.as_str())
        .collect();
    names.sort_unstable();
    assert_eq!(names, ["herdr-gpui-3-3", "herdr-gpui-3-3_0"]);
}
