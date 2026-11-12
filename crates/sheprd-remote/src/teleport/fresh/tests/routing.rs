use super::*;

#[test]
fn a_missing_kept_repository_is_cloned_without_a_daemon_snapshot() {
    let fixture = fixture();
    let home = tempfile::tempdir().unwrap();
    let mut destination_host = host();
    destination_host.env = vec![("HOME".into(), home.path().display().to_string())];
    let origin = Origin {
        place: Place {
            endpoint_id: "source".into(),
            label: "Source".into(),
            host: host(),
        },
        workspace_id: "kept-source".into(),
        repo_key: fixture.key.clone(),
        repo_label: "project".into(),
    };
    let destination = HostRepositories {
        place: Place {
            endpoint_id: "destination".into(),
            label: "Destination".into(),
            host: destination_host,
        },
        repositories: None,
        retired: Vec::new(),
        lands_in: sheprd_session::Keeper::App,
    };
    let mut steps = Vec::new();
    let cancelled = AtomicBool::new(false);
    let prepared = prepare(
        &origin,
        &destination,
        &fixture.feature,
        &mut |step| steps.push(step),
        &cancelled,
    )
    .unwrap();
    let work::Repository::Herdr2 { checkout } = prepared.work_repository() else {
        panic!("missing repositories must not fall back to Herdr 1");
    };
    assert!(Path::new(checkout).starts_with(home.path()));
    assert_eq!(
        git(Path::new(checkout), &["cat-file", "-t", &fixture.feature]),
        "commit"
    );
    assert!(steps.contains(&Step::Clone));
    assert!(!steps.contains(&Step::CreateWorktree));
    prepared.finish(&cancelled);
}

#[test]
fn preparing_a_kept_destination_ships_commit_and_returns_git_routing() {
    let fixture = fixture();
    let origin = Origin {
        place: Place {
            endpoint_id: "source".into(),
            label: "Source".into(),
            host: host(),
        },
        workspace_id: "kept-source".into(),
        repo_key: fixture.key.clone(),
        repo_label: "project".into(),
    };
    let destination = HostRepositories {
        place: Place {
            endpoint_id: "destination".into(),
            label: "Destination".into(),
            host: host(),
        },
        repositories: Some(vec![Repository {
            key: fixture.destination.clone(),
            label: "project".into(),
            workspace_id: "kept-destination".into(),
        }]),
        retired: Vec::new(),
        lands_in: sheprd_session::Keeper::App,
    };
    let cancelled = AtomicBool::new(false);
    let prepared = prepare(
        &origin,
        &destination,
        &fixture.feature,
        &mut |_| {},
        &cancelled,
    )
    .unwrap();
    let work::Repository::Herdr2 { checkout } = prepared.work_repository() else {
        panic!("kept IDs must never reach Herdr 1");
    };
    assert!(prepared.repository.workspace_id.is_empty());
    assert_eq!(
        git(Path::new(checkout), &["cat-file", "-t", &fixture.feature]),
        "commit"
    );
    assert!(prepared.reference.is_some());
    prepared.finish(&cancelled);
    assert_eq!(
        git(
            Path::new(&fixture.destination),
            &["for-each-ref", "refs/herdr-teleport"]
        ),
        ""
    );
}
