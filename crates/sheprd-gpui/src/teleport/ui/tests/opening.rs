use super::*;
use crate::core_terminals::panes::{Axis, Panes};
use core::prelude::v1::test;
use sheprd_backend::TerminalId;

fn id(name: &str) -> TerminalId {
    TerminalId::new(name).unwrap()
}

/// The tabs a move rebuilt in the destination's Herdr 2 open in its new
/// workspace, on that host, with their splits.
#[gpui::test]
fn a_moves_herdr_2_tabs_open_in_the_destination_workspace(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(crate::sidebar::layout_tests::fixture_window);
    let mut split = Panes::single(id("t5"));
    split.split(&id("t5"), Axis::Column, id("t6")).unwrap();
    let tabs = vec![split, Panes::single(id("t7"))];
    let outcome = Outcome {
        endpoint_id: "buildbox".into(),
        workspace: Landed::Workspace("w9".into()),
        repo_key: "repo".into(),
        branch: "feature".into(),
        warnings: Vec::new(),
        core_tabs: tabs.clone(),
    };
    let scope = Scope::endpoint("buildbox");
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.settle_teleport(&outcome, &scope, window, cx)
        })
    });
    cx.update(|_, cx| {
        let opened: Vec<Panes> = cx
            .global::<Store>()
            .in_workspace(&Scope::endpoint("buildbox"), "w9")
            .filter_map(|tab| match &tab.location {
                Some(Location::CoreTerminal {
                    panes,
                    observe: false,
                    ..
                }) => Some(panes.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(opened, tabs);
    });
}

/// A move whose window closed before it finished leaves its tabs for the
/// next window that ticks: the source's moved tab closes and the rebuilt
/// ones open. A window still there takes the outcome itself.
#[gpui::test]
fn another_window_finishes_a_move_whose_window_closed(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(crate::sidebar::layout_tests::fixture_window);
    let moved = Panes::single(id("t1"));
    let source = Scope::endpoint("laptop");
    let source_tab = cx.update(|_, cx| {
        let location = Location::core_terminal(moved.clone(), false);
        Store::update(cx, |store| store.open(source, "w1", Some(location), None)).unwrap()
    });
    let outcome = || Outcome {
        endpoint_id: "buildbox".into(),
        workspace: Landed::Workspace("w9".into()),
        repo_key: "repo".into(),
        branch: "feature".into(),
        warnings: Vec::new(),
        core_tabs: vec![Panes::single(id("t5"))],
    };
    let unclaimed = Unclaimed::default();
    // The destination as the starting window named it, which the claiming
    // window, with its own `buildbox`, must not rename.
    let destination = Scope::local(std::path::Path::new("/run/default.sock"));
    let leftover = || unclaimed::Handover {
        scope: destination.clone(),
        closing: vec![(source_tab, moved.clone())],
        source: super::source(),
        source_scope: Scope::endpoint("laptop"),
    };

    let (sender, receiver) = mpsc::channel();
    unclaimed::deliver(&sender, Ok(outcome()), leftover(), &unclaimed);
    assert!(matches!(receiver.try_recv(), Ok(Event::Moved(Ok(_)))));

    drop(receiver);
    unclaimed::deliver(&sender, Ok(outcome()), leftover(), &unclaimed);
    cx.update(|window, cx| {
        cx.set_global(unclaimed);
        view.update(cx, |view, cx| view.poll_teleport(window, cx));
        let store = cx.global::<Store>();
        assert!(store.get(source_tab).is_none());
        assert_eq!(store.in_workspace(&destination, "w9").count(), 1);
        assert_eq!(
            store
                .in_workspace(&Scope::endpoint("buildbox"), "w9")
                .count(),
            0
        );
        // Taken once.
        assert!(cx.global::<Unclaimed>().clone().is_empty());
    });
}
