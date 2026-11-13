use super::*;

/// Another window split the tab after this one last read it. Recovery must
/// keep that split, not write this window's stale copy over it.
#[test]
fn recovery_keeps_a_split_another_window_saved() {
    let old = TerminalId::new("t1").unwrap();
    let other = TerminalId::new("t2").unwrap();
    let new = TerminalId::new("t3").unwrap();
    let added = TerminalId::new("t4").unwrap();
    let mut panes = Panes::single(old.clone());
    panes.split(&old, Axis::Row, other.clone()).unwrap();
    let mut shown = TabPanes::new(Host::Local, panes.clone(), false);
    shown.active = old.clone();
    let read = shown.location();
    shown.saved = Some(read);
    let mut elsewhere = TabPanes::new(Host::Local, panes, false);
    elsewhere.active = old.clone();
    elsewhere
        .panes
        .split(&other, Axis::Column, added.clone())
        .unwrap();
    let stored = elsewhere.location();
    let mapping = BTreeMap::from([(old.clone(), new.clone())]);

    let (saved, changed) = recover(&Host::Local, &stored, Some(&mut shown), &mapping);
    let saved = saved.unwrap();
    assert!(changed);
    let left = shown.refresh(&saved);
    assert!(left.is_empty());
    for terminal in [&new, &other, &added] {
        assert!(shown.panes.contains(terminal), "{terminal:?} missing");
    }
    assert!(!shown.panes.contains(&old));
    assert_eq!(shown.active, new);
    assert_eq!(shown.saved.as_ref(), Some(&saved));
}

#[test]
fn a_tab_this_window_never_showed_is_remapped_in_the_store_only() {
    let old = TerminalId::new("t1").unwrap();
    let new = TerminalId::new("t3").unwrap();
    let stored = TabPanes::new(Host::Local, Panes::single(old.clone()), false).location();
    let mapping = BTreeMap::from([(old, new.clone())]);
    let (saved, changed) = recover(&Host::Local, &stored, None, &mapping);
    assert!(!changed);
    let restored = TabPanes::restore(Host::Local, &saved.unwrap()).unwrap();
    assert!(restored.panes.contains(&new));
}
