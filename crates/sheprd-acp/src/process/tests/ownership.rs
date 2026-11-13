use super::*;

#[test]
fn an_early_startup_return_reaps_only_the_agent_it_created() {
    let dir = tempfile::tempdir().unwrap();
    let spawned = spawn(
        &OsString::from("/bin/sh"),
        &["-c".into(), "exec sleep 30".into()],
        &[],
        dir.path(),
    )
    .unwrap();
    let owner = spawned.process.owned();
    assert!(spawned.process.lock().try_wait().unwrap().is_none());
    drop(owner);
    assert!(spawned.process.lock().try_wait().unwrap().is_some());
}
