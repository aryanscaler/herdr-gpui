#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Invalid remote work request: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Remote work message exceeds {limit} bytes")]
    Size { limit: usize },
    #[error("Unsupported remote work protocol version {0}")]
    Version(u8),
}
