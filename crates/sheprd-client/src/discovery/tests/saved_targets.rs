use super::*;

#[test]
fn saved_routes_preserve_host_session_and_installation() {
    for target in [
        ConnectTarget::Local,
        ConnectTarget::Session {
            name: "agents".into(),
            development: true,
        },
        ConnectTarget::Ssh {
            target: "user@box".into(),
            session: "agents".into(),
        },
        ConnectTarget::Wsl {
            distro: "Ubuntu".into(),
            session: "agents".into(),
        },
    ] {
        let saved = serde_json::to_vec(&target).unwrap();
        assert_eq!(
            serde_json::from_slice::<ConnectTarget>(&saved).unwrap(),
            target
        );
    }
}

#[cfg(feature = "azure")]
#[test]
fn saved_cloud_route_preserves_stable_provider_identity() {
    let target = ConnectTarget::Cloud {
        provider: CloudProvider::Azure,
        account: "subscription".into(),
        id: "stable-machine-id".into(),
        machine: "display-name".into(),
        session: "agents".into(),
    };
    let saved = serde_json::to_vec(&target).unwrap();
    assert_eq!(
        serde_json::from_slice::<ConnectTarget>(&saved).unwrap(),
        target
    );
}
