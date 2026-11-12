//! CLI adapters must reach app-kept work without a Herdr 1 snapshot.
use super::*;
use crate::{
    browser::{Location, Store},
    control::protocol::work::{Reply, Script},
    core_terminals::panes::Panes,
    notification_mutes::{Mutes, Scope},
};
#[cfg(any(target_os = "linux", target_os = "macos"))]
use crate::{control::protocol::work::FanOutRequest, menu::Page};
use sheprd_backend::TerminalId;

fn kept(
    view: &mut SheprdWindow,
    window: &mut gpui::Window,
    cx: &mut gpui::Context<SheprdWindow>,
) -> Caller {
    view.endpoints[0].connection.target = sheprd_client::ConnectTarget::Local;
    view.live.snapshot = None;
    view.live.missing_installation = true;
    let folder = if cfg!(windows) {
        "C:/src/tools"
    } else {
        "/src/tools"
    };
    view.core_terminals.seed_checkout_for_test(
        folder,
        Some(sheprd_vcs::git::Identity {
            branch: Some("feature".into()),
            key: format!("{folder}/.git"),
            label: "tools".into(),
            linked: true,
        }),
    );
    let terminal = TerminalId::new("t-kept").unwrap();
    let location = Location::core_terminal(Panes::single(terminal.clone()), false);
    let id = view
        .open_kept_workspace(
            Some("tools"),
            std::path::Path::new(folder),
            location,
            window,
            cx,
        )
        .unwrap();
    let scope = crate::browser::scope(&view.endpoints[0]);
    let tab = cx
        .global::<Store>()
        .in_workspace(&scope, &id)
        .next()
        .unwrap()
        .id;
    view.core_terminals
        .show_for_test(tab, Panes::single(terminal), false, Vec::new());
    view.core_terminals.place_for_test(tab, scope, &id);
    Caller {
        terminal_id: Some("t-kept".into()),
        ..caller(None)
    }
}

#[gpui::test]
fn kept_mute_uses_the_callers_scope_and_preserves_the_host_mute(cx: &mut gpui::TestAppContext) {
    let (view, cx) = cx.add_window_view(fixture_window);
    cx.update(|window, cx| view.update(cx, |view, cx| {
        let here = kept(view, window, cx);
        let scope = crate::browser::scope(&view.endpoints[0]);
        let id = find(view, &here).ok().unwrap().workspace.id;
        for muted in [true, true, false] {
            let response = mute::set(view, &here, muted, cx);
            let Response::Work { reply: Reply::Mute { mute } } = response else {
                panic!("{response:?}");
            };
            assert_eq!(mute.muted, muted);
            assert_eq!(mute.workspace, "tools");
            assert_eq!(Mutes::of(cx).unwrap().kept(scope.as_str(), Scope::Workspace, &id), muted);
            assert!(!Mutes::of(cx).unwrap().kept("ssh:elsewhere", Scope::Workspace, &id));
        }
        Mutes::update(cx, |mutes| mutes.set_host(crate::core_terminals::Host::Local.endpoint(), true));
        let response = mute::set(view, &caller(None), false, cx);
        assert!(matches!(response, Response::Work { reply: Reply::Mute { mute } } if !mute.muted && mute.host_muted));
        let foreign = Caller { daemon_socket: Some("/unrelated/daemon.sock".into()), ..caller(None) };
        assert!(error_code(&mute::set(view, &foreign, true, cx)).is_some());
        assert!(!Mutes::of(cx).unwrap().kept(scope.as_str(), Scope::Workspace, &id));
    }));
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[gpui::test]
fn kept_fan_out_only_composes_and_refuses_a_different_selected_host(cx: &mut gpui::TestAppContext) {
    let (view, cx) = cx.add_window_view(fixture_window);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            let here = kept(view, window, cx);
            let request = || FanOutRequest {
                prompt: "fix the test".into(),
                agents: vec![("claude".into(), 1)],
            };
            let response = flows::fan_out(view, &here, request(), window, cx);
            assert!(matches!(response, Response::Done { .. }), "{response:?}");
            assert_eq!(view.menu.page, Some(Page::FanOut));
            assert!(
                view.fan_out.as_ref().unwrap().composing(),
                "the user still confirms launch"
            );
            view.dismiss_menu(window, cx);
            view.fan_out = None;
            view.endpoints.push(crate::endpoint::Endpoint::new(
                "ssh:box".into(),
                "Box".into(),
                sheprd_client::ConnectTarget::Ssh {
                    target: "nobody@invalid.invalid".into(),
                    session: "default".into(),
                },
                true,
            ));
            view.selected_endpoint = 1;
            let response = flows::fan_out(view, &here, request(), window, cx);
            assert_eq!(
                error_code(&response),
                Some(ErrorCode::InvalidRequest),
                "{response:?}"
            );
            assert!(view.menu.page.is_none());
            assert!(view.fan_out.is_none());
        })
    });
}

