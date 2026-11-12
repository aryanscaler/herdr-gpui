use super::*;

#[test]
fn remote_worktree_folder_uses_posix_separators_on_every_client() {
    let folder = Path::new("My Repo").join("fan-out-lane");
    assert_eq!(relative(&folder), ".herdr/worktrees/My Repo/fan-out-lane");
}
