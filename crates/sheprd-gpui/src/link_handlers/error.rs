#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("link_handlers: {0}")]
    Invalid(&'static str),
    #[error("link_handlers pattern: {0}")]
    Pattern(#[from] regex::Error),
    #[error("The clicked terminal's host is no longer available")]
    HostChanged,
    #[error("Link handler failed: {0}")]
    Run(#[from] sheprd_client::Error),
    #[error("Link handler failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("Link handler manifest: {0}")]
    Manifest(#[from] toml::de::Error),
    #[error("Link handler manifest is not UTF-8: {0}")]
    Utf8(#[source] std::string::FromUtf8Error),
}
