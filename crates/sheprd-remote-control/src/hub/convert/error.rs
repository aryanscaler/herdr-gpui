use sheprd_client::herdr::convert;

/// Why a phone's request to move a terminal agent into a chat failed. All
/// but [`Self::Hub`] and a [`Self::Refused`] after the stop began leave the
/// agent running.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    // keep-sorted start
    #[error("another request is already moving this agent into a chat")]
    AlreadyConverting,
    #[error("Herdr's answer about the pane did not have the expected shape")]
    Answer(#[source] serde_json::Error),
    #[error("this computer's home folder is unknown, where the agent keeps its session")]
    Home,
    #[error(transparent)]
    Hub(Box<crate::hub::Error>),
    #[error("agents move into chats on macOS and Linux")]
    Platform,
    #[error(transparent)]
    Refused(#[from] convert::Error),
    #[error("the worker stopped before the agent moved")]
    Worker,
    // keep-sorted end
}

impl From<crate::hub::Error> for Error {
    fn from(error: crate::hub::Error) -> Self {
        Self::Hub(Box::new(error))
    }
}
