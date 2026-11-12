use super::*;
use sheprd_client::ConnectTarget;

#[core::prelude::v1::test]
fn editing_a_saved_host_drops_its_installed_agent_cache_and_inflight_probe() {
    let first = ConnectTarget::Ssh {
        target: "first".into(),
        session: "default".into(),
    };
    let second = ConnectTarget::Wsl {
        distro: "second".into(),
        session: "default".into(),
    };
    let mut probes = Harnesses::default();
    probes.bind("saved-host", &first);
    probes.record("saved-host", vec![Agent::Claude]);
    assert_eq!(
        probes.agents_for("saved-host", &first),
        Some([Agent::Claude].as_slice())
    );
    assert_eq!(probes.agents_for("saved-host", &second), None);
    probes.bind("saved-host", &second);
    assert!(probes.begin("saved-host", REMOTE_REFRESH, Instant::now()));
    assert_eq!(probes.agents_for("saved-host", &second), None);
}
