use super::*;
use sheprd_client::ConnectTarget;
use sheprd_session::Keeper;

#[test]
fn custom_sockets_require_app_ownership_and_a_local_posix_shell() {
    let target = ConnectTarget::Socket("custom.sock".into());
    assert!(!scriptable(&target));
    assert!(!scriptable_for(&target, Keeper::Herdr1));
    assert_eq!(
        scriptable_for(&target, Keeper::App),
        cfg!(any(target_os = "linux", target_os = "macos"))
    );
}

#[test]
fn script_routes_respect_client_platform_and_destination_shell() {
    let ssh = ConnectTarget::Ssh {
        target: "box".into(),
        session: "default".into(),
    };
    assert!(scriptable_for(&ssh, Keeper::App));
    let wsl = ConnectTarget::Wsl {
        distro: "Ubuntu".into(),
        session: "default".into(),
    };
    assert_eq!(scriptable_for(&wsl, Keeper::App), cfg!(windows));
    assert_eq!(
        scriptable_for(&ConnectTarget::Local, Keeper::App),
        cfg!(any(target_os = "linux", target_os = "macos"))
    );
}
