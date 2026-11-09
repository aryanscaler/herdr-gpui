//! The right-click menus of herdr-core tabs and panes, and naming them.

use super::commands::{flash, id, run, window_with_split};
use crate::{
    browser::Store,
    controls::Command,
    core_terminals::menu::{Action, Target, clean_name},
    menu::Page,
};
use gpui::{TestAppContext, point, px};

#[gpui::test]
fn a_tab_menu_offers_a_new_tab_renaming_and_closing(cx: &mut TestAppContext) {
    let (view, cx, tab) = window_with_split(cx, &["t1"]);
    let actions = cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.open_core_menu(Target::Tab(tab), point(px(10.), px(10.)), window, cx);
            assert_eq!(view.menu.page, Some(Page::CoreMenu));
            assert_eq!(view.menu.core.as_ref().unwrap().target(), &Target::Tab(tab));
            view.core_menu_actions(&Target::Tab(tab))
        })
    });
    assert_eq!(actions, [Action::NewTab, Action::Rename, Action::Close]);
}

#[gpui::test]
fn a_pane_menu_offers_what_its_pane_can_do(cx: &mut TestAppContext) {
    let (view, cx, tab) = window_with_split(cx, &["t1", "t2"]);
    let pane = Target::Pane {
        tab,
        terminal: id("t2"),
    };
    let actions = cx.update(|_, cx| view.read(cx).core_menu_actions(&pane));
    assert_eq!(
        actions,
        [
            Action::Rename,
            Action::SplitRight,
            Action::SplitDown,
            Action::Zoom,
            Action::MoveTo,
            Action::Mirror,
            Action::Close,
        ]
    );
    // Zoomed, the row unzooms.
    run(&view, cx, Command::FocusRight);
    run(&view, cx, Command::Zoom);
    let actions = cx.update(|_, cx| view.read(cx).core_menu_actions(&pane));
    assert!(actions.contains(&Action::Unzoom) && !actions.contains(&Action::Zoom));
}

#[gpui::test]
fn a_mirror_pane_only_closes(cx: &mut TestAppContext) {
    let (view, cx, tab) = window_with_split(cx, &["t1"]);
    cx.update(|_, cx| {
        view.update(cx, |view, _| {
            view.core_terminals.tabs.get_mut(&tab).unwrap().observe = true;
        })
    });
    let pane = Target::Pane {
        tab,
        terminal: id("t1"),
    };
    let actions = cx.update(|_, cx| view.read(cx).core_menu_actions(&pane));
    assert_eq!(actions, [Action::Close]);
    run(&view, cx, Command::RenamePane);
    assert_eq!(
        flash(&view, cx),
        Some(crate::i18n::fl!("core-terminals-unsupported"))
    );
}

#[gpui::test]
fn rename_opens_on_the_focused_tab_and_a_name_stands_over_the_title(cx: &mut TestAppContext) {
    let (view, cx, tab) = window_with_split(cx, &["t1", "t2"]);
    run(&view, cx, Command::RenameTab);
    let page = cx.update(|_, cx| view.read(cx).menu.page);
    assert_eq!(page, Some(Page::RenameCore));

    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.set_core_name(&Target::Tab(tab), clean_name("  build  "), cx);
            view.set_core_name(
                &Target::Pane {
                    tab,
                    terminal: id("t2"),
                },
                clean_name("tests"),
                cx,
            );
        })
    });
    let title = cx.update(|_, cx| cx.global::<Store>().get(tab).unwrap().title.clone());
    assert_eq!(title, "build");
    let label = cx.update(|_, cx| view.read(cx).core_pane_label(tab, &id("t2"), cx).0);
    assert_eq!(label, "tests");

    // A blank name gives the program's title back.
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.set_core_name(
                &Target::Pane {
                    tab,
                    terminal: id("t2"),
                },
                clean_name("   "),
                cx,
            )
        })
    });
    let label = cx.update(|_, cx| view.read(cx).core_pane_label(tab, &id("t2"), cx).0);
    assert_ne!(label, "tests");
}

