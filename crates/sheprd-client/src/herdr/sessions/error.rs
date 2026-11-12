#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("session evidence contains an unsupported configuration variable")]
    Environment,
    #[error("session evidence belongs to another process or terminal")]
    Identity,
    #[error("invalid session evidence")]
    Json(#[from] serde_json::Error),
    #[error("session evidence contains an invalid session reference")]
    Session,
    #[error("session evidence exceeds its size limit")]
    TooLarge,
    #[error("unsupported session evidence version")]
    Version,
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
