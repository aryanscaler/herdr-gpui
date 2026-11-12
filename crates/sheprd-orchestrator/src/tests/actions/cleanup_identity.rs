use super::*;

#[test]
fn remove_refuses_a_reused_workspace_id_with_a_different_checkout() {
    let dir = tempfile::tempdir().unwrap();
    let site = site(dir.path(), "");
    let cancelled = AtomicBool::new(false);
    perform(&site, &Action::Dispatch(Box::new(request())), &cancelled).unwrap();
    let id = Store::open(&site.database)
        .unwrap()
        .runs()
        .unwrap()
        .remove(0)
        .id;
    let unrelated = snapshot(&[("w9", true)]).replace("/tmp/wt/377-icons", "/unrelated");
    std::fs::write(dir.path().join("home/snapshot"), unrelated).unwrap();
    assert!(matches!(
        perform(&site, &Action::Remove { run: id }, &cancelled),
        Err(Error::InvalidCheckout)
    ));
    assert!(
        !calls(dir.path())
            .iter()
            .any(|call| call.starts_with("worktree remove"))
    );
    assert!(
        !Store::open(&site.database)
            .unwrap()
            .runs()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn remove_keeps_a_checkout_whose_legacy_record_has_no_destination() {
    let dir = tempfile::tempdir().unwrap();
    let site = site(dir.path(), "");
    let mut database = Store::open(&site.database).unwrap();
    database
        .save_run(&run("legacy", Owner::Sheprd), None)
        .unwrap();
    let result = perform(
        &site,
        &Action::Remove {
            run: "legacy".into(),
        },
        &AtomicBool::new(false),
    );
    assert!(matches!(result, Err(Error::NothingToOpen)));
    assert!(!database.runs().unwrap().is_empty());
    assert!(calls(dir.path()).is_empty());
}
