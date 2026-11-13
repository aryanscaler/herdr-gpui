#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Too many pending Teleport arrivals; reopen the app to recover them first")]
    Full,
    #[error("Could not persist the Teleport arrival")]
    Io(#[from] std::io::Error),
    #[error("Invalid Teleport arrival record")]
    Json(#[from] serde_json::Error),
    #[error("Teleport arrival record exceeds its size limit")]
    TooLarge,
    #[error("The app has no state directory for a durable Teleport arrival")]
    Unavailable,
    #[error("Unsupported Teleport arrival record version")]
    Version,
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
