//! Moving the agent in a core terminal into a chat, through the shared
//! `sheprd_core_client::convert` the app uses too.

use super::{Error, Source, lock, pane_id};
use crate::hub::convert::{Error as ConvertError, Result};
use sheprd_backend::TerminalId;
use sheprd_chat_protocol::Agent;
use std::{path::Path, sync::atomic::AtomicBool};

impl Source {
    /// Blocking: moves `agent` in the terminal behind `pane` into a chat run
    /// by `program`; `home` holds the agent's own records. Everything that
    /// could refuse is checked before the agent is touched.
    pub(in crate::hub) fn convert(
        &self,
        pane: &str,
        agent: Agent,
        program: &Path,
        home: &Path,
        cancelled: &AtomicBool,
    ) -> Result<()> {
        let (core, address) = {
            let state = lock(&self.state);
            let core = state
                .core
                .clone()
                .ok_or(crate::hub::Error::from(Error::Disconnected))?;
            let boot = &core.hello().core.id;
            let address = state
                .terminals
                .iter()
                .find(|terminal| pane_id(boot, &terminal.address) == pane)
                .map(|terminal| terminal.address.clone())
                .ok_or(sheprd_client::herdr::convert::Error::Moved)?;
            // Another machine's terminal: its agent's records are there.
            if address.contains('/') {
                return Err(sheprd_client::herdr::convert::Error::NoAgent.into());
            }
            (core, address)
        };
        let terminal =
            TerminalId::new(address).map_err(|_| sheprd_client::herdr::convert::Error::Moved)?;
        // The chat is followed once listed: the terminal is labelled with it.
        sheprd_core_client::convert::run(&core, &terminal, agent, program, home, cancelled)
            .map(|_endpoint| ())
            .map_err(|error| match error {
                sheprd_core_client::Error::Convert(refused) => ConvertError::Refused(refused),
                error => crate::hub::Error::from(Error::Client(error)).into(),
            })
    }
}
