/// Why an agent did not move into a chat, beyond what the shared policy in
/// [`Self::Convert`] refuses. Each one before the agent is told to stop
/// leaves it running; [`Self::Disabled`], [`Self::Stale`], and
/// [`Self::Type`] can also come after, and leave its session to continue by
/// hand.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    // keep-sorted start
    #[error("another agent is still moving into a chat")]
    AlreadyConverting,
    #[error("could not read the pane's processes")]
    Answer(#[source] serde_json::Error),
    #[error(transparent)]
    Convert(#[from] sheprd_client::herdr::convert::Error),
    #[error("{0}")]
    Core(#[source] std::sync::Arc<sheprd_core_client::Error>),
    #[error("ACP chats are off in Settings")]
    Disabled,
    #[error(transparent)]
    Existing(#[from] super::existing::Error),
    #[error("sheprd-acp was not found on this host")]
    HostProgram(#[source] super::existing::Error),
    #[error("no home folder to find the agent's session in")]
    NoHome,
    #[error("Sheprd is not connected")]
    NotConnected,
    #[error("agents move into chats on this machine's Herdr or an SSH host's, on macOS and Linux")]
    NotReachable,
    #[error(transparent)]
    Program(#[from] crate::chat::StartError),
    #[error("the window moved to another connection before the chat could start")]
    Stale,
    #[error("{0}")]
    Type(#[source] std::sync::Arc<crate::Error>),
    #[error("could not start the worker")]
    Worker(#[source] std::io::Error),
    // keep-sorted end
}
