use super::*;
use crate::{
    browser::{Location, Store},
    core_terminals::{Host, panes::Panes},
};
use sheprd_backend::TerminalId;
use sheprd_client::ConnectTarget;

#[gpui::test]
fn control_notes_and_feedback_resolve_remote_keys_without_local_aliases(
    cx: &mut gpui::TestAppContext,
) {
    let window = cx.add_window(crate::sidebar::layout_tests::fixture_window);
    let key = window
        .update(cx, |view, _, cx| {
            let endpoint = crate::endpoint::Endpoint::new(
                "ssh:notes".into(),
                "Notes".into(),
                ConnectTarget::Ssh {
                    target: "notes".into(),
                    session: "work".into(),
                },
                true,
            );
            let scope = crate::browser::scope(&endpoint);
            view.endpoints.push(endpoint);
            let host = Host::Remote(scope.clone());
            let terminal = TerminalId::new("t1").unwrap();
            let pane_id = crate::chat::core_pane::key_on(&host, &terminal);
            let location = Location::core_terminal(Panes::single(terminal.clone()), false);
            let tab = Store::update(cx, |store| {
                store.open(scope.clone(), "kept", Some(location), None)
            })
            .unwrap();
            view.core_terminals
                .show_on_host_for_test(tab, host, terminal);
            view.live.snapshot = None;
            FeedbackKey { scope, pane_id }
        })
        .unwrap();
    let request = NotesRequest {
        caller: Caller {
            pane_id: Some(key.pane_id.clone()),
            daemon_socket: Some("/local.sock".into()),
            ..Default::default()
        },
        text: "remote only".into(),
    };
    cx.update(|cx| {
        assert_eq!(core_scope(&key.pane_id, cx), Some(key.scope.clone()));
        assert_eq!(core_scope("core:t1", cx), None);
        assert_eq!(
            notes(&request, cx),
            Response::NotesSent { to: NotesTo::Kept }
        );
        assert!(cx.global::<Feedback>().has(&key));
    });
    let address = socket::test_address();
    let server = socket::Server::bind(&address.path).unwrap();
    let request = FeedbackRequest {
        pane_id: key.pane_id,
        daemon_socket: Some("/local.sock".into()),
        wait_seconds: 0,
    };
    let (incoming, call) = scoped_notes::queued_feedback(&server, &request);
    cx.update(|cx| feedback(&request, incoming, &mut Vec::new(), cx));
    assert_eq!(
        call.join().unwrap().unwrap(),
        Response::Feedback {
            text: Some("remote only".into())
        }
    );
}

#[gpui::test]
fn missing_remote_control_callers_are_refused_instead_of_queued_locally(
    cx: &mut gpui::TestAppContext,
) {
    let pane = crate::chat::core_pane::key_on(
        &Host::Remote(Scope::endpoint("ssh:gone")),
        &TerminalId::new("t1").unwrap(),
    );
    let request = NotesRequest {
        caller: Caller {
            pane_id: Some(pane.clone()),
            daemon_socket: Some("/local.sock".into()),
            ..Default::default()
        },
        text: "not local".into(),
    };
    cx.update(|cx| {
        assert!(matches!(
            notes(&request, cx),
            Response::Error {
                code: ErrorCode::NotFound,
                ..
            }
        ))
    });
    let address = socket::test_address();
    let server = socket::Server::bind(&address.path).unwrap();
    let request = FeedbackRequest {
        pane_id: pane,
        daemon_socket: Some("/local.sock".into()),
        wait_seconds: 0,
    };
    let (incoming, call) = scoped_notes::queued_feedback(&server, &request);
    cx.update(|cx| feedback(&request, incoming, &mut Vec::new(), cx));
    assert!(matches!(
        call.join().unwrap().unwrap(),
        Response::Error {
            code: ErrorCode::NotFound,
            ..
        }
    ));
}
