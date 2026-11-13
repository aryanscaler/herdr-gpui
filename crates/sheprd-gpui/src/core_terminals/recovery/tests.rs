use super::*;
use sheprd_session::panes::{Axis, Panes};

mod stale_window;

#[test]
fn replacing_a_chat_preserves_split_focus_zoom_and_user_names() {
    let old = TerminalId::new("t1").unwrap();
    let other = TerminalId::new("t2").unwrap();
    let new = TerminalId::new("t3").unwrap();
    let mut panes = Panes::single(old.clone());
    panes.split(&old, Axis::Row, other.clone()).unwrap();
    let mut shown = TabPanes::new(Host::Local, panes, false);
    shown.active = old.clone();
    shown.previous = Some(old.clone());
    shown.zoomed = Some(old.clone());
    shown.names.insert(old.clone(), "My chat".into());
    let mapping = BTreeMap::from([(old.clone(), new.clone())]);
    assert!(shown.remap(&mapping));
    assert!(!shown.panes.contains(&old));
    assert!(shown.panes.contains(&new) && shown.panes.contains(&other));
    assert_eq!(shown.active, new);
    assert_eq!(shown.previous.as_ref(), Some(&new));
    assert_eq!(shown.zoomed.as_ref(), Some(&new));
    assert_eq!(shown.names.get(&new).map(String::as_str), Some("My chat"));
    assert!(!shown.remap(&mapping));
}
