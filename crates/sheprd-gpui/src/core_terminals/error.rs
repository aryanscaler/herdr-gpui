#[derive(Debug, thiserror::Error)]
pub enum Error {
    // keep-sorted start
    #[error("Could not answer from the terminal's history")]
    Answer,
    #[error(transparent)]
    Bridge(#[from] sheprd_client::Error),
    #[error(transparent)]
    Core(#[from] sheprd_core_client::Error),
    #[error("Could not save the terminal's history for the editor")]
    EditFile(#[source] std::io::Error),
    #[error("Could not read the terminal's history")]
    History(#[source] std::sync::Arc<dyn std::error::Error + Send + Sync>),
    #[error("The selected text moved before it was read")]
    Moved,
    #[error("This build has no herdr-core beside it; install herdr-core from releases.herdr.dev")]
    NotBundled,
    /// A remote core was asked of a target on this computer.
    #[error("This host is on this computer")]
    NotRemote,
    #[error("Could not start a worker for the terminal: {0}")]
    Worker(#[source] std::io::Error),
    // keep-sorted end
}

impl Error {
    /// Whether the host has no herdr-core installed, so its tabs stay with
    /// its Herdr until one is.
    pub(crate) fn core_missing(&self) -> bool {
        let Self::Bridge(error) = self else {
            return false;
        };
        let missing = |error: &sheprd_client::Error| {
            matches!(
                error,
                sheprd_client::Error::CoreBridge(sheprd_client::core_bridge::Error::Missing)
            )
        };
        match error {
            // A cloud connector reports through `io::Error`.
            sheprd_client::Error::Io(io) => io
                .get_ref()
                .and_then(|inner| inner.downcast_ref::<sheprd_client::Error>())
                .is_some_and(missing),
            error => missing(error),
        }
    }
}