#[gpui::test]
fn a_kept_caller_cannot_mute_in_a_window_that_does_not_hold_it(cx: &mut gpui::TestAppContext) {
    let (own, own_cx) = cx.add_window_view(fixture_window);
    let here = own_cx.update(|window, cx| own.update(cx, |view, cx| kept(view, window, cx)));
    let (other, other_cx) = cx.add_window_view(fixture_window);
    let id = other_cx.update(|window, _| window.window_handle().window_id().as_u64());
    let wrong_window = Caller {
        window: Some(id),
        ..here.clone()
    };
    cx.update(|cx| {
        let response = in_window(&wrong_window, cx, |view, _, cx| {
            mute::set(view, &wrong_window, true, cx)
        });
        assert!(error_code(&response).is_some(), "{response:?}");
    });
    let _ = other;
    own.read_with(cx, |view, cx| {
        assert!(matches!(mute::status(view, &here, cx), Response::Work { reply: Reply::Mute { mute } } if !mute.muted));
    });
}

#[gpui::test]
fn scripts_and_worktree_dialogs_refuse_a_callers_unselected_host(cx: &mut gpui::TestAppContext) {
    let (view, cx) = cx.add_window_view(fixture_window);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            let here = kept(view, window, cx);
            view.endpoints.push(crate::endpoint::Endpoint::new(
                "ssh:box".into(),
                "Box".into(),
                sheprd_client::ConnectTarget::Ssh {
                    target: "nobody@invalid.invalid".into(),
                    session: "default".into(),
                },
                true,
            ));
            view.selected_endpoint = 1;
            for kind in [Script::Setup, Script::Run] {
                let response = flows::script(view, &here, kind, cx);
                assert_eq!(
                    error_code(&response),
                    Some(ErrorCode::InvalidRequest),
                    "{response:?}"
                );
                assert!(view.worktree_script.is_none());
            }
            for dialog in [
                worktrees::Dialog::Create(None),
                worktrees::Dialog::Open,
                worktrees::Dialog::Delete,
            ] {
                let response = worktrees::dialog(view, &here, dialog, window, cx);
                assert_eq!(
                    error_code(&response),
                    Some(ErrorCode::InvalidRequest),
                    "{response:?}"
                );
                assert!(view.menu.page.is_none());
            }
        })
    });
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[gpui::test]
fn kept_worktree_commands_open_confirmation_with_a_valid_branch_selection(
    cx: &mut gpui::TestAppContext,
) {
    let (view, cx) = cx.add_window_view(fixture_window);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            let here = kept(view, window, cx);
            let branch = "x";
            let response = worktrees::dialog(
                view,
                &here,
                worktrees::Dialog::Create(Some(branch.into())),
                window,
                cx,
            );
            assert!(matches!(response, Response::Done { .. }), "{response:?}");
            let input = view.menu.input.as_ref().unwrap();
            assert_eq!(input.text, branch);
            assert!(input.selection.end <= branch.len());
            assert_eq!(
                view.menu.page,
                Some(Page::Dialog(crate::menu::WorkspaceAction::NewWorktree)),
                "the user confirms creation"
            );
            view.dismiss_menu(window, cx);
            let response = worktrees::dialog(view, &here, worktrees::Dialog::Open, window, cx);
            assert!(matches!(response, Response::Done { .. }), "{response:?}");
            assert!(view.worktree_listing());
            assert_eq!(
                view.menu.page,
                Some(Page::Dialog(crate::menu::WorkspaceAction::NewWorktree)),
                "the user picks a checkout"
            );
        })
    });
}
