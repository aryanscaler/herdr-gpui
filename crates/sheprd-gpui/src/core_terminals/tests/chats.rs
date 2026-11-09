//! What the chat side reads of herdr-core tabs: the terminals a chat may be
//! drawn over, the pane a new chat splits, and moving the pane in use.

use crate::{
    browser::TabId,
    core_terminals::Host,
    core_terminals::{CoreTerminals, TabPanes, panes::Panes},
};
use sheprd_backend::{RunState, TerminalId, TerminalInfo};
use sheprd_client::protocol::AgentStatus;

mod remote;
mod visibility;

fn id(name: &str) -> TerminalId {
    TerminalId::new(name).unwrap()
}

fn info(name: &str) -> TerminalInfo {
    TerminalInfo {
        agent: AgentStatus::Unknown,
        cwd: None,
        foreground: None,
        id: id(name),
        labels: Default::default(),
        name: None,
        reporter: None,
        state: RunState::Running,
        title: None,
    }
}

/// Tab 1 drives t1 and t2; tab 2 mirrors t3.
fn terminals() -> CoreTerminals {
    let mut terminals = CoreTerminals::default();
    let mut panes = Panes::single(id("t1"));
    panes
        .split(&id("t1"), crate::core_terminals::panes::Axis::Row, id("t2"))
        .unwrap();
    terminals
        .tabs
        .insert(TabId::test(1), TabPanes::new(Host::Local, panes, false));
    terminals.tabs.insert(
        TabId::test(2),
        TabPanes::new(Host::Local, Panes::single(id("t3")), true),
    );
    for name in ["t1", "t2", "t3", "t4"] {
        terminals.infos.insert(info(name));
    }
    terminals
}

#[test]
fn chats_are_drawn_only_over_panes_of_tabs_that_drive_them() {
    let terminals = terminals();
    let mut shown: Vec<&str> = terminals
        .chat_terminals()
        .map(|info| info.id.as_str())
        .collect();
    shown.sort_unstable();
    assert_eq!(shown, ["t1", "t2"]);
    assert_eq!(terminals.tab_of(&id("t2")), Some(TabId::test(1)));
    assert_eq!(terminals.tab_of(&id("t3")), None);
    assert_eq!(terminals.tab_of(&id("t4")), None);
}

#[test]
fn a_new_chat_splits_the_pane_in_use_but_never_a_mirror() {
    let mut terminals = terminals();
    assert_eq!(terminals.chat_split_at(TabId::test(1)), Some(id("t1")));
    assert_eq!(terminals.chat_split_at(TabId::test(2)), None);
    assert_eq!(terminals.chat_split_at(TabId::test(9)), None);
    assert!(terminals.set_active(TabId::test(1), id("t2")));
    assert_eq!(terminals.chat_split_at(TabId::test(1)), Some(id("t2")));
}

#[test]
fn the_pane_in_use_moves_only_to_a_pane_of_its_tab() {
    let mut terminals = terminals();
    assert!(!terminals.set_active(TabId::test(1), id("t1")));
    assert!(!terminals.set_active(TabId::test(1), id("t3")));
    assert!(!terminals.set_active(TabId::test(9), id("t1")));
    assert!(terminals.set_active(TabId::test(1), id("t2")));
}

#[test]
fn a_core_that_does_not_answer_reports_no_agent() {
    let mut terminals = terminals();
    let mut working = info("t1");
    working.agent = AgentStatus::Working;
    terminals.infos.insert(working);

    // Its last list stays known for the panes' chrome, but no agent in it
    // is counted until the core answers again.
    assert!(terminals.infos.get(&id("t1")).is_some());
    assert!(terminals.reported(&Host::Local, &id("t1")).is_none());
}
