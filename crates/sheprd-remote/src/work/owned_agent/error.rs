#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Ownership could not be verified; inspect the retained checkout and terminal")]
    Uncertain,
    #[error(transparent)]
    Work(#[from] super::super::Error),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
