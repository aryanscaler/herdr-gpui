use super::*;
use crate::{actions, repo};
use sheprd_client::{ConnectTarget, RemoteHost};

#[test]
fn development_session_route_survives_database_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = store::database(&dir);
    let target = ConnectTarget::Session {
        name: "agents".into(),
        development: true,
    };
    let recorded = HerdrSession {
        target: Some(target.clone()),
        ..session("dev")
    };
    Store::open(&path)
        .unwrap()
        .save_run(&run("dev", Owner::Sheprd), Some(&recorded))
        .unwrap();
    assert_eq!(
        actions::target_of(&Store::open(&path).unwrap().sessions().unwrap()[0]),
        target
    );
}

#[test]
fn wsl_and_ssh_sessions_keep_distinct_durable_targets() {
    let dir = tempfile::tempdir().unwrap();
    let path = store::database(&dir);
    let mut database = Store::open(&path).unwrap();
    for (id, host) in [
        ("wsl", RemoteHost::Wsl("Ubuntu".into())),
        ("ssh", RemoteHost::Ssh("Ubuntu".into())),
    ] {
        let target = host.target("work");
        let (host, name) = actions::place(&target);
        let recorded = HerdrSession {
            target: Some(target),
            host,
            session: name,
            pane: Pane::Pending,
            ..session(id)
        };
        database
            .save_run(&run(id, Owner::Sheprd), Some(&recorded))
            .unwrap();
    }
    drop(database);
    let database = Store::open(&path).unwrap();
    for session in database.sessions().unwrap() {
        let expected = if session.run_id == "wsl" {
            RemoteHost::Wsl("Ubuntu".into())
        } else {
            RemoteHost::Ssh("Ubuntu".into())
        };
        assert_eq!(actions::target_of(&session), expected.target("work"));
        assert_eq!(session.pane, Pane::Pending);
    }
}

#[test]
fn schema_three_ssh_rows_migrate_without_becoming_wsl_or_local() {
    let dir = tempfile::tempdir().unwrap();
    let path = store::database(&dir);
    let mut database = Store::open(&path).unwrap();
    let expected = HerdrSession {
        host: Some("box".into()),
        ..session("r1")
    };
    database
        .save_run(&run("r1", Owner::Sheprd), Some(&expected))
        .unwrap();
    drop(database);
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute_batch(
            "ALTER TABLE herdr_sessions DROP COLUMN target_json;
         ALTER TABLE herdr_sessions DROP COLUMN pending;
         PRAGMA user_version = 3;",
        )
        .unwrap();
    assert_eq!(Store::open(&path).unwrap().sessions().unwrap(), [expected]);
}

#[test]
fn repository_identity_includes_wsl_distribution_without_changing_legacy_ids() {
    let output = "common\t/repo/.git\nmain\t/repo\n";
    let local = repo::parse(&ConnectTarget::Local, output)
        .unwrap()
        .repository;
    let ssh = repo::parse(&RemoteHost::Ssh("Ubuntu".into()).target("work"), output)
        .unwrap()
        .repository;
    let wsl = repo::parse(&RemoteHost::Wsl("Ubuntu".into()).target("work"), output)
        .unwrap()
        .repository;
    let other = repo::parse(&RemoteHost::Wsl("Debian".into()).target("work"), output)
        .unwrap()
        .repository;
    assert_ne!(wsl.id(), local.id());
    assert_ne!(wsl.id(), ssh.id());
    assert_ne!(wsl.id(), other.id());
    assert_eq!(
        local.id(),
        uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, b"/repo/.git")
    );
    assert_eq!(
        ssh.id(),
        uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, b"ssh://Ubuntu/repo/.git")
    );
}
