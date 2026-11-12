use super::*;

#[test]
fn kept_dialog_requests_cannot_smuggle_force_or_a_different_host() {
    for op in [
        WorkOp::KeptList {},
        WorkOp::KeptOpen {},
        WorkOp::KeptDelete {},
        WorkOp::KeptCreate {
            branch: Some("feature/remote".into()),
        },
    ] {
        let encoded = serde_json::to_string(&op).unwrap();
        assert_eq!(serde_json::from_str::<WorkOp>(&encoded).unwrap(), op);
    }
    for value in [
        r#"{"op":"kept_delete","force":true}"#,
        r#"{"op":"kept_open","host":"local"}"#,
    ] {
        assert!(serde_json::from_str::<WorkOp>(value).is_err());
    }
}
