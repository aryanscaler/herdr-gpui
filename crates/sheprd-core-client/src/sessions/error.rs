#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("no verified session selection is available for this terminal")]
    Absent,
    #[error("the agent explicitly cleared its session selection")]
    Cleared,
    #[error("the agent did not report an explicit session selection")]
    Event,
    #[error(transparent)]
    Evidence(#[from] sheprd_client::herdr::sessions::Error),
    #[error("the session reporter no longer owns this terminal process")]
    Identity,
    #[error(transparent)]
    Probe(#[from] sheprd_client::Error),
    #[error("the session report is older than the recorded selection")]
    Sequence,
}
