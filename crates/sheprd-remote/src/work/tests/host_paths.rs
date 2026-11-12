use super::*;

#[test]
fn custom_herdr_socket_keeps_local_core_files_without_authorizing_herdr_commands() {
    let target = ConnectTarget::Socket("/forwarded/herdr.sock".into());
    assert!(Host::new(&target).is_err());
    let host = host_for_kept(&target).unwrap();
    assert!(host.is_local());
    assert_eq!(
        host.identity(),
        Host::new(&ConnectTarget::Local).unwrap().identity()
    );
}

#[test]
fn opening_checks_the_repository_on_the_supplied_host() {
    let fixture = Fixture::new();
    let other = Fixture::new();
    let host = fixture.host();
    let new = NewWorktree {
        branch: "feature",
        base: "HEAD",
        label: None,
    };
    let tree = create(&host, &fixture.herdr2(), &new, &never()).unwrap();
    let checkout = identity(&host, &tree.path, &never()).unwrap();
    assert!(checkout.linked);
    assert_eq!(checkout.branch.as_deref(), Some("feature"));
    assert_eq!(
        checkout.key,
        identity(&host, fixture.repo.to_str().unwrap(), &never())
            .unwrap()
            .key
    );
    assert_eq!(
        fs::canonicalize(
            open(&host, fixture.repo.to_str().unwrap(), &tree.path, &never()).unwrap()
        )
        .unwrap(),
        fs::canonicalize(&tree.path).unwrap()
    );
    assert!(open(&host, other.repo.to_str().unwrap(), &tree.path, &never()).is_err());
    assert!(Path::new(&tree.path).is_dir());
}

#[test]
fn cancelled_work_never_changes_the_checkout() {
    let fixture = Fixture::new();
    let host = fixture.host();
    let cancelled = AtomicBool::new(true);
    let new = NewWorktree {
        branch: "feature",
        base: "HEAD",
        label: None,
    };
    assert!(matches!(
        create(&host, &fixture.herdr2(), &new, &cancelled),
        Err(Error::Cancelled)
    ));
    assert_eq!(git(&fixture.repo, &["branch", "--list", "feature"]), "");
    assert!(matches!(
        remove(&host, fixture.repo.to_str().unwrap(), true, &cancelled),
        Err(Error::Cancelled)
    ));
    assert!(fixture.repo.join("a.txt").is_file());
}

#[test]
fn main_checkout_and_relative_paths_are_never_removed() {
    let fixture = Fixture::new();
    let host = fixture.host();
    assert!(remove(&host, fixture.repo.to_str().unwrap(), true, &never()).is_err());
    for path in [".", "", "../repo", "/repo\nother"] {
        assert!(matches!(
            remove(&host, path, true, &never()),
            Err(Error::Output { step: Step::Remove })
        ));
    }
    assert!(fixture.repo.join("a.txt").is_file());
}

#[test]
fn a_failed_status_query_cannot_authorize_removal() {
    let fixture = Fixture::new();
    let host = fixture.host();
    let new = NewWorktree {
        branch: "feature",
        base: "HEAD",
        label: None,
    };
    let tree = create(&host, &fixture.herdr2(), &new, &never()).unwrap();
    let real_git = String::from_utf8(
        Command::new("/bin/sh")
            .args(["-c", "command -v git"])
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    fixture.program(
        "git",
        &format!(
            "case \"$*\" in *'status --porcelain'*) exit 42;; esac\nexec {} \"$@\"",
            shell_quote(real_git.trim())
        ),
    );
    assert!(matches!(
        remove(&host, &tree.path, false, &never()),
        Err(Error::Script {
            step: Step::Remove,
            ..
        })
    ));
    assert!(Path::new(&tree.path).is_dir());
    assert!(
        !fixture
            .calls()
            .iter()
            .any(|call| call.contains("worktree remove"))
    );
}

#[test]
fn wsl_hosts_have_distinct_identity_and_never_fall_back_to_local_git() {
    let fixture = Fixture::new();
    let host = Host::new(&ConnectTarget::Wsl {
        distro: "Ubuntu".into(),
        session: "default".into(),
    })
    .unwrap();
    let other = Host::new(&ConnectTarget::Wsl {
        distro: "Debian".into(),
        session: "default".into(),
    })
    .unwrap();
    assert!(!host.is_local());
    assert_ne!(host.identity(), other.identity());
    assert_ne!(host.identity(), fixture.host().identity());
    assert!(remove(&host, fixture.repo.to_str().unwrap(), true, &never()).is_err());
    assert!(fixture.repo.join("a.txt").is_file());
}
