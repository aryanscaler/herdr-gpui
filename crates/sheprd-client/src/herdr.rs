//! Herdr's own view of panes and the agents in them, as its socket API and
//! `herdr` CLI report them, and what Sheprd does with that view in more
//! than one place: Teleport, the app's chats, and the remote control daemon.

mod agent_kind;
pub mod convert;
pub mod processes;
pub mod sessions;
pub mod snapshot;
pub mod worktree;

pub use agent_kind::AgentKind;
