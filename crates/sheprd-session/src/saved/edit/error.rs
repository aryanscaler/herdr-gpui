use crate::panes;

/// Why a change to the app's saved tabs was not made.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    // keep-sorted start
    #[error("The saved tabs are not readable")]
    Format(#[from] serde_json::Error),
    #[error("The saved tabs are larger than the app reads")]
    Large,
    #[error("The tab's panes cannot change that way")]
    Panes(#[from] panes::Error),
    #[error("The app keeps no such tab of Herdr 2 terminals")]
    Tab,
    #[error("The tab only watches its terminal")]
    Watching,
    #[error("The app keeps no such workspace")]
    Workspace,
    // keep-sorted end
}
