//! Copy mode in a herdr-core view goes on working after the terminal is
//! attached again: a motion the dropped link never answered stops holding
//! back the keys after it.

use super::{
    history::{point, requests, shown},
    worker::{Scripted, size},
};
use crate::core_terminals::worker;
use sheprd_backend::{CopyMotion, History, Role, TerminalId};

#[gpui::test]
fn copy_mode_answers_keys_again_once_attached_again(cx: &mut gpui::TestAppContext) {
    let (view, cx, first) = shown(cx);
    cx.update(|window, cx| view.update(cx, |view, cx| view.enter_copy_mode(window, cx)));
    // A motion goes out, and the connection drops before its answer is read.
    cx.simulate_keystrokes("w");
    requests(&first, 1);
    let second = Scripted::default();
    let terminal = TerminalId::new("t1").unwrap();
    let link = worker::spawn(second.clone(), terminal, size(10, 3), Role::Active).unwrap();
    cx.update(|_, cx| view.update(cx, |view, cx| view.relink(link, cx)));
    // The next key asks the new link rather than waiting on the old one.
    cx.simulate_keystrokes("b");
    assert_eq!(
        requests(&second, 1)[0],
        History::Motion {
            cursor: point(9, 0),
            motion: CopyMotion::PreviousWordStart,
        }
    );
    cx.update(|_, cx| assert!(view.read(cx).copy_mode_active()));
}
