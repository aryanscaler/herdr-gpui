/// Why the daemon did not change a Herdr 2 tab's layout itself.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    // keep-sorted start
    #[error("Sheprd changed its tabs meanwhile; try again")]
    Changed,
    #[error(transparent)]
    Edit(#[from] sheprd_session::saved::edit::Error),
    #[error("This tab is the Herdr desktop's; change it there")]
    Fixed,
    #[error("This computer keeps no more Herdr 2 tab layouts")]
    Full,
    #[error("Sheprd's saved tabs could not be read or written")]
    Io(#[from] std::io::Error),
    #[error("A workspace needs a name")]
    Name,
    #[error("This computer keeps no Sheprd tabs")]
    NoTabs,
    #[error(transparent)]
    Panes(#[from] sheprd_session::panes::Error),
    #[error("Sheprd is starting on this computer; try again in a moment")]
    SheprdOpen,
    #[error("That is not a Herdr 2 terminal")]
    Terminal(#[from] sheprd_backend::Error),
    // keep-sorted end
}
