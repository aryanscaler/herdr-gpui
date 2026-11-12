//! Chat layout follows its host; equal terminal IDs never share metadata.

use super::*;
use crate::browser::Scope;

#[test]
fn a_remote_tab_splits_and_shows_chats_without_borrowing_local_metadata() {
    let mut terminals = terminals();
    // The remote core also has a t1, in its own tab.
    terminals.tabs.insert(
        TabId::test(3),
        TabPanes::new(
            Host::Remote(Scope::endpoint("ssh:box")),
            Panes::single(id("t1")),
            false,
        ),
    );
    let shown: Vec<&str> = terminals
        .chat_terminals()
        .map(|info| info.id.as_str())
        .collect();
    assert_eq!(shown.iter().filter(|id| **id == "t1").count(), 1);
    assert_eq!(terminals.tab_of(&id("t1")), Some(TabId::test(1)));
    assert!(terminals.drives(TabId::test(3)));
    assert_eq!(terminals.chat_split_at(TabId::test(3)), Some(id("t1")));
    assert_eq!(terminals.visible_chat_panes(TabId::test(3)), [&id("t1")]);
    assert_eq!(terminals.info(TabId::test(3), &id("t1")), None);
    assert_eq!(
        terminals.chat_tab_on(&Host::Remote(Scope::endpoint("ssh:box")), &id("t1")),
        Some(TabId::test(3))
    );
    assert_eq!(
        terminals.chat_tab_on(&Host::Remote(Scope::endpoint("ssh:other")), &id("t1")),
        None
    );
}

#[test]
fn remote_mirrors_never_split_or_draw_an_interactive_chat() {
    let mut terminals = terminals();
    terminals.tabs.insert(
        TabId::test(3),
        TabPanes::new(
            Host::Remote(Scope::endpoint("wsl:Ubuntu")),
            Panes::single(id("t1")),
            true,
        ),
    );
    assert!(!terminals.drives(TabId::test(3)));
    assert_eq!(terminals.chat_split_at(TabId::test(3)), None);
    assert!(terminals.visible_chat_panes(TabId::test(3)).is_empty());
}
