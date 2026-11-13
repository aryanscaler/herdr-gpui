//! How much a QUIC connection may make the relay hold. A peer can send up
//! to its connection's receive window that the relay has not read, which
//! it stops doing while the far side of a session does not read either;
//! and the relay holds up to the send window of what it wrote that the
//! peer has not acknowledged. So each window is a promise of memory.
//!
//! A connection starts with [`BASE`], enough for requests and a listener's
//! messages, and gains [`SESSION`] each way for each session it carries.
//! Both come from the hub's buffer budget (`max_buffer`), so however many
//! connections callers open, the relay never promises more memory than
//! that: when the budget is spent, new connections and sessions are
//! refused rather than buffered.

use crate::hub::{Buffer, Hub};
use quinn::{Connection, VarInt};
use std::sync::{Arc, Mutex};

/// A connection's window each way while it carries no session.
pub const BASE: u32 = 64 * 1024;
/// What each session adds to its connection's windows, and the most one
/// stream may have in flight: 2 MiB passes a session's 4 MiB/s limit over
/// a 500 ms round trip.
pub const SESSION: u32 = 2 * 1024 * 1024;

/// The buffer budget a connection takes before its handshake, for both
/// of its base windows.
#[must_use]
pub fn base_cost() -> u64 {
    2 * u64::from(BASE)
}

/// One connection's windows, sized to the sessions it carries.
#[derive(Debug)]
pub struct Window {
    connection: Connection,
    sessions: Mutex<u32>,
}

impl Window {
    #[must_use]
    pub fn new(connection: Connection) -> Arc<Self> {
        Arc::new(Self {
            connection,
            sessions: Mutex::new(0),
        })
    }

    /// Widens the windows for one more session, while the result lives;
    /// `None` when the hub's budget, or this caller's share of it, cannot
    /// cover it. Counted against the connection's current address.
    pub fn widen(self: &Arc<Self>, hub: &Arc<Hub>) -> Option<Widened> {
        let address = self.connection.remote_address().ip();
        let buffer = hub.reserve_buffer(address, 2 * u64::from(SESSION))?;
        self.change(|sessions| sessions.saturating_add(1));
        Some(Widened {
            _buffer: buffer,
            window: Arc::clone(self),
        })
    }

    /// Applies `change` to the session count and sizes the windows to it,
    /// under one lock so concurrent changes apply in order.
    fn change(&self, change: impl FnOnce(u32) -> u32) {
        let mut sessions = match self.sessions.lock() {
            Ok(sessions) => sessions,
            Err(poisoned) => poisoned.into_inner(),
        };
        *sessions = change(*sessions);
        let bytes = u64::from(BASE) + u64::from(*sessions) * u64::from(SESSION);
        if let Ok(window) = VarInt::from_u64(bytes) {
            self.connection.set_receive_window(window);
        }
        self.connection.set_send_window(bytes);
    }
}

/// One session's share of its connection's windows.
#[derive(Debug)]
pub struct Widened {
    _buffer: Buffer,
    window: Arc<Window>,
}

impl Drop for Widened {
    fn drop(&mut self) {
        self.window.change(|sessions| sessions.saturating_sub(1));
    }
}
