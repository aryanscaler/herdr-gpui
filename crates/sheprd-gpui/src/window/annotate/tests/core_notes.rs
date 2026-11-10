//! Notes on a Herdr 2 pane go to its agent through the core; what no agent
//! takes is copied.

use super::*;
use crate::{
    terminal_notes::Note,
    window::annotate::{Queued, core::CoreAgent},
};
use sheprd_backend::TerminalId;
use sheprd_core_client::reply::Replied;

fn copied(cx: &mut VisualTestContext) -> Option<String> {
    cx.update(|_, cx| cx.read_from_clipboard().and_then(|item| item.text()))
}

fn queued(terminal: &str, comment: &str, agent: Option<&str>) -> Queued {
    let core = agent.map(|agent| CoreAgent {
        host: crate::core_terminals::Host::Local,
        terminal: TerminalId::new(terminal).unwrap(),
        agent: agent.into(),
    });
    Queued {
        origin: None,
        note: Note::new(
            core.as_ref().map(|agent| agent.terminal.to_string()),
            "api",
            "error: build failed",
            comment,
        )
        .unwrap(),
        core,
    }
}

#[gpui::test]
fn notes_for_an_unreachable_herdr_2_agent_are_copied(cx: &mut TestAppContext) {
    let (view, cx) = window(cx, shown(None, false));
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.terminal_notes.queued = vec![
                queued("t1", "Look at the build.", Some("claude")),
                queued("t2", "No agent here.", None),
            ];
            view.send_terminal_notes(cx);
        })
    });
    // The tab has no core to reach: both are copied, in one write.
    let text = copied(cx).unwrap();
    assert!(text.contains("Look at the build."));
    assert!(text.contains("No agent here."));
    view.read_with(cx, |view, _| assert_eq!(view.terminal_notes.queued(), 0));
}

#[gpui::test]
fn notes_a_herdr_2_agent_did_not_take_are_copied(cx: &mut TestAppContext) {
    let (view, cx) = window(cx, shown(None, false));
    for (sent, copies) in [
        (Ok(Replied::Sent), false),
        (Ok(Replied::Gone), true),
        (
            Err(sheprd_core_client::Error::Refused {
                method: sheprd_core_client::Method::TerminalSend,
                kind: Some(sheprd_core_client::wire::BLOCKED.into()),
                message: "blocked".into(),
            }),
            true,
        ),
    ] {
        cx.update(|_, cx| cx.write_to_clipboard(gpui::ClipboardItem::new_string("before".into())));
        cx.update(|_, cx| {
            view.update(cx, |view, cx| {
                let copies = crate::agent_notes::Copies::default();
                view.core_notes_delivered(sent, "the notes".into(), &copies, cx);
            })
        });
        let expected = if copies { "the notes" } else { "before" };
        assert_eq!(copied(cx).as_deref(), Some(expected));
    }
}

#[gpui::test]
fn a_later_refusal_keeps_the_sends_earlier_copies(cx: &mut TestAppContext) {
    let (view, cx) = window(cx, shown(None, false));
    let gone = || Ok(Replied::Gone);
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            let copies = crate::agent_notes::Copies::default();
            view.core_notes_delivered(gone(), "first agent".into(), &copies, cx);
            view.core_notes_delivered(gone(), "second agent".into(), &copies, cx);
        })
    });
    let text = copied(cx).unwrap();
    assert!(text.contains("first agent") && text.contains("second agent"));
}
