//! Dragging across wrapped code rows that carry line numbers.
use super::*;

const FIRST: &str =
    "let first = \"one long line of code that has to wrap inside the narrow chat column\";";
const SECOND: &str = "short();";
const THIRD: &str =
    "let third = \"another long line of code that also wraps over several visual rows\";";

#[gpui::test]
fn wrapped_numbered_code_copies_exactly_in_both_drag_directions(cx: &mut TestAppContext) {
    let (view, _peer, cx) = open(cx, ConnectTarget::Local);
    narrow(&view, ChatLayoutMode::Document, cx);
    view.update(cx, |view, _| {
        view.config.chat.wrap_code = true;
        view.config.chat.code_line_numbers = true;
    });
    let program = Program::new();
    let source = format!("```rust\n{FIRST}\n{SECOND}\n{THIRD}\n```");
    let _stream = attach_with(
        &view,
        &program,
        snapshot(cursor(1, 0), vec![agent(1, &source)]),
        cx,
    );
    acknowledge_size(&view, cx);
    poll(&view, &program, cx);
    poll(&view, &program, cx);
    assert!(
        cx.debug_bounds("chat-code-gutter").is_none(),
        "wrapped rows carry their own numbers"
    );
    let start = position(&view, caret(&view, 0, 0, 0, "let ".len(), cx), cx);
    let first_end = position(&view, caret(&view, 0, 0, 0, FIRST.len(), cx), cx);
    assert!(first_end.y > start.y + px(10.), "the first line must wrap");
    let cut = THIRD.len() - "rows\";".len();
    let end = position(&view, caret(&view, 0, 2, 0, cut, cx), cx);
    let third_start = position(&view, caret(&view, 0, 2, 0, 0, cx), cx);
    assert!(
        end.y > third_start.y + px(10.),
        "the end lies on a wrapped row"
    );
    let expected = format!("{}\n{SECOND}\n{}", &FIRST["let ".len()..], &THIRD[..cut]);
    for (from, to) in [(start, end), (end, start)] {
        cx.write_to_clipboard(gpui::ClipboardItem::new_string("not copied".into()));
        drag(cx, from, to);
        let selected = view.read_with(cx, |view, _| {
            let pane = view.chats.pane("w1:p1").unwrap();
            pane.selection.text(pane.prepared.text())
        });
        assert_eq!(
            selected.as_deref(),
            Some(expected.as_str()),
            "{from:?} to {to:?}"
        );
        cx.simulate_keystrokes(copy_key());
        assert_eq!(clipboard(cx).as_deref(), Some(expected.as_str()));
    }
}
