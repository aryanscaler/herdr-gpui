//! The process list of a herdr-core pane, from its menu.

use super::commands::{id, window_with_split};
use crate::{
    core_terminals::{
        Host,
        menu::{Action, Target},
    },
    menu::Page,
    processes::Source,
};
use gpui::{TestAppContext, point, px};

#[gpui::test]
fn only_this_computers_panes_list_their_processes(cx: &mut TestAppContext) {
    let (view, cx, tab) = window_with_split(cx, &["t1"]);
    cx.update(|_, cx| {
        view.update(cx, |view, _| {
            assert_eq!(
                view.core_processes_source(tab, &id("t1")),
                Some(Source::AppCore {
                    terminal: "t1".into()
                })
            );
            assert_eq!(view.core_processes_source(tab, &id("t9")), None);
            view.core_terminals.tabs.get_mut(&tab).unwrap().host =
                Host::Remote(crate::browser::Scope::endpoint("ssh:box"));
            assert_eq!(view.core_processes_source(tab, &id("t1")), None);
        })
    });
    let pane = Target::Pane {
        tab,
        terminal: id("t1"),
    };
    let actions = cx.update(|_, cx| view.read(cx).core_menu_actions(&pane, cx));
    assert!(!actions.contains(&Action::Processes));
}

#[gpui::test]
fn the_process_row_opens_the_list_until_its_pane_goes(cx: &mut TestAppContext) {
    let (view, cx, tab) = window_with_split(cx, &["t1", "t2"]);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            let pane = Target::Pane {
                tab,
                terminal: id("t2"),
            };
            view.open_core_menu(pane, point(px(10.), px(10.)), window, cx);
            view.activate_core_menu(Action::Processes, window, cx);
            assert_eq!(view.menu.page, Some(Page::PaneProcesses));
            assert!(view.core_processes_current().is_ok());
            assert!(
                view.core_terminals
                    .tabs
                    .get_mut(&tab)
                    .unwrap()
                    .panes
                    .remove(&id("t2"))
            );
            assert!(matches!(
                view.core_processes_current(),
                Err(crate::Error::StalePane)
            ));
        })
    });
}
