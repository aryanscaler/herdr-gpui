//! Work requests on an authenticated remote core connection. The transport
//! supplies the terminal, host and workspace; none can be asserted by JSON.
//! This namespace conveys no browser authority and cannot forward arbitrary
//! control requests. A reply acknowledges preparation, never confirmation.
use super::WorkOp;
use crate::protocol::Response;
use serde::{Deserialize, Serialize};

mod error;
pub use error::Error;

pub const REQUEST_PREFIX: &str = "sheprd-work-";
pub const REPLY_PREFIX: &str = "sheprd-work-reply-";
/// A window's claim on a request, beside it: the caller removes the request
/// to cancel, which a claim can therefore never recreate.
pub const CLAIM_PREFIX: &str = "sheprd-work-claim-";
pub const MAX_REQUEST: usize = 4096;
pub const MAX_REPLY: usize = 16 * 1024;
pub const VERSION: u8 = 1;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub version: u8,
    pub window: Option<u64>,
    pub operation: WorkOp,
}

/// A nonce is never reused, even after cancellation or an ambiguous timeout.
pub fn valid_id(id: &str) -> bool {
    id.len() == 32 && id.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub fn request_id(name: &str) -> Option<&str> {
    name.strip_prefix(REQUEST_PREFIX).filter(|id| valid_id(id))
}

impl Request {
    pub fn decode(text: &str) -> Result<Self, Error> {
        if text.len() > MAX_REQUEST {
            return Err(Error::Size { limit: MAX_REQUEST });
        }
        let request: Self = serde_json::from_str(text)?;
        if request.version != VERSION {
            return Err(Error::Version(request.version));
        }
        Ok(request)
    }
}

/// One bounded reply. A large read refuses instead of truncating a JSON value
/// or returning another host's data in a second, unscoped request.
pub fn encode_reply(response: &Response) -> Result<String, Error> {
    let json = serde_json::to_string(response)?;
    if json.len() > MAX_REPLY {
        return Err(Error::Size { limit: MAX_REPLY });
    }
    Ok(json)
}

#[cfg(test)]
mod tests;
