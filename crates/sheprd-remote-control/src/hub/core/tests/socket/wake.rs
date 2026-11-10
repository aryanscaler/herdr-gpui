//! Starting this machine's core for a phone, with a stand-in `herdr-core`.

use super::*;
use crate::core_target::CoreTarget;
use std::os::unix::fs::PermissionsExt as _;

/// A source that joins `socket`, unjoined yet.
fn unjoined(socket: &std::path::Path) -> Source {
    Source {
        connected: AtomicBool::new(false),
        endpoint: CoreTarget::Endpoint(Endpoint {
            socket: socket.to_owned(),
            session: "phone".into(),
        }),
        holds: AtomicU64::new(0),
        own: Mutex::default(),
        poller: Mutex::default(),
        scope: None,
        state: Mutex::default(),
        tabs: None,
    }
}

/// A `herdr-core` that checks it was asked to start the source's session,
/// then exits with `code`.
fn program(dir: &std::path::Path, socket: &std::path::Path, code: u8) -> std::path::PathBuf {
    let path = dir.join("herdr-core");
    std::fs::write(
        &path,
        format!(
            "#!/bin/sh\n[ \"$*\" = 'server start --json' ] || exit 7\n\
             [ \"$HC_SOCKET\" = '{}' ] || exit 8\n[ \"$HC_SESSION\" = phone ] || exit 9\n\
             echo nope >&2\nexit {code}\n",
            socket.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

/// A core that answers one hello and then stays connected.
fn listening(socket: &std::path::Path) -> thread::JoinHandle<()> {
    let listener = UnixListener::bind(socket).unwrap();
    thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        let mut hello = String::new();
        BufReader::new(&mut socket).read_line(&mut hello).unwrap();
        let hello: Value = serde_json::from_str(&hello).unwrap();
        let response = json!({"jsonrpc": "2.0", "id": hello["id"], "result": {
            "generation": 1, "version": "0.1.18", "framed": true,
            "core": {"id": "woken", "name": "test"},
            "methods": Method::ALL.iter().map(|method| method.as_str()).collect::<Vec<_>>()}});
        writeln!(socket, "{response}").unwrap();
        let _ = socket.read(&mut [0; 1]);
    })
}

#[test]
fn a_started_core_is_joined_for_new_work_at_once() {
    let dir = tempfile::Builder::new()
        .prefix("rw")
        .tempdir_in("/tmp")
        .unwrap();
    let socket = dir.path().join("core.sock");
    let server = listening(&socket);
    let source = unjoined(&socket);
    source.wake(&program(dir.path(), &socket, 0)).unwrap();
    assert!(source.connected(), "new work goes to it before a poll");
    drop(source);
    server.join().unwrap();
}

#[test]
fn a_core_that_fails_to_start_leaves_the_source_unjoined() {
    let dir = tempfile::Builder::new()
        .prefix("rw")
        .tempdir_in("/tmp")
        .unwrap();
    let socket = dir.path().join("core.sock");
    let source = unjoined(&socket);
    let error = source.wake(&program(dir.path(), &socket, 1)).unwrap_err();
    assert!(
        matches!(
            &error,
            HubError::Core(Error::Client(sheprd_core_client::Error::StartFailed { stderr, .. }))
                if stderr.trim() == "nope"
        ),
        "{error:?}"
    );
    assert!(!source.connected());
}
