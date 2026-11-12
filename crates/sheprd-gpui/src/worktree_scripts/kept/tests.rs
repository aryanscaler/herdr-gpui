use super::*;
use sheprd_session::{Keeper, Workspace, Worktree};

fn workspace(branch: &str, worktree: Option<(&str, bool)>) -> Workspace {
    Workspace {
        ahead_behind: None,
        branch: Some(branch.into()),
        cwd: "/repo/lane".into(),
        custom_label: false,
        id: "lane".into(),
        keeper: Keeper::App,
        label: "lane".into(),
        status: sheprd_backend::AgentStatus::Idle,
        tabs: Vec::new(),
        tokens: Vec::new(),
        worktree: worktree.map(|(key, linked)| Worktree {
            key: key.into(),
            label: "repo".into(),
            linked,
        }),
    }
}

#[test]
fn the_same_linked_checkout_keeps_the_archive() {
    let current = workspace("lane", Some(("/repo/.git", true)));
    assert_eq!(
        checkout_identity(Some(&current), "/repo/.git", Some("lane")),
        Identity::Same
    );
}

/// A failed remote Git lookup leaves the workspace without its worktree;
/// that is no proof the checkout changed, so the deletion keeps waiting.
#[test]
fn an_unknown_worktree_keeps_waiting() {
    let current = workspace("lane", None);
    assert_eq!(
        checkout_identity(Some(&current), "/repo/.git", Some("lane")),
        Identity::Unknown
    );
}

/// A workspace the session does not list yet, or one whose worktree a
/// failed lookup left unknown, is no proof of a replacement, whatever its
/// branch reads; a closed workspace is caught by the store before this.
#[test]
fn an_unlisted_workspace_or_unknown_worktree_keeps_waiting() {
    assert_eq!(
        checkout_identity(None, "/repo/.git", Some("lane")),
        Identity::Unknown
    );
    assert_eq!(
        checkout_identity(Some(&workspace("other", None)), "/repo/.git", Some("lane")),
        Identity::Unknown
    );
}

#[test]
fn another_checkout_cancels_the_archive() {
    for current in [
        workspace("other", Some(("/repo/.git", true))),
        workspace("lane", Some(("/other/.git", true))),
        workspace("lane", Some(("/repo/.git", false))),
    ] {
        assert_eq!(
            checkout_identity(Some(&current), "/repo/.git", Some("lane")),
            Identity::Changed,
            "{current:?}"
        );
    }
}
