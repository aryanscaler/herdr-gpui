#![allow(clippy::unwrap_used)]
use super::*;
use crate::{
    browser::{Location, Store},
    core_terminals::{Host, panes::Panes},
};
use sheprd_backend::TerminalId;
use sheprd_client::ConnectTarget;

#[gpui::test]
fn remote_core_notes_keep_their_host_without_a_herdr_session(cx: &mut gpui::TestAppContext) {
    let (view, cx) = cx.add_window_view(crate::sidebar::layout_tests::fixture_window);
    view.update(cx, |view, cx| {
        let terminal = TerminalId::new("t1").unwrap();
        let mut keys = Vec::new();
        for host in ["one", "two"] {
            let endpoint = crate::endpoint::Endpoint::new(
                format!("ssh:{host}"),
                host.into(),
                ConnectTarget::Ssh {
                    target: host.into(),
                    session: "work".into(),
                },
                true,
            );
            let scope = crate::browser::scope(&endpoint);
            view.endpoints.push(endpoint);
            let host = Host::Remote(scope.clone());
            let pane_id = crate::chat::core_pane::key_on(&host, &terminal);
            let location = Location::core_terminal(Panes::single(terminal.clone()), false);
            let tab = Store::update(cx, |store| {
                store.open(scope.clone(), "kept", Some(location), None)
            })
            .unwrap();
            view.core_terminals
                .show_on_host_for_test(tab, host, terminal.clone());
            keys.push(FeedbackKey { scope, pane_id });
        }
        // Local stays selected; neither remote host needs any Herdr 1 snapshot.
        view.live.snapshot = None;
        assert_eq!(
            view.core_pane_scope(&keys[0].pane_id),
            Some(keys[0].scope.clone())
        );
        let delivered = view.deliver_from(
            None,
            Some(keys[0].pane_id.clone()),
            "first host".into(),
            None,
            cx,
        );
        assert!(matches!(delivered, Delivered::Kept));
        assert!(cx.global::<Feedback>().has(&keys[0]));
        assert!(!cx.global::<Feedback>().has(&keys[1]));
        // A closed/unheld remote key never borrows the current Herdr session.
        view.live.snapshot = Some(Arc::new(crate::sidebar::layout_tests::snapshot(1)));
        let origin = Origin::of(view).unwrap();
        let missing = crate::chat::core_pane::key_on(
            &Host::Remote(keys[0].scope.clone()),
            &TerminalId::new("t9").unwrap(),
        );
        let delivered = view.deliver_from(
            Some(&origin),
            Some(missing.clone()),
            "do not misroute".into(),
            None,
            cx,
        );
        assert!(matches!(delivered, Delivered::Copy(_)));
        assert!(!cx.global::<Feedback>().has(&FeedbackKey {
            scope: crate::browser::scope(&view.endpoints[0]),
            pane_id: missing
        }));
    });
}
