use super::*;

fn core(dir: &Path, endpoint: &EndpointId, pane: &str, socket: &Path) -> Option<Keeper> {
    Keeper::core(
        Some(dir.to_owned()),
        Some(endpoint),
        &builtin(Vec::new()),
        true,
        &abs("/work"),
        |name| match name {
            "HC_TERMINAL_ID" => Some(pane.into()),
            "HC_TERMINAL_SOCKET" => Some(socket.as_os_str().to_owned()),
            // The caller's socket is deliberately unrelated.
            "HC_SOCKET" => Some(abs("/wrong/core.sock").into_os_string()),
            _ => None,
        },
    )
}

#[test]
fn core_records_pin_the_terminal_host_and_exact_session_apart_from_herdr_one() {
    let dir = tempfile::tempdir().unwrap();
    let endpoint = ENDPOINT.parse().unwrap();
    let socket = abs("/state/named-session/core.sock");
    let keeper = core(dir.path(), &endpoint, "t9", &socket).unwrap();
    keeper.opened("first");
    keeper.opened("latest");
    assert!(restore::list(dir.path(), None).records.is_empty());
    let records = restore::list(&dir.path().join("core"), None).records;
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].0.session, "latest");
    assert_eq!(records[0].0.socket, socket);
    assert_eq!(records[0].0.pane, "t9");
    assert_eq!(records[0].0.cwd, abs("/work"));
    assert!(records[0].0.adapter);
    // Losing the process leaves continuity information. Only explicit normal
    // shutdown removes it, not dropping a client/keeper handle.
    drop(keeper);
    assert!(restore::record_path(&dir.path().join("core"), &endpoint).is_file());
    core(dir.path(), &endpoint, "t9", &socket).unwrap().ended();
    assert!(
        restore::list(&dir.path().join("core"), None)
            .records
            .is_empty()
    );
}

#[test]
fn core_identity_never_falls_back_to_an_unrelated_callers_socket() {
    let dir = tempfile::tempdir().unwrap();
    let endpoint = ENDPOINT.parse().unwrap();
    for pane in ["", "box/t9"] {
        assert!(core(dir.path(), &endpoint, pane, &abs("/core.sock")).is_none());
    }
    assert!(core(dir.path(), &endpoint, "t9", Path::new("relative.sock")).is_none());
    assert!(
        Keeper::core(
            Some(dir.path().to_owned()),
            Some(&endpoint),
            &builtin(Vec::new()),
            false,
            &abs("/work"),
            |name| match name {
                "HC_TERMINAL_ID" => Some("t9".into()),
                "HC_SOCKET" => Some(abs("/wrong/core.sock").into_os_string()),
                _ => None,
            },
        )
        .is_none()
    );
}
