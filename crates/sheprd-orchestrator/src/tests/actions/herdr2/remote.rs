//! Separate fake homes prove placement and controls do not use the source host.
use super::*;
use crate::actions::Elsewhere;
use sheprd_remote::teleport::{FreshOrigin, HostRepositories, Place, job::Repository};

#[test]
fn cross_host_core_dispatch_preserves_destination_and_prompt_after_reopening() {
    let source = tempfile::tempdir().unwrap();
    let destination = tempfile::tempdir().unwrap();
    let source_site = site(source.path(), "");
    let mut destination_site = site(destination.path(), "");
    fake_core(destination.path(), "idle");
    let target = ConnectTarget::Ssh {
        target: "build-box".into(),
        session: "agents".into(),
    };
    destination_site.target = target.clone();
    // Both hosts expose an existing repository; its label identifies it
    // without a Herdr 1 inventory or workspace.
    let place = |host: Host, id: &str| Place {
        host,
        endpoint_id: id.into(),
        label: id.into(),
    };
    let mut request = herdr2_request();
    request.elsewhere = Some(Elsewhere {
        origin: FreshOrigin {
            place: place(source_site.host.clone(), "source"),
            workspace_id: "kept-source".into(),
            repo_key: format!("{}/.git", source_site.main_root),
            repo_label: "app".into(),
        },
        destination: HostRepositories {
            place: place(destination_site.host.clone(), "destination"),
            repositories: Some(vec![Repository {
                key: format!("{}/.git", destination_site.main_root),
                label: "app".into(),
                workspace_id: String::new(),
            }]),
            retired: Vec::new(),
            lands_in: sheprd_session::Keeper::App,
        },
        target: target.clone(),
    });
    perform(
        &source_site,
        &Action::Dispatch(Box::new(request)),
        &AtomicBool::new(false),
    )
    .unwrap();
    assert!(calls(source.path()).is_empty());
    assert!(calls(destination.path()).is_empty());
    assert!(core_calls(source.path()).is_empty());
    assert!(checkout(destination.path()).join(".git").exists());
    assert!(!checkout(source.path()).exists());
    assert_eq!(
        core_calls(destination.path())
            .iter()
            .filter(|call| call.contains("Fix the icons"))
            .count(),
        1
    );

    // Reopen the durable database as a new worker would after restarting.
    let store = Store::open(&source_site.database).unwrap();
    let run = store.runs().unwrap().remove(0);
    let session = store.sessions().unwrap().remove(0);
    assert_eq!(crate::actions::target_of(&session), target);
    assert_eq!(run.model.as_deref(), Some("opus"));
    assert_eq!(
        run.workspace.as_ref().unwrap().path.as_deref(),
        Some(checkout(destination.path()).as_path())
    );
    drop(store);
    // The destination worker uses its isolated fake transport, but the
    // original database, exactly as persisted-run routing would select it.
    destination_site.database = source_site.database.clone();
    perform(
        &destination_site,
        &Action::Send {
            run: run.id.clone(),
            text: "Continue".into(),
        },
        &AtomicBool::new(false),
    )
    .unwrap();
    perform(
        &destination_site,
        &Action::Stop {
            run: run.id.clone(),
        },
        &AtomicBool::new(false),
    )
    .unwrap();
    perform(
        &destination_site,
        &Action::Remove { run: run.id },
        &AtomicBool::new(false),
    )
    .unwrap();
    assert!(!checkout(destination.path()).exists());
    assert!(Path::new(&source_site.main_root).join(".git").exists());
    assert!(Path::new(&destination_site.main_root).join(".git").exists());
    assert!(core_calls(source.path()).is_empty());
}