#[gpui::test]
fn the_strip_shows_the_pane_in_use_by_name_and_a_cleared_name_goes(cx: &mut TestAppContext) {
    let (view, cx, tab) = window_with_split(cx, &["t1", "t2"]);
    let title = |cx: &mut gpui::VisualTestContext| {
        cx.update(|_, cx| cx.global::<Store>().get(tab).unwrap().title.clone())
    };
    let opened = title(cx);
    let pane = Target::Pane {
        tab,
        terminal: id("t1"),
    };
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.set_core_name(&pane, clean_name("api"), cx)
        })
    });
    assert_eq!(title(cx), "api", "the pane in use names its tab");
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.set_core_name(&Target::Tab(tab), clean_name("build"), cx);
            view.set_core_name(&Target::Tab(tab), None, cx);
            view.set_core_name(&pane, None, cx);
        })
    });
    // No program title came, so the tab reads as it opened.
    assert_eq!(title(cx), opened);
}

#[gpui::test]
fn moving_on_from_a_named_pane_drops_its_name_from_the_strip(cx: &mut TestAppContext) {
    let (view, cx, tab) = window_with_split(cx, &["t1", "t2"]);
    let title = |cx: &mut gpui::VisualTestContext| {
        cx.update(|_, cx| cx.global::<Store>().get(tab).unwrap().title.clone())
    };
    let opened = title(cx);
    let pane = Target::Pane {
        tab,
        terminal: id("t1"),
    };
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.set_core_name(&pane, clean_name("api"), cx)
        })
    });
    assert_eq!(title(cx), "api");
    // t2 has no name, and its program set no title yet.
    run(&view, cx, Command::FocusRight);
    cx.update(|window, cx| view.update(cx, |view, cx| view.poll_core_panes(window, cx)));
    assert_eq!(title(cx), opened);
}

#[gpui::test]
fn closing_a_pane_while_disconnected_ends_it_once_the_core_answers(cx: &mut TestAppContext) {
    let (view, cx, tab) = window_with_split(cx, &["t1", "t2"]);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.close_core_pane(tab, id("t2"), window, cx)
        })
    });
    let unended = cx.update(|_, cx| view.read(cx).core_terminals.unended.lock().unwrap().clone());
    assert_eq!(unended, [(crate::core_terminals::Host::Local, id("t2"))]);
}

#[gpui::test]
fn a_menu_left_on_a_pane_that_went_closes_nothing(cx: &mut TestAppContext) {
    let (view, cx, tab) = window_with_split(cx, &["t1", "t2"]);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            // t2's program ended while its menu was open.
            view.remove_core_pane(tab, &id("t2"), window, cx);
            view.close_core_pane(tab, id("t2"), window, cx);
        })
    });
    let open = cx.update(|_, cx| cx.global::<Store>().get(tab).is_some());
    assert!(open, "t1, the pane left, still runs in its tab");
}

#[gpui::test]
fn closing_a_menu_gives_the_keyboard_back_to_the_pane(cx: &mut TestAppContext) {
    let (view, cx, tab) = window_with_split(cx, &["t1", "t2"]);
    run(&view, cx, Command::RenameTab);
    let focused = cx.update(|window, cx| {
        view.update(cx, |view, cx| view.dismiss_menu(window, cx));
        let view = view.read(cx);
        view.core_terminals.views[&(tab, id("t1"))]
            .read(cx)
            .focus_handle()
            .is_focused(window)
    });
    assert!(focused);
}

#[gpui::test]
fn closing_a_menu_gives_the_keyboard_back_to_what_held_it(cx: &mut TestAppContext) {
    let (view, cx, tab) = window_with_split(cx, &["t1", "t2"]);
    // A chat's composer over the pane, say, holds the keyboard.
    let composer = cx.update(|window, cx| {
        let composer = cx.focus_handle();
        window.focus(&composer, cx);
        composer
    });
    let kept = cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            let pane = Target::Pane {
                tab,
                terminal: id("t2"),
            };
            view.open_core_menu(pane, point(px(10.), px(10.)), window, cx);
            assert_eq!(view.menu.page, Some(Page::CoreMenu));
            view.dismiss_menu(window, cx);
        });
        composer.is_focused(window)
    });
    assert!(kept, "not the terminal behind it");
}

