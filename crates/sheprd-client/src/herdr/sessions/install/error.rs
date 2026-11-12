use std::path::PathBuf;

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("session reporter installation is supported only on Linux and macOS")]
    UnsupportedPlatform,
    #[error("session-install supports only codex, copilot and opencode")]
    UnsupportedAgent,
    #[error("configuration directory must be an absolute UTF-8 path: {0}")]
    Path(PathBuf),
    #[error("could not access {path}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error(
        "cannot safely edit {path}: expected {expected}; see crates/sheprd-acp/README.md, Manual session reporter installation"
    )]
    Shape {
        path: PathBuf,
        expected: &'static str,
    },
    #[error(
        "cannot safely edit {path}: JSONC/comments and malformed JSON are not rewritten; see crates/sheprd-acp/README.md, Manual session reporter installation"
    )]
    Json {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("cannot edit TOML configuration {path}")]
    Toml {
        path: PathBuf,
        #[source]
        source: toml_edit::TomlError,
    },
}
