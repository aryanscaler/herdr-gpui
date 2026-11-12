use super::*;

#[test]
fn worktree_dialog_commands_keep_global_targeting_and_reject_extra_arguments() {
    for (verb, expected) in [
        ("list", control::WorktreeCommand::List),
        ("open", control::WorktreeCommand::Open),
        ("delete", control::WorktreeCommand::Delete),
    ] {
        let options = LaunchOptions::parse(["worktree", verb, "--json", "--window", "7"]).unwrap();
        let LaunchMode::Control(invocation) = options.mode else {
            panic!("control command")
        };
        assert_eq!(
            invocation.command,
            control::ControlCommand::Worktree(expected)
        );
        assert!(invocation.json);
        assert_eq!(invocation.window, Some(7));
        assert!(LaunchOptions::parse(["worktree", verb, "extra"]).is_err());
    }
    let options = LaunchOptions::parse(["worktree", "create", "feature/remote"]).unwrap();
    let LaunchMode::Control(invocation) = options.mode else {
        panic!("control command")
    };
    assert_eq!(
        invocation.command,
        control::ControlCommand::Worktree(control::WorktreeCommand::Create {
            branch: Some("feature/remote".into())
        })
    );
}
