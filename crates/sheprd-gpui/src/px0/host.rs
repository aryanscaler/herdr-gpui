//! px0 on the SSH hosts the windows connect to, one workbench per host, so a
//! remote workspace's px0 tab opens the workspace's own folder there. The
//! workbench on this computer cannot: the folder is on another machine.
//!
//! Each host's workbench is a [`worker::Worker`] whose `ssh` runs `serve.sh`
//! on the host. The script starts px0 there, as the app does here, with a
//! registry of the app's own under `~/.herdr-gpui/px0-state`, and reports the
//! loopback port it listens on; the worker forwards that port here. As here,
//! the app never installs px0 on a host. A workbench runs until the app quits
//! or its tab's Retry starts it again; nothing restarts on its own.
//!
//! px0 serves its workbench without a token, so on the host, as on this
//! computer, anyone who can reach its loopback port can use it.
// i18n: migrated
mod error;
mod worker;

pub use error::Error;

use super::State;
use gpui::{App, Global};
use std::{collections::HashMap, sync::Arc};

enum Slot {
    Running(worker::Worker),
    /// A state no worker reports: a start that failed, or a test's.
    Known(State),
}

/// The workbenches on the hosts, by SSH target.
#[derive(Default)]
pub(crate) struct Hosts {
    slots: HashMap<String, Slot>,
    quitting: bool,
}

impl Global for Hosts {}

impl Hosts {
    /// Installs the hosts' workbenches, stopped when the app quits. Native
    /// test modes never install them, so no host gets one.
    pub(crate) fn install(cx: &mut App) {
        cx.set_global(Self::default());
        cx.on_app_quit(|cx| {
            let hosts = cx.global_mut::<Self>();
            hosts.quitting = true;
            let workers: Vec<_> = hosts
                .slots
                .drain()
                .filter_map(|(_, slot)| match slot {
                    Slot::Running(worker) => Some(worker),
                    Slot::Known(_) => None,
                })
                .collect();
            cx.background_executor().spawn(async move {
                for worker in workers {
                    worker.shutdown();
                }
            })
        })
        .detach();
    }

    /// Starts px0 on `target` unless it runs or failed there already, as
    /// when a tab first needs it. Returns at once: the worker does the rest.
    pub(crate) fn want(cx: &mut App, target: &str) {
        if !crate::browser::EMBEDDED {
            return;
        }
        let Some(hosts) = cx.try_global::<Self>() else {
            return;
        };
        if hosts.quitting || hosts.slots.contains_key(target) {
            return;
        }
        let slot = match worker::Worker::start(target.to_owned()) {
            Ok(worker) => {
                tracing::info!(target, "Starting px0 on a host");
                Slot::Running(worker)
            }
            Err(error) => Slot::Known(State::Failed(Arc::new(error))),
        };
        cx.global_mut::<Self>()
            .slots
            .insert(target.to_owned(), slot);
    }

    pub(crate) fn state(cx: &App, target: &str) -> State {
        match cx
            .try_global::<Self>()
            .and_then(|hosts| hosts.slots.get(target))
        {
            Some(Slot::Running(worker)) => worker.state(),
            Some(Slot::Known(state)) => state.clone(),
            None => State::Idle,
        }
    }

    /// Forgets `target`'s workbench, so the next [`Self::want`] starts it
    /// again. Dropping its worker only asks it to stop: its own thread ends
    /// the `ssh`, which stops a workbench the script started.
    pub(crate) fn reset(cx: &mut App, target: &str) {
        if cx.has_global::<Self>() {
            cx.global_mut::<Self>().slots.remove(target);
        }
    }

    /// Hosts whose workbenches are in `states`, with nothing started.
    #[cfg(test)]
    pub(crate) fn fixture(cx: &mut App, states: impl IntoIterator<Item = (String, State)>) {
        cx.set_global(Self {
            slots: states
                .into_iter()
                .map(|(target, state)| (target, Slot::Known(state)))
                .collect(),
            quitting: false,
        });
    }
}

#[cfg(test)]
mod tests;
