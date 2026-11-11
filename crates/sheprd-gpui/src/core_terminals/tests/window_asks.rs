//! Asks raised while the owning window is being updated must wait for it.

use super::commands::flash;
use crate::{
    core_terminals::{asks::window_call, view::ToWindow},
    i18n::{Localize as _, fl},
    window::Flash,
};
use gpui::TestAppContext;

#[gpui::test]
fn an_ask_made_while_the_window_updates_lands_after_it(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(crate::palette::fixture_window);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.config.clipboard_toast.enabled = true;
            window_call(cx)(ToWindow::Copied, window, cx);
            assert!(view.flash.is_none());
        });
    });

    cx.run_until_parked();
    assert_eq!(flash(&view, cx), Some(fl!("window-find-copied")));
}

#[gpui::test]
fn asks_keep_the_order_they_were_made_in(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(crate::palette::fixture_window);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.config.clipboard_toast.enabled = true;
            let ask = window_call(cx);
            ask(ToWindow::Copied, window, cx);
            ask(ToWindow::Refused(crate::Error::MalformedLink), window, cx);
            assert!(view.flash.is_none());
        });
    });

    cx.run_until_parked();
    cx.update(|_, cx| {
        assert_eq!(
            view.read(cx).flash.as_ref().map(|(flash, _)| flash),
            Some(&Flash::warning(crate::Error::MalformedLink.localize()))
        );
    });
}
