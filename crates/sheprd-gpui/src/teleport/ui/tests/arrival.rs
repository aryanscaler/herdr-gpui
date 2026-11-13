use super::*;
use crate::browser::{Location, Store};
use core::prelude::v1::test;
use sheprd_backend::TerminalId;
use sheprd_session::panes::Panes;

#[gpui::test]
fn replaying_an_arrival_does_not_duplicate_or_stop_its_terminals(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(crate::sidebar::layout_tests::fixture_window);
    let scope = cx.update(|_, cx| view.read(cx).endpoint_scope("local"));
    let outcome = Outcome {
        source_retired: false,
        endpoint_id: "local".into(),
        workspace: Landed::New {
            checkout: "/recovered/feature".into(),
            label: None,
        },
        repo_key: "/recovered/.git".into(),
        branch: "feature".into(),
        warnings: Vec::new(),
        core_tabs: vec![Panes::single(TerminalId::new("t99").unwrap())],
    };
    cx.update(|window, cx| {
        let first = view.update(cx, |view, cx| view.settle_teleport(&outcome, &scope, window, cx)).unwrap();
        let second = view.update(cx, |view, cx| view.settle_teleport(&outcome, &scope, window, cx)).unwrap();
        assert_eq!(first, second);
        let held = cx.global::<Store>().in_workspace(&scope, &first).filter(|tab|
            matches!(&tab.location, Some(Location::CoreTerminal { panes, .. }) if panes.contains(&TerminalId::new("t99").unwrap()))).count();
        assert_eq!(held, 1);
    });
}

#[test]
fn a_retirement_warning_does_not_authorize_closing_source_tabs() {
    let mut teleport = Teleport::start(source(), "feature".into(), Vec::new());
    let (sender, receiver) = mpsc::channel();
    teleport.events = receiver;
    sender.send(Event::Step(Step::Retire)).unwrap();
    teleport.poll();
    assert!(
        !teleport.retired,
        "starting retirement is not acknowledgement"
    );
}
