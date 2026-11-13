use super::*;

#[test]
fn remote_requests_cannot_supply_host_workspace_terminal_or_local_caller() {
    let base =
        serde_json::json!({"version": 1, "window": 9, "operation": {"op": "mute", "muted": true}});
    assert!(Request::decode(&base.to_string()).is_ok());
    for field in [
        "host",
        "scope",
        "workspace",
        "terminal",
        "caller",
        "daemon_socket",
    ] {
        let mut forged = base.clone();
        forged[field] = "elsewhere".into();
        assert!(
            matches!(Request::decode(&forged.to_string()), Err(Error::Json(_))),
            "{field}"
        );
    }
}

#[test]
fn only_work_operations_and_current_version_are_accepted() {
    assert!(matches!(
        Request::decode(r#"{"version":2,"window":null,"operation":{"op":"note"}}"#),
        Err(Error::Version(2))
    ));
    assert!(matches!(
        Request::decode(
            r#"{"version":1,"window":null,"operation":{"op":"cloud_exec","command":"true"}}"#
        ),
        Err(Error::Json(_))
    ));
    assert!(matches!(
        Request::decode(&" ".repeat(MAX_REQUEST + 1)),
        Err(Error::Size { .. })
    ));
    assert!(matches!(
        encode_reply(&Response::done("x".repeat(MAX_REPLY))),
        Err(Error::Size { .. })
    ));
}

#[test]
fn response_namespace_and_malformed_nonces_never_become_requests() {
    let id = "0123456789abcdef0123456789abcdef";
    assert_eq!(request_id(&format!("{REQUEST_PREFIX}{id}")), Some(id));
    assert_eq!(request_id(&format!("{REPLY_PREFIX}{id}")), None);
    assert_eq!(request_id(&format!("{CLAIM_PREFIX}{id}")), None);
    for bad in ["", "../t1", "0123456789abcdef0123456789abcdef_0"] {
        assert_eq!(request_id(&format!("{REQUEST_PREFIX}{bad}")), None);
    }
}
