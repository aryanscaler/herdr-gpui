use std::{ffi::OsString, io, net::SocketAddr, path::PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    // keep-sorted start
    #[error("{argument:?} is not a valid argument")]
    Argument { argument: OsString },
    #[error("could not listen on {address}")]
    Bind {
        address: SocketAddr,
        #[source]
        source: io::Error,
    },
    #[error(transparent)]
    Directory(#[from] crate::directory::Error),
    #[error(transparent)]
    Endpoint(#[from] crate::endpoint::Error),
    #[error("{name} is not valid: {value:?}")]
    Invalid { name: &'static str, value: OsString },
    #[cfg(feature = "server")]
    #[error(transparent)]
    Quic(#[from] crate::quic::Error),
    #[error("could not draw random bytes")]
    Random(#[source] getrandom::Error),
    #[error("could not start the runtime")]
    Runtime(#[source] io::Error),
    #[error("could not read {}", path.display())]
    SecretRead {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("the server failed")]
    Serve(#[source] io::Error),
    #[error("the listen token file holds more than {max} tokens")]
    TooManyTokens { max: usize },
    #[error("{name} must be at least {min} characters")]
    WeakSecret { min: usize, name: &'static str },
    // keep-sorted end
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
