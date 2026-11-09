//! A Herdr 2 terminal a phone started for one of this computer's
//! workspaces lands in a tab of that workspace, once.

use crate::{
    SheprdWindow,
    browser::{Location, Store},
    sidebar::layout_tests::{SidebarFixture, fixture_window, snapshot},
};
use gpui::{Entity, TestAppContext, VisualTestContext, prelude::*};
use sheprd_backend::{AgentStatus, RunState, TerminalId, TerminalInfo};
use std::sync::Arc;

/// The folder a phone asks a new workspace for: absolute on this platform,
/// as adoption requires.
const FOLDER: &str = if cfg!(windows) {
    r"C:\code\api"
} else {
    "/code/api"
};

fn id(name: &str) -> TerminalId {
    TerminalId::new(name).unwrap()
}

/// A terminal the core lists, labelled for `workspace` when given.
fn listed(name: &str, workspace: Option<&str>) -> TerminalInfo {
    TerminalInfo {
        agent: AgentStatus::Unknown,
        cwd: None,
        foreground: None,
        id: id(name),
        labels: workspace
            .map(|workspace| {
                [(
                    sheprd_session::WORKSPACE_LABEL.to_owned(),
                    workspace.to_owned(),
                )]
                .into()
            })
            .unwrap_or_default(),
        name: None,
        reporter: None,
        state: RunState::Running,
        title: None,
    }
}

/// A window on a Herdr 1 session with workspaces `w0` and `w1`.
fn window(cx: &mut TestAppContext) -> (Entity<SheprdWindow>, &mut VisualTestContext) {
    let (fixture, cx) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| fixture_window(window, cx));
        SidebarFixture(view)
    });
    let view = cx.update(|_, cx| fixture.read(cx).0.clone());
    cx.update(|_, cx| {
        Store::update(cx, |_| {});
        view.update(cx, |view, _| {
            view.live.snapshot = Some(Arc::new(snapshot(2)));
        })
    });
    (view, cx)
}

/// The workspaces of the tabs holding `terminal`.
fn tabs_of(cx: &mut VisualTestContext, terminal: &str) -> Vec<String> {
    cx.update(|_, cx| {
        cx.global::<Store>()
            .tabs()
            .filter(|tab| match &tab.location {
                Some(Location::CoreTerminal { panes, .. }) => panes.contains(&id(terminal)),
                _ => false,
            })
            .map(|tab| tab.workspace_id.clone())
            .collect()
    })
}

#[gpui::test]
fn a_terminal_labelled_for_a_workspace_gets_a_tab_there_once(cx: &mut TestAppContext) {
    let (view, cx) = window(cx);
    cx.update(|_, cx| {
        // Another host's tab holds its own `t5`, another terminal.
        Store::update(cx, |store| {
            let location =
                Location::core_terminal(sheprd_session::panes::Panes::single(id("t5")), false);
            store.open(
                crate::browser::Scope::endpoint("box"),
                "w9",
                Some(location),
                None,
            )
        });
        view.update(cx, |view, cx| {
            let infos = &mut view.core_terminals.infos;
            infos.insert(listed("t5", Some("w1")));
            // Not labelled, or for a workspace this computer does not have.
            infos.insert(listed("t6", None));
            infos.insert(listed("t7", Some("gone")));
            view.adopt_labelled_terminals(true, cx);
        })
    });
    assert_eq!(tabs_of(cx, "t5"), ["w9", "w1"]);
    assert!(tabs_of(cx, "t6").is_empty());
    assert!(tabs_of(cx, "t7").is_empty());
    // Once, even after the user closes its tab.
    cx.update(|_, cx| {
        Store::update(cx, |store| {
            let tabs: Vec<_> = store.tabs().map(|tab| tab.id).collect();
            for tab in tabs {
                store.close(tab);
            }
        });
        view.update(cx, |view, cx| view.adopt_labelled_terminals(true, cx));
    });
    assert!(tabs_of(cx, "t5").is_empty());
}

#[gpui::test]
fn a_phones_new_workspace_is_kept_in_its_folder_with_the_terminal_in_it(cx: &mut TestAppContext) {
    let (view, cx) = window(cx);
    let new = |name: &str| {
        let mut info = listed(name, None);
        info.labels.insert(
            sheprd_session::NEW_WORKSPACE_LABEL.to_owned(),
            FOLDER.to_owned(),
        );
        info
    };
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.core_terminals.infos.insert(new("t5"));
            view.adopt_labelled_terminals(true, cx);
            // A second one for the same folder joins that workspace.
            view.core_terminals.infos.insert(new("t6"));
            view.adopt_labelled_terminals(true, cx);
        })
    });
    let kept = view.read_with(cx, |view, _| {
        view.kept_workspace_at(std::path::Path::new(FOLDER))
    });
    let kept = kept.expect("a workspace kept in the folder");
    assert_eq!(tabs_of(cx, "t5"), std::slice::from_ref(&kept));
    assert_eq!(tabs_of(cx, "t6"), [kept]);
}

