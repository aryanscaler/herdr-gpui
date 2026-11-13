#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("the previous chat's agent child is still alive or its cleanup is unverified")]
    ChildAlive,
    #[error("this chat was deliberately closed")]
    Closed,
    #[error("invalid or unsupported chat recovery record")]
    Invalid,
    #[error("chat recovery reached its bounded history limit")]
    Limit,
    #[error("the chat recovery owner or launch generation changed")]
    Moved,
}
