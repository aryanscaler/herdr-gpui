use super::*;
use crate::px0::Error;

#[test]
fn every_px0_variant_has_a_message() {
    let io = || std::io::Error::other("disk on fire");
    assert_localized(&[
        Error::Address,
        Error::Missing,
        Error::StateDirectory,
        Error::Discovery(io()),
        Error::Spawn(io()),
        Error::Worker(io()),
        Error::Http(ureq::Error::ConnectionFailed),
        Error::Read(io()),
        Error::Json(serde_json::from_str::<u8>("x").unwrap_err()),
        Error::NotWorkbench,
        Error::Timeout,
        Error::Exited,
        Error::Process(io()),
        Error::Shutdown(std::sync::mpsc::RecvTimeoutError::Timeout),
    ]);
}

#[test]
fn every_px0_host_variant_has_a_message() {
    use crate::px0::host::Error as Host;
    assert_localized(&[
        Host::Address,
        Host::Command(sheprd_client::Error::Disconnected),
        Host::Exited,
        Host::Forward(sheprd_client::Error::Disconnected),
        Host::Missing,
        Host::Output,
        Host::Setup,
        Host::Spawn(std::io::Error::other("disk on fire")),
        Host::Ssh,
        Host::Timeout,
        #[cfg(unix)]
        Host::Stopped(std::os::unix::process::ExitStatusExt::from_raw(256)),
    ]);
    assert_localized(&[Error::Host(Host::Missing)]);
}