#[gpui::test]
fn a_phones_new_terminal_finds_its_workspace_while_another_host_is_shown(cx: &mut TestAppContext) {
    let (view, cx) = window(cx);
    let new = |name: &str| {
        let mut info = listed(name, None);
        info.labels.insert(
            sheprd_session::NEW_WORKSPACE_LABEL.to_owned(),
            FOLDER.to_owned(),
        );
        info
    };
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.core_terminals.infos.insert(new("t5"));
            view.adopt_labelled_terminals(true, cx);
            let remote = crate::endpoint::Endpoint::new(
                "ssh:test".into(),
                "Remote".into(),
                sheprd_client::ConnectTarget::Ssh {
                    target: "unused".into(),
                    session: "default".into(),
                },
                true,
            );
            view.endpoints.push(remote);
            view.endpoints[0].live = std::mem::take(&mut view.live);
            view.selected_endpoint = 1;
            view.core_terminals.infos.insert(new("t6"));
            view.adopt_labelled_terminals(true, cx);
        })
    });
    let kept: Vec<String> = cx.update(|_, cx| {
        let scope = crate::browser::scope(&view.read(cx).endpoints[0]);
        cx.global::<Store>()
            .kept_workspaces(&scope)
            .map(|kept| kept.id.clone())
            .collect()
    });
    assert_eq!(kept.len(), 1, "one workspace for the folder: {kept:?}");
    assert_eq!(tabs_of(cx, "t5"), kept);
    assert_eq!(tabs_of(cx, "t6"), kept);
}

#[gpui::test]
fn primary_session_work_goes_to_the_primary_whichever_session_is_shown(cx: &mut TestAppContext) {
    let (view, cx) = window(cx);
    cx.update(|_, cx| {
        view.update(cx, |view, _| {
            let session = |id: &str, target| {
                crate::endpoint::Endpoint::new(id.into(), id.into(), target, false)
            };
            // The daemon's primary, then a named session the window shows.
            view.endpoints = vec![
                session("primary", sheprd_client::ConnectTarget::Local),
                session(
                    "dev",
                    sheprd_client::ConnectTarget::Session {
                        name: "dev-shown".into(),
                        development: false,
                    },
                ),
            ];
            view.selected_endpoint = 1;
            assert_eq!(view.core_host(), Some(1), "the shown session");
            assert_eq!(view.endpoint_of_session(None), Some(0));
            assert_eq!(view.endpoint_of_session(Some("dev-shown")), Some(1));
        })
    });
}

#[test]
fn an_adopted_terminal_loses_what_adopts_it_and_keeps_its_chat() {
    use sheprd_session::{NEW_WORKSPACE_LABEL, SESSION_LABEL, WORKSPACE_LABEL};
    let mut labels: std::collections::BTreeMap<String, String> = [
        (WORKSPACE_LABEL, "w1"),
        (SESSION_LABEL, "dev"),
        (NEW_WORKSPACE_LABEL, FOLDER),
        (sheprd_chat_protocol::TOKEN, "endpoint"),
    ]
    .into_iter()
    .map(|(key, value)| (key.to_owned(), value.to_owned()))
    .collect();
    // As herdr-core applies `terminal.label`: an empty value removes one.
    for (label, value) in crate::core_terminals::adopt::released_labels() {
        assert!(value.is_empty());
        labels.remove(label);
    }
    assert_eq!(sheprd_session::labelled_workspace(&labels), None);
    assert_eq!(sheprd_session::new_workspace(&labels), None);
    assert!(
        labels.contains_key(sheprd_chat_protocol::TOKEN),
        "still a chat"
    );
}

#[test]
fn an_adopted_terminal_keeps_its_labels_until_its_tab_is_saved() {
    use crate::core_terminals::adopt::saved_holds;
    let saved = |scope: &str, panes: &str| {
        serde_json::to_vec(&serde_json::json!({"tabs": [{
            "id": 1, "scope": scope, "workspace_id": "w1", "title": "t",
            "location": {"kind": "core_terminal", "panes": panes},
        }]}))
        .unwrap()
    };
    assert!(saved_holds(&saved("s", "t1"), "s", &id("t1")));
    assert!(!saved_holds(&saved("s", "t2"), "s", &id("t1")), "not yet");
    assert!(
        !saved_holds(&saved("other", "t1"), "s", &id("t1")),
        "another host"
    );
    assert!(!saved_holds(b"{", "s", &id("t1")), "a damaged file");
}
