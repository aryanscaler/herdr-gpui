use super::*;
use crate::cloud::Requirement;

#[test]
fn a_required_core_download_failure_never_installs_herdr_1() {
    let home = machine_without_core_downloads();
    let result = Requirement::Core.install(on(home.path()), &|| false);
    assert!(
        matches!(result, Err(Error::Install(Failure::Core(_)))),
        "{result:?}"
    );
    assert!(!home.path().join(".local/bin/herdr").exists());
    assert!(!home.path().join(".local/bin/herdr-core").exists());
}

#[test]
fn required_core_probe_accepts_core_without_herdr_1() {
    let home = tempfile::tempdir().unwrap();
    executable(&home.path().join(".local/bin/herdr-core"));
    assert!(
        Requirement::Core
            .installed(local(home.path()), &|| false)
            .unwrap()
    );
}

#[test]
fn required_core_setup_honors_cancellation() {
    let home = machine_without_core_downloads();
    assert!(matches!(
        Requirement::Core.install(on(home.path()), &|| true),
        Err(Error::Cancelled)
    ));
    assert!(!home.path().join(".local/bin/herdr").exists());
}
