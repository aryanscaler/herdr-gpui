/// Why px0 could not be started on an SSH host, or reached from here.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    // keep-sorted start
    #[error("px0 did not publish its workbench address on the host in time")]
    Address,
    #[error("Cannot run px0 on the host: {0}")]
    Command(#[source] sheprd_client::Error),
    #[error("Cannot forward the host's px0 workbench here: {0}")]
    Forward(#[source] sheprd_client::Error),
    #[error("px0 is not installed on the host; install it on its PATH")]
    Missing,
    #[error("Cannot read px0's workbench address on the host: {0}")]
    Read(#[source] sheprd_client::Error),
    #[error("Cannot start the SSH connection for px0: {0}")]
    Spawn(#[source] std::io::Error),
    #[error("SSH to the host failed")]
    Ssh,
    #[error("px0 on the host ended ({0})")]
    Stopped(std::process::ExitStatus),
    #[error("The host's px0 did not answer in time")]
    Timeout,
    // keep-sorted end
}

impl Error {
    /// Why the `ssh` running `serve.sh` ended with `status`: the script
    /// exits 127 when px0 is missing and with px0's status otherwise, and
    /// `ssh` exits 255 for its own failures.
    pub(super) fn exited(status: std::process::ExitStatus) -> Self {
        match status.code() {
            Some(127) => Self::Missing,
            Some(255) => Self::Ssh,
            _ => Self::Stopped(status),
        }
    }
}
