use super::*;

#[test]
fn recreated_same_branch_checkout_does_not_inherit_the_old_runs_receipt() {
    let dir = tempfile::tempdir().unwrap();
    let site = site(dir.path(), "");
    fake_core(dir.path(), "idle");
    let cancelled = AtomicBool::new(false);
    perform(
        &site,
        &Action::Dispatch(Box::new(herdr2_request())),
        &cancelled,
    )
    .unwrap();
    let run = Store::open(&site.database)
        .unwrap()
        .runs()
        .unwrap()
        .remove(0);
    let path = checkout(dir.path());
    for args in [
        vec!["worktree", "remove", "--force", path.to_str().unwrap()],
        vec!["worktree", "add", path.to_str().unwrap(), "377-icons"],
    ] {
        assert!(
            Command::new("git")
                .arg("-C")
                .arg(&site.main_root)
                .args(args)
                .status()
                .unwrap()
                .success()
        );
    }
    std::fs::write(path.join("unrelated.txt"), "keep this work").unwrap();
    std::fs::write(
        dir.path().join("home/core-inventory"),
        r#"{"terminals":[]}"#,
    )
    .unwrap();
    let before = core_calls(dir.path()).len();
    assert!(perform(&site, &Action::Remove { run: run.id }, &cancelled).is_err());
    assert!(path.join("unrelated.txt").exists());
    assert_eq!(core_calls(dir.path()).len(), before);
}

#[test]
fn replaced_terminal_is_neither_prompted_nor_killed_or_removed() {
    let dir = tempfile::tempdir().unwrap();
    let site = site(dir.path(), "");
    fake_core(dir.path(), "idle");
    let cancelled = AtomicBool::new(false);
    perform(
        &site,
        &Action::Dispatch(Box::new(herdr2_request())),
        &cancelled,
    )
    .unwrap();
    let database = Store::open(&site.database).unwrap();
    let run = database.runs().unwrap().remove(0);
    let inventory = dir.path().join("home/core-inventory");
    let mut list: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&inventory).unwrap()).unwrap();
    list["terminals"][0]["argv"] = serde_json::json!(["sh"]);
    std::fs::write(&inventory, serde_json::to_vec(&list).unwrap()).unwrap();
    let before = core_calls(dir.path()).len();
    for action in [
        Action::Send {
            run: run.id.clone(),
            text: "do not type this".into(),
        },
        Action::Stop {
            run: run.id.clone(),
        },
        Action::Remove { run: run.id },
    ] {
        assert!(matches!(
            perform(&site, &action, &cancelled),
            Err(Error::OwnedAgent(_))
        ));
    }
    assert!(
        core_calls(dir.path())[before..]
            .iter()
            .all(|call| call.starts_with("ls "))
    );
    assert!(checkout(dir.path()).join(".git").exists());
    assert_eq!(database.runs().unwrap().len(), 1);
}
