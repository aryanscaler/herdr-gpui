//! The relay: joins a phone or computer to another computer when they have
//! no direct path, so remote control and SSH work from any network without
//! Tailscale. `docs/relay-plan.md` has the design.
//!
//! The relay is a blind pipe. Peers run their own end-to-end encrypted
//! handshake inside the stream it carries, so taking the relay over yields
//! ciphertext, timing, and sizes, never content:
//!
//! - A computer **listens** on a [`RouteId`], the hash of a P-256 public key
//!   it proves it holds. Nothing else identifies it.
//! - A peer **dials** that route. The relay tells the listener, which
//!   **accepts** with a fresh socket, and the two are spliced.
//! - Every resource is bounded: connections in total and per address,
//!   listeners, sessions, frame sizes, and bandwidth per session and in
//!   total ([`Limits`]). Nothing is stored and nothing survives a restart.
//! - Logs carry no addresses, routes, or payloads. The dashboard shows only
//!   the start of a route.
//!
//! - [`directory`]: Sheprd's own relays, listed on sheprd.org.
//! - [`endpoint`]: a relay's address, Sheprd's or a private one, and the
//!   URLs built from it.
//! - [`options`]: settings from flags and `RELAY_*` variables, all optional.
//! - [`limits`]: the bounds, and their safe defaults.
//! - [`bucket`]: keyed rate limits and bandwidth throttles.
//! - [`route`]: route and session ids, and the listener's proof.
//! - [`secret`]: secrets compared in constant time.
//! - [`guard`]: banning addresses that misbehave, automatically or by hand.
//! - [`hub`]: who is listening, who waits, and what each address holds.
//! - [`splice`]: carrying frames between a dialer and a listener.
//! - [`metrics`]: Prometheus counters and the dashboard's snapshot.
//! - [`dashboard`]: the read-only page behind a login.
//! - [`wire`]: the QUIC protocol's binary requests and statuses.
//! - [`socket`]: the WebSocket fallback's protocol.
//! - [`quic`]: the QUIC transport, sessions as streams.
//! - [`web`]: the WebSocket fallback over HTTPS, for networks without UDP,
//!   and the dashboard's routes.

//!
//! Without the `server` feature, only [`directory`], [`endpoint`],
//! [`route`], [`socket`], and [`wire`] are built:
//! what a client needs to reach a relay.

#[cfg(feature = "server")]
mod broker;
#[cfg(feature = "server")]
pub mod bucket;
#[cfg(feature = "server")]
pub mod dashboard;
pub mod directory;
pub mod endpoint;
mod error;
#[cfg(feature = "server")]
pub mod guard;
#[cfg(feature = "server")]
pub mod hub;
#[cfg(feature = "server")]
pub mod limits;
#[cfg(feature = "server")]
pub mod metrics;
#[cfg(feature = "server")]
pub mod options;
#[cfg(feature = "server")]
pub mod quic;
pub mod route;
#[cfg(feature = "server")]
pub mod secret;
#[cfg(feature = "server")]
mod serve;
pub mod socket;
#[cfg(feature = "server")]
pub mod splice;
#[cfg(feature = "server")]
pub mod web;
pub mod wire;

pub use endpoint::Endpoint;
pub use error::{Error, Result};
#[cfg(feature = "server")]
pub use limits::Limits;
#[cfg(feature = "server")]
pub use options::{Options, USAGE};
pub use route::{RouteId, SessionId};
#[cfg(feature = "server")]
pub use serve::{bind_quic, serve};

#[cfg(test)]
mod tests;