#[gpui::test]
fn a_pane_that_exits_under_its_menu_hands_the_keyboard_on(cx: &mut TestAppContext) {
    let (view, cx, tab) = window_with_split(cx, &["t1", "t2"]);
    run(&view, cx, Command::FocusRight);
    let focused = cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            let pane = Target::Pane {
                tab,
                terminal: id("t2"),
            };
            view.open_core_menu(pane, point(px(10.), px(10.)), window, cx);
            // Its program exits while the menu is open.
            assert!(view.remove_core_pane(tab, &id("t2"), window, cx));
            view.core_terminals.views.remove(&(tab, id("t2")));
            view.dismiss_menu(window, cx);
        });
        let view = view.read(cx);
        view.core_terminals.views[&(tab, id("t1"))]
            .read(cx)
            .focus_handle()
            .is_focused(window)
    });
    assert!(focused);
}

#[test]
fn a_name_is_one_bounded_line() {
    assert_eq!(clean_name(" a\u{202e}b\n"), Some("ab".into()));
    assert_eq!(clean_name(""), None);
    assert_eq!(clean_name(&"x".repeat(500)).unwrap().chars().count(), 128);
}

#[gpui::test]
fn closing_a_pane_from_its_menu_keeps_the_tab(cx: &mut TestAppContext) {
    let (view, cx, tab) = window_with_split(cx, &["t1", "t2"]);
    run(&view, cx, Command::ClosePane);
    let panes = cx.update(|_, cx| view.read(cx).core_terminals.tabs[&tab].panes.count());
    assert_eq!(panes, 1);
}

#[gpui::test]
fn closing_a_pane_that_already_exited_leaves_its_tab(cx: &mut TestAppContext) {
    let (view, cx, tab) = window_with_split(cx, &["t1"]);
    // The menu named `t2`, which exited while it was open.
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.close_core_pane(tab, id("t2"), window, cx)
        })
    });
    let open = cx.update(|_, cx| cx.global::<Store>().get(tab).is_some());
    assert!(open, "the tab and its remaining terminal stay");
    let shown = cx.update(|_, cx| view.read(cx).core_terminals.tabs.contains_key(&tab));
    assert!(shown);
}

#[gpui::test]
fn zooming_a_pane_that_already_exited_changes_nothing(cx: &mut TestAppContext) {
    let (view, cx, tab) = window_with_split(cx, &["t1", "t2"]);
    let gone = Target::Pane {
        tab,
        terminal: id("t9"),
    };
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.open_core_menu(gone, point(px(10.), px(10.)), window, cx);
            view.activate_core_menu(Action::Zoom, window, cx);
            let shown = &view.core_terminals.tabs[&tab];
            assert_eq!(shown.zoomed, None, "the split stays drawn");
            assert!(view.menu.core.is_none(), "the menu still closes");
        })
    });
}

#[gpui::test]
fn a_cleared_name_gives_way_to_the_default_title(cx: &mut TestAppContext) {
    let (view, cx, tab) = window_with_split(cx, &["t1"]);
    let title = |cx: &mut gpui::VisualTestContext| {
        cx.update(|_, cx| cx.global::<Store>().get(tab).unwrap().title.clone())
    };
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.set_core_name(&Target::Tab(tab), clean_name("build"), cx);
            view.poll_core_panes(window, cx);
        })
    });
    assert_eq!(title(cx), "build");
    // The program never set a title of its own.
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.set_core_name(&Target::Tab(tab), clean_name(""), cx);
            view.poll_core_panes(window, cx);
        })
    });
    assert_eq!(title(cx), "Terminal");
}
