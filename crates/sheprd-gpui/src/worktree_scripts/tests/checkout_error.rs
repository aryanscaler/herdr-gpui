use super::*;

/// The checkout is found through the kept workspace's host, so a folder Git
/// cannot enter fails as that host's Git step, and still reads localized.
#[test]
fn a_failed_kept_checkout_lookup_is_a_localized_host_git_failure() {
    let temp = tempfile::tempdir().unwrap();
    let missing = temp.path().join("missing");
    // The lookup runs Git on the workspace's host, here this computer.
    let error = super::super::kept_script_checkout(
        &sheprd_client::ConnectTarget::Local,
        missing.to_str().unwrap(),
        &AtomicBool::new(false),
    )
    .unwrap_err();
    let crate::Error::Remote(sheprd_remote::Error::Work(sheprd_remote::work::Error::Script {
        step: sheprd_remote::work::Step::List,
        ..
    })) = &error
    else {
        panic!("expected a host Git checkout lookup failure, got {error:?}");
    };
    crate::i18n::errors::tests::assert_localized(&[error]);
}
