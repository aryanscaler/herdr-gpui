/// Why text is not a relay endpoint.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    // keep-sorted start
    #[error("a relay address cannot carry a user name or password")]
    Credentials,
    #[error("a relay address cannot carry a query or fragment")]
    Extra,
    #[error("plain http is only for this computer or a private network; use https")]
    Insecure,
    #[error("a relay address needs a host")]
    NoHost,
    #[error("a relay address must be https or wss, not {scheme}")]
    Scheme { scheme: String },
    #[error("that is not a URL")]
    Url(#[from] url::ParseError),
    // keep-sorted end
}
