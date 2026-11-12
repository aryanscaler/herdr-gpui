#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("could not connect to the chat's host")]
    Connect(#[from] std::io::Error),
    #[error(transparent)]
    Script(#[from] sheprd_client::Error),
}
