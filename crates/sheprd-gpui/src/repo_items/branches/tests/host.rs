use super::*;
use std::{os::unix::fs::PermissionsExt, sync::atomic::AtomicBool};

#[test]
fn branches_come_from_the_supplied_host_and_require_its_repository_identity() {
    let home = tempfile::tempdir().unwrap();
    let bin = home.path().join(".local/bin");
    std::fs::create_dir_all(&bin).unwrap();
    let git = bin.join("git");
    std::fs::write(
        &git,
        r#"#!/bin/sh
case "$*" in
*rev-parse*) printf '/remote/repo/.git\n/remote/repo/.git\n' ;;
*symbolic-ref*) printf 'main\n' ;;
*for-each-ref*) printf 'refs/heads/remote-only\000\n' ;;
*) exit 1 ;;
esac
"#,
    )
    .unwrap();
    std::fs::set_permissions(&git, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut host =
        sheprd_remote::teleport::Host::new(&sheprd_client::ConnectTarget::Local).unwrap();
    host.env
        .push(("HOME".into(), home.path().to_string_lossy().into_owned()));
    let mut input = Input {
        checkout: Some("/remote/repo".into()),
        repo_key: Some("/remote/repo/.git".into()),
        branch: "main".into(),
    };
    assert_eq!(
        list_from_host(&host, &input, &AtomicBool::new(false)).unwrap(),
        vec![Branch {
            name: "remote-only".into()
        }]
    );
    input.repo_key = Some("/different/repo/.git".into());
    assert!(matches!(
        list_from_host(&host, &input, &AtomicBool::new(false)),
        Err(crate::Error::StaleWorkspace)
    ));
}

#[test]
fn an_unavailable_wsl_host_never_lists_the_clients_repository() {
    let target = sheprd_client::ConnectTarget::Wsl {
        distro: "Ubuntu".into(),
        session: "work".into(),
    };
    let input = Input {
        checkout: Some(env!("CARGO_MANIFEST_DIR").into()),
        repo_key: Some("/remote/.git".into()),
        branch: "main".into(),
    };
    assert!(list_on(&target, &input, &AtomicBool::new(false)).is_err());
}
