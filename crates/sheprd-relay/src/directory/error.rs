/// Why a relay list cannot be used.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    // keep-sorted start
    #[error("relay {index} has an endpoint that is not a relay address")]
    Endpoint {
        index: usize,
        #[source]
        source: crate::endpoint::Error,
    },
    #[error("the relay list expired at {expires}")]
    Expired { expires: u64 },
    #[error("the relay list is not the expected JSON")]
    Format,
    #[error("relay {index} is not https")]
    Insecure { index: usize },
    #[error("a relay's {field} is too long or not printable")]
    Label { field: &'static str },
    #[error("relay {index} has a QUIC pin that is not 32 bytes of base64url")]
    Pin { index: usize },
    #[error("relay {index} is on this computer or a private network")]
    Private { index: usize },
    #[error("the relay list is number {serial}, older than the kept number {kept}")]
    Stale { serial: u64, kept: u64 },
    #[error("the relay list is larger than 64 KiB")]
    TooLarge,
    #[error("the relay list names more than 64 relays")]
    TooMany,
    #[error("the relay list carries no signature from a known signer")]
    Unsigned,
    #[error("the relay list has no serial and expiry to sign")]
    Unstamped,
    #[error("the relay list is version {version}; this client reads version 1")]
    Version { version: u32 },
    // keep-sorted end
}
