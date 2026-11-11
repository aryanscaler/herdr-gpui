//! Why a phone's question about a workspace, or its change to a Herdr 2
//! tab's layout, has no answer.

use crate::control::{changes::Failure, layout};
use axum::http::StatusCode;

#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
    // keep-sorted start
    #[error("too many questions are waiting for Sheprd")]
    Busy,
    #[error(transparent)]
    Failed(#[from] Failure),
    #[error(transparent)]
    Layout(#[from] layout::Failure),
    #[error("Sheprd is not open on this computer")]
    NoDesktop,
    #[error("Sheprd did not answer in time")]
    TimedOut,
    // keep-sorted end
}

impl Error {
    pub(crate) fn status(&self) -> StatusCode {
        match self {
            Self::Busy => StatusCode::TOO_MANY_REQUESTS,
            Self::Failed(Failure::RateLimited | Failure::GitHubPending) => {
                StatusCode::TOO_MANY_REQUESTS
            }
            Self::Failed(Failure::GitHubSignIn | Failure::GitHubScope) => StatusCode::CONFLICT,
            Self::Failed(Failure::Gone | Failure::NoFile) => StatusCode::NOT_FOUND,
            Self::Failed(
                Failure::LogTooLong | Failure::NoBase | Failure::NoLog | Failure::NoRepository,
            ) => StatusCode::CONFLICT,
            Self::Failed(Failure::Unreadable | Failure::GitHubUnavailable) => {
                StatusCode::BAD_GATEWAY
            }
            Self::Layout(layout::Failure::Busy) => StatusCode::TOO_MANY_REQUESTS,
            Self::Layout(layout::Failure::Gone) => StatusCode::NOT_FOUND,
            Self::Layout(layout::Failure::Watching) => StatusCode::CONFLICT,
            Self::Layout(layout::Failure::Refused) => StatusCode::BAD_GATEWAY,
            Self::NoDesktop => StatusCode::SERVICE_UNAVAILABLE,
            Self::TimedOut => StatusCode::GATEWAY_TIMEOUT,
        }
    }
}
