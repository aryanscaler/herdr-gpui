use super::*;

#[cfg(unix)]
mod previous_agent;

const ID: &str = "0123456789abcdef0123456789abcdef";

/// A lock is released once every copy of its descriptor closes. A process
/// another test thread forks holds a copy until it execs (the descriptor is
/// close-on-exec), so the release can trail `drop` briefly. Bounded.
fn relock(dir: &Path, id: &EndpointId) -> File {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        match lock_in(dir, id) {
            Ok(file) => return file,
            Err(error) if std::time::Instant::now() >= deadline => panic!("{error:#}"),
            Err(_) => std::thread::sleep(std::time::Duration::from_millis(10)),
        }
    }
}

#[test]
fn an_endpoint_has_one_owner_until_it_exits_not_until_a_timeout() {
    let dir = tempfile::tempdir().unwrap();
    let id = ID.parse().unwrap();
    let first = lock_in(dir.path(), &id).unwrap();
    assert!(lock_in(dir.path(), &id).is_err());
    drop(first);
    let next = relock(dir.path(), &id);
    assert!(lock_in(dir.path(), &id).is_err());
    drop(next);
    assert!(dir.path().join(format!("{ID}.lock")).exists());
}

#[test]
fn recovery_requires_every_part_of_the_original_context() {
    let cwd = std::env::current_dir().unwrap();
    let record = restore::Record {
        adapter: false,
        agent: sheprd_chat_protocol::Agent::Codex,
        cwd: cwd.clone(),
        endpoint: ID.parse().unwrap(),
        pane: "t9".to_owned(),
        session: "exact-session".to_owned(),
        socket: cwd.join("core.sock"),
    };
    let env = std::collections::BTreeMap::from([
        (ENDPOINT_ENV, std::ffi::OsString::from(ID)),
        ("HC_TERMINAL_ID", "t9".into()),
        ("HC_TERMINAL_SOCKET", record.socket.clone().into_os_string()),
    ]);
    assert!(matches_context(&record, &record.endpoint, &cwd, |name| env
        .get(name)
        .cloned()));
    for key in env.keys() {
        let mut wrong = env.clone();
        wrong.insert(key, "other".into());
        assert!(!matches_context(&record, &record.endpoint, &cwd, |name| {
            wrong.get(name).cloned()
        }));
        wrong.remove(key);
        assert!(!matches_context(&record, &record.endpoint, &cwd, |name| {
            wrong.get(name).cloned()
        }));
    }
    assert!(!matches_context(
        &record,
        &record.endpoint,
        &cwd.join("other"),
        |name| env.get(name).cloned()
    ));
    let other = "abcdef0123456789abcdef0123456789".parse().unwrap();
    assert!(!matches_context(&record, &other, &cwd, |name| env
        .get(name)
        .cloned()));
}
