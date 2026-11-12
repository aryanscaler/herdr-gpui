#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
    #[error(transparent)]
    Cli(#[from] crate::cli::Error),
    #[error(transparent)]
    Core(#[from] sheprd_core_client::Error),
    #[error("the selected core does not match SHEPRD_EXPECT_CORE")]
    CoreIdentity,
    #[error(transparent)]
    Install(#[from] sheprd_client::herdr::sessions::install::Error),
    #[error("session helper I/O failed")]
    Io(#[from] std::io::Error),
    #[error("could not encode the session binding")]
    Json(#[from] serde_json::Error),
    #[error("session-report requires HC_TERMINAL_ID")]
    MissingTerminal,
    #[error("session-report requires the owning HC_TERMINAL_SOCKET")]
    MissingTerminalSocket,
    #[error("could not determine the home directory")]
    NoHome,
    #[error("HC_TERMINAL_ID must be valid UTF-8")]
    TerminalEncoding,
    #[error("session report exceeds {limit} bytes")]
    TooLarge { limit: usize },
}
