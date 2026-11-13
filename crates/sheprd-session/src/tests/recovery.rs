#![allow(clippy::unwrap_used)]
use crate::{
    SavedTabs,
    panes::{Axis, Panes},
};
use sheprd_backend::TerminalId;
use std::collections::BTreeMap;

#[test]
fn rebind_preserves_geometry_and_refuses_duplicates() {
    let old = TerminalId::new("t1").unwrap();
    let other = TerminalId::new("t2").unwrap();
    let new = TerminalId::new("t3").unwrap();
    let mut panes = Panes::single(old.clone());
    panes.split(&old, Axis::Column, other.clone()).unwrap();
    let before = panes.rects();
    assert!(!panes.replace(&old, other));
    assert!(panes.replace(&old, new.clone()));
    let after = panes.rects();
    assert_eq!(before[0].1, after[0].1);
    assert_eq!(before[1], after[1]);
    assert_eq!(after[0].0, new);
    assert!(!panes.replace(&old, new));
}

#[test]
fn saved_layout_remapping_is_host_scoped_and_keeps_names_and_zoom() {
    let json = serde_json::json!({"tabs": [
        {"id":1,"scope":"local:a","workspace_id":"w1","title":"one","location":{"kind":"core_terminal","panes":"t1","names":{"t1":"mine"},"zoomed":"t1"}},
        {"id":2,"scope":"remote:b","workspace_id":"w1","title":"two","location":{"kind":"core_terminal","panes":"t1"}}
    ]});
    let mut saved = SavedTabs::parse(&serde_json::to_vec(&json).unwrap()).unwrap();
    let new = TerminalId::new("t3").unwrap();
    let map = BTreeMap::from([(TerminalId::new("t1").unwrap(), new.clone())]);
    saved.remap("local:a", &map);
    assert_eq!(
        saved.core_tabs("local:a").next().unwrap().panes.first(),
        &new
    );
    assert_eq!(
        saved
            .pane_names("local:a", 1)
            .unwrap()
            .get("t3")
            .map(String::as_str),
        Some("mine")
    );
    assert_eq!(saved.tab_zoomed("local:a", 1), Some(&new));
    assert_eq!(
        saved
            .core_tabs("remote:b")
            .next()
            .unwrap()
            .panes
            .first()
            .as_str(),
        "t1"
    );
}
