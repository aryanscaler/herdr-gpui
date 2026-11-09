/// Why px0 could not be started on an SSH host, or reached from here.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    // keep-sorted start
    #[error("px0 did not publish its workbench address on the host in time")]
    Address,
    #[error("Cannot run px0 on the host: {0}")]
    Command(#[source] sheprd_client::Error),
    #[error("px0 stopped on the host before its workbench was ready")]
    Exited,
    #[error("Cannot forward the host's px0 workbench here: {0}")]
    Forward(#[source] sheprd_client::Error),
    #[error("px0 is not installed on the host; install it on its PATH")]
    Missing,
    #[error("The host's px0 script answered with something this app cannot read")]
    Output,
    #[error("Cannot prepare px0's folder on the host")]
    Setup,
    #[error("Cannot start the SSH connection for px0: {0}")]
    Spawn(#[source] std::io::Error),
    #[error("SSH to the host failed")]
    Ssh,
    #[error("The connection to the host's px0 ended ({0})")]
    Stopped(std::process::ExitStatus),
    #[error("The host's px0 did not answer in time")]
    Timeout,
    // keep-sorted end
}

impl Error {
    /// The script's failure for its exit `status`, as `serve.sh` names
    /// them; `ssh` exits 255 for its own.
    pub(super) fn exited(status: std::process::ExitStatus) -> Self {
        match status.code() {
            Some(2) => Self::Setup,
            Some(3) => Self::Missing,
            Some(4) => Self::Address,
            Some(5) => Self::Exited,
            Some(255) => Self::Ssh,
            _ => Self::Stopped(status),
        }
    }
}
