//! Which Herdr 2 tabs a note may type into: any tab that types into its
//! terminals, whichever host's core runs them, but never a mirror.

use crate::{
    browser::TabId,
    core_terminals::{CoreTerminals, panes::Panes},
};
use sheprd_backend::TerminalId;

#[test]
fn a_tab_types_into_its_terminals_and_a_mirror_does_not() {
    let mut terminals = CoreTerminals::default();
    let panes = |name: &str| Panes::single(TerminalId::new(name).unwrap());
    terminals.show_for_test(TabId::test(1), panes("t1"), false, Vec::new());
    terminals.show_for_test(TabId::test(2), panes("t1"), true, Vec::new());
    assert!(terminals.types_into(TabId::test(1)));
    assert!(!terminals.types_into(TabId::test(2)));
    assert!(!terminals.types_into(TabId::test(3)));
}
