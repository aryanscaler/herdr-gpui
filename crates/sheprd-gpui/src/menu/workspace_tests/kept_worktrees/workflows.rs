//! A kept workspace's worktree workflows: fan-out from it, and its setup,
//! run, and archive scripts, which run in herdr-core tabs of kept
//! workspaces. Git and herdr-core are not run: the scripts are read from a
//! temporary folder and the trust question is answered in memory.

use super::*;
use crate::worktree_scripts::{Checkout, Grant, Launch, ScriptKind, Trust};

fn actions(
    view: &Entity<SheprdWindow>,
    cx: &mut VisualTestContext,
    id: &str,
) -> Vec<WorkspaceMenuAction> {
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.open_workspace_menu(id, gpui::Point::default(), window, cx);
            view.workspace_items()
                .into_iter()
                .map(|(action, _)| action)
                .collect()
        })
    })
}

#[gpui::test]
fn a_kept_checkout_offers_fan_out_and_its_scripts(cx: &mut TestAppContext) {
    let (view, cx, id) = window(cx, LINKED, Some(true));
    let actions = actions(&view, cx, &id);
    assert!(actions.contains(&WorkspaceMenuAction::FanOut));
    assert!(actions.contains(&WorkspaceMenuAction::Script(ScriptKind::Run)));
    assert!(actions.contains(&WorkspaceMenuAction::Script(ScriptKind::Setup)));
}

#[gpui::test]
fn a_kept_main_checkout_runs_scripts_but_sets_nothing_up(cx: &mut TestAppContext) {
    let (view, cx, id) = window(cx, MAIN, Some(false));
    let actions = actions(&view, cx, &id);
    assert!(actions.contains(&WorkspaceMenuAction::FanOut));
    assert!(actions.contains(&WorkspaceMenuAction::Script(ScriptKind::Run)));
    assert!(!actions.contains(&WorkspaceMenuAction::Script(ScriptKind::Setup)));
}

#[gpui::test]
fn a_kept_folder_outside_git_offers_no_workflows(cx: &mut TestAppContext) {
    let (view, cx, id) = window(cx, PLAIN, None);
    let actions = actions(&view, cx, &id);
    assert!(!actions.contains(&WorkspaceMenuAction::FanOut));
    assert!(!actions.contains(&WorkspaceMenuAction::Script(ScriptKind::Run)));
}

#[gpui::test]
fn a_fan_out_from_a_kept_checkout_branches_through_its_folder(cx: &mut TestAppContext) {
    let (view, cx, id) = window(cx, LINKED, Some(true));
    let _ = actions(&view, cx, &id);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.open_fan_out(window, cx);
            assert_eq!(view.menu.page, Some(Page::FanOut));
            let origin = view.fan_out.as_ref().unwrap().origin();
            assert_eq!(origin.checkout.as_deref(), Some(LINKED));
            assert_eq!(origin.workspace_id, id);
            assert_eq!(origin.repo_label, "app");
            assert!(view.fan_out_origin_current());
        })
    });
}

pub(super) fn kept_launch(
    view: &SheprdWindow,
    kind: ScriptKind,
    workspace: &str,
    path: &str,
) -> Launch {
    Launch {
        kind,
        endpoint: (view.selection_epoch, view.endpoints[0].generation),
        endpoint_id: view.endpoints[0].id.clone(),
        boot: String::new(),
        workspace: workspace.to_owned(),
        repo: "app".into(),
        repo_key: Path::new(MAIN).join(".git").display().to_string(),
        checkout: Some(Checkout {
            path: path.to_owned(),
            root: Some(MAIN.into()),
        }),
        force: false,
        requested: false,
    }
}

pub(super) fn write_scripts(dir: &Path, text: &str) {
    std::fs::create_dir_all(dir.join(".herdr")).unwrap();
    std::fs::write(dir.join(crate::worktree_scripts::CONFIG_PATH), text).unwrap();
}

#[gpui::test]
fn a_new_checkouts_setup_waits_for_its_kept_workspace(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().display().to_string();
    write_scripts(dir.path(), "[scripts]\nsetup = \"npm ci\"\n");
    let (view, cx, id) = window(cx, &folder, Some(true));
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            let elsewhere = kept_launch(view, ScriptKind::Setup, "", NEW);
            view.queue_kept_setup(elsewhere);
            view.poll_kept_scripts(cx);
            assert!(view.queued_kept_setup().is_some(), "no workspace yet");
            assert!(view.worktree_script.is_none());
            let here = kept_launch(view, ScriptKind::Setup, "", &folder);
            view.queue_kept_setup(here);
            view.poll_kept_scripts(cx);
            // The ready one started; the one still waiting kept its place.
            let waiting = view.queued_kept_setups();
            assert_eq!(waiting.len(), 1);
            assert_eq!(waiting[0].checkout.as_ref().unwrap().path, NEW);
        })
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            // No daemon answers for it, yet the untrusted file is asked about.
            view.poll_worktree_script(window, cx);
            assert_eq!(view.menu.page, Some(Page::WorktreeScript));
            assert!(view.worktree_script.is_some());
            assert!(view.local_error.is_none(), "{:?}", view.local_error);
            let _ = &id;
        })
    });
}

#[gpui::test]
fn a_trusted_kept_archive_runs_before_its_checkout_is_removed(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().display().to_string();
    let text = "[scripts]\narchive = \"make clean\"\n";
    write_scripts(dir.path(), text);
    let (view, cx, id) = window(cx, &folder, Some(true));
    let config = crate::worktree_scripts::Config::parse(text.as_bytes()).unwrap();
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            let launch = Launch {
                force: true,
                requested: true,
                ..kept_launch(view, ScriptKind::Archive, &id, &folder)
            };
            cx.default_global::<Trust>().grant(Grant {
                endpoint: launch.endpoint_id.clone(),
                repo_key: launch.repo_key.clone(),
                digest: config.digest.clone(),
            });
            view.start_archive_script(launch, config, cx).unwrap();
            assert!(view.worktree_script.is_none(), "it runs at once");
            assert!(view.follows_kept_archive(&id));
            assert!(view.is_kept_workspace(&id), "nothing is removed yet");
        })
    });
}

mod followed;
mod subfolder;
