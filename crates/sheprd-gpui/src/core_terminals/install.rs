//! Installing the pinned herdr-core on a saved device, from its menu: the
//! user's explicit ask, run on a background thread through the device's own
//! connection (`sheprd_remote::core_install`). A device that already has a
//! core is left as it is. Once the core is there, every window that could
//! not reach the host drops its hub, so the next Herdr 2 tab dials it again
//! rather than remembering that the host had none.

use super::{Host, Start};
use crate::window::main_window::MainWindow;
use crate::{
    SheprdWindow,
    i18n::{Localize, fl},
    window::Flash,
};
use gpui::{Context, Global};
use sheprd_client::{ConnectTarget, ScriptHost, SshLane};
use sheprd_remote::core_install::{self, Outcome, Target};
use std::collections::HashSet;
use std::sync::atomic::AtomicBool;

/// Why the install did not finish.
#[derive(Debug)]
pub(crate) enum Failed {
    Install(core_install::Error),
    /// A cloud provider could not prepare the machine's command.
    #[cfg(feature = "cloud")]
    Command(std::io::Error),
    /// The device is this computer, which has the bundled core.
    Local,
}

impl Localize for Failed {
    fn localize(&self) -> String {
        match self {
            Self::Install(error) => error.localize(),
            // The system's or provider's own words, as other cloud commands show them.
            #[cfg(feature = "cloud")]
            Self::Command(error) => error.to_string(),
            Self::Local => fl!("core-terminals-install-local"),
        }
    }
}

/// Blocking: installs the pinned core on `target` unless it has one.
fn install(target: &ConnectTarget) -> Result<Outcome, Failed> {
    // The install runs to its end; its idle bound stops a stalled one.
    let stop = AtomicBool::new(false);
    match target {
        ConnectTarget::Ssh { target, .. } => {
            core_install::install(Target::Host(ScriptHost::Ssh(target, SshLane::Bulk)), &stop)
                .map_err(Failed::Install)
        }
        ConnectTarget::Wsl { distro, .. } => {
            core_install::install(Target::Host(ScriptHost::Wsl(distro)), &stop)
                .map_err(Failed::Install)
        }
        #[cfg(feature = "cloud")]
        ConnectTarget::Cloud {
            provider,
            account,
            id,
            ..
        } => sheprd_remote::cloud::with_command(*provider, account, id, &stop, |command| {
            core_install::install(Target::Command(command), &stop)
        })
        .map_err(Failed::Command)?
        .map_err(Failed::Install),
        ConnectTarget::Local | ConnectTarget::Session { .. } | ConnectTarget::Socket(_) => {
            Err(Failed::Local)
        }
    }
}

/// Devices with an install running, from any window, so a second ask waits
/// for the first rather than racing it.
#[derive(Default)]
struct Running(HashSet<String>);

impl Global for Running {}

impl SheprdWindow {
    /// Installs the pinned herdr-core on the saved device `id`, saying when
    /// it starts and how it ended. Refused while one runs there.
    pub(crate) fn install_core_on(&mut self, id: &str, cx: &mut Context<Self>) {
        self.install_core_with(id, install, cx);
    }

    /// [`Self::install_core_on`] running `install`, which tests replace.
    pub(crate) fn install_core_with(
        &mut self,
        id: &str,
        install: fn(&ConnectTarget) -> Result<Outcome, Failed>,
        cx: &mut Context<Self>,
    ) {
        let Some(endpoint) = self.endpoints.iter().find(|endpoint| endpoint.id == id) else {
            return;
        };
        if !cx.default_global::<Running>().0.insert(id.to_owned()) {
            let device = endpoint.label.clone();
            self.show_flash(
                Flash::hint(fl!(
                    "core-terminals-install-running",
                    device = device.as_str()
                )),
                cx,
            );
            return;
        }
        let id = id.to_owned();
        let target = endpoint.connection.target.clone();
        let device = endpoint.label.clone();
        let host = Host::of(&crate::browser::scope(endpoint));
        self.show_flash(
            Flash::hint(fl!(
                "core-terminals-install-started",
                device = device.as_str()
            )),
            cx,
        );
        let background = cx
            .background_executor()
            .spawn(async move { install(&target) });
        cx.spawn(async move |this, cx| {
            let result = background.await;
            let installed = result.is_ok();
            let _ = cx.update(|cx| cx.default_global::<Running>().0.remove(&id));
            let _ = this.update(cx, |view, cx| {
                view.core_install_ended(host.clone(), &device, result, cx)
            });
            // Every other window that found the host without a core dials
            // it again too. This one already did: a second reset would drop
            // the attempt it just made and dial the host twice.
            if installed {
                let this = this.entity_id();
                cx.update(|cx| {
                    for handle in MainWindow::all(cx) {
                        let _ = handle.update(cx, |view, _, cx| {
                            if cx.entity_id() != this {
                                view.core_installed(&host);
                            }
                        });
                    }
                });
            }
        })
        .detach();
    }

    /// Forgets what this window found of `host` once it has a core. Only a
    /// host the window could not reach is dialed anew; a connected core
    /// keeps its views and their history. A terminal asked for meanwhile
    /// keeps its place and waits for the new attempt.
    fn core_installed(&mut self, host: &Host) {
        let terminals = &mut self.core_terminals;
        if terminals.backend_of(host).is_some() {
            return;
        }
        let waiting = terminals.waits_on(host);
        let Some(hub) = terminals.hub_of_mut(host) else {
            return;
        };
        hub.forget();
        if waiting {
            hub.connect(Start::IfMissing);
        }
    }

    pub(crate) fn core_install_ended(
        &mut self,
        host: Host,
        device: &str,
        result: Result<Outcome, Failed>,
        cx: &mut Context<Self>,
    ) {
        let flash = match result {
            Ok(outcome) => {
                self.core_installed(&host);
                let version = core_install::pinned()
                    .map(|pinned| pinned.version)
                    .unwrap_or_default();
                Flash::success(match outcome {
                    Outcome::Installed => {
                        fl!(
                            "core-terminals-install-installed",
                            device = device,
                            version = version
                        )
                    }
                    Outcome::Present => fl!("core-terminals-install-present", device = device),
                })
            }
            Err(error) => {
                tracing::warn!(?error, "Could not install herdr-core on a device");
                let reason = error.localize();
                Flash::warning(fl!(
                    "core-terminals-install-failed",
                    device = device,
                    reason = reason
                ))
            }
        };
        self.show_flash(flash, cx);
    }
}
