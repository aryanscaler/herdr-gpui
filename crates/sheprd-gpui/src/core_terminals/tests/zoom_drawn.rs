//! Which panes of a zoomed Herdr 2 tab count as drawn, so a note follows
//! only a pane whose body is on screen.

use crate::core_terminals::{
    Host, TabPanes,
    panes::{Axis, Panes},
};
use sheprd_backend::TerminalId;
use std::time::Duration;

fn id(name: &str) -> TerminalId {
    TerminalId::new(name).unwrap()
}

#[test]
fn a_landed_zoom_draws_only_its_pane() {
    let mut panes = Panes::single(id("t1"));
    panes.split(&id("t1"), Axis::Row, id("t2")).unwrap();
    let mut tab = TabPanes::new(Host::Local, panes, false);
    let now = crate::motion::now();
    assert_eq!(tab.drawn_at(now), [&id("t1"), &id("t2")]);
    tab.set_zoom(Some(id("t2")));
    // While it grows, the split still shows behind it.
    assert_eq!(tab.drawn_at(crate::motion::now()).len(), 2);
    let landed = crate::motion::now() + Duration::from_secs(5);
    assert_eq!(tab.drawn_at(landed), [&id("t2")]);
}
