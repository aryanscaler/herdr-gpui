use super::*;
use crate::{browser::Scope, core_terminals::Host};

#[test]
fn a_core_key_is_host_scoped_and_only_local_keys_decode_as_local_terminals() {
    let terminal = TerminalId::new("t1").unwrap();
    let local = key(&terminal);
    assert_eq!(local, "core:t1");
    assert_eq!(terminal_of(&local), Some(terminal.clone()));
    let first = key_on(&Host::Remote(Scope::endpoint("ssh:one")), &terminal);
    let second = key_on(&Host::Remote(Scope::endpoint("ssh:two")), &terminal);
    assert_ne!(first, second);
    assert_ne!(first, local);
    assert!(terminal_of(&first).is_none());
}

#[test]
fn host_and_terminal_separators_cannot_alias_a_key() {
    let a = key_on(
        &Host::Remote(Scope::endpoint("ssh:a:t")),
        &TerminalId::new("1").unwrap(),
    );
    let b = key_on(
        &Host::Remote(Scope::endpoint("ssh:a")),
        &TerminalId::new("t:1").unwrap(),
    );
    assert_ne!(a, b);
}
