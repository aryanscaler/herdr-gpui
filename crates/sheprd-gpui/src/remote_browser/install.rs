//! Puts the `herdr-gpui` shim, and the agent skill naming it, on each saved
//! SSH host a window connects to, through its Herdr or its herdr-core, once per run and only when the user chose
//! to install the skill; once they remove it, takes both off again. Like the
//! local skill, a file is written or removed only where it is missing or
//! carries this app's marker: a file of the same name the user wrote is left
//! alone, and nothing else on the host changes. A host offline when the
//! choice changed is updated when a window next connects to it.
use crate::{
    SheprdWindow,
    agent_skill::{AgentSkill, Choice},
};
use gpui::{Context, Global};
use sheprd_client::{ConnectTarget, ScriptHost, ScriptLimits, run_script};
use std::{collections::HashSet, sync::atomic::AtomicBool, time::Duration};

pub(super) const SHIM: &str = concat!(
    "#!/bin/sh\ncore_work() {\nexec python3 - \"$@\" <<'SHEPRD_WORK_PY'\n",
    include_str!("../remote_work/shim.py"),
    "\nSHEPRD_WORK_PY\n}\n",
    include_str!("herdr-gpui.sh"),
);
/// Where agents on the host run the shim from; their shell expands `~`.
pub(super) const SHIM_COMMAND: &str = "~/.local/state/herdr/gpui/bin/herdr-gpui";

/// Writes standard input to the shim's path unless the user has a file of
/// their own there.
// i18n: data
pub(super) const SHIM_SCRIPT: &str = r#"set -eu
dir="$HOME/.local/state/herdr/gpui/bin"
file="$dir/herdr-gpui"
if [ -e "$file" ] && ! grep -q herdr-gpui-managed-shim "$file"; then exit 0; fi
mkdir -p "$dir"
tmp="$file.new.$$"
trap 'rm -f "$tmp"' EXIT
cat >"$tmp"
chmod 755 "$tmp"
mv -f "$tmp" "$file""#;

/// Writes standard input as the skill in each agent directory the host has,
/// as the app does on this machine.
// i18n: data
pub(super) const SKILL_SCRIPT: &str = r#"set -eu
text=$(cat)
for root in "$HOME/.claude" "$HOME/.agents"; do
    [ -d "$root" ] || continue
    dir="$root/skills/herdr-gpui-browser"
    file="$dir/SKILL.md"
    if [ -e "$file" ] && ! grep -q herdr-gpui-managed-skill "$file"; then continue; fi
    mkdir -p "$dir"
    printf '%s\n' "$text" >"$file.new.$$"
    mv -f "$file.new.$$" "$file"
done"#;

/// Removes the shim and the skill copies this app wrote, keeping files the
/// user wrote under the same names.
// i18n: data
pub(super) const REMOVE_SCRIPT: &str = r#"set -u
remove() {
    if [ -f "$1" ] && grep -q "$2" "$1"; then rm -f "$1"; fi
}
remove "$HOME/.local/state/herdr/gpui/bin/herdr-gpui" herdr-gpui-managed-shim
for root in "$HOME/.claude" "$HOME/.agents"; do
    dir="$root/skills/herdr-gpui-browser"
    remove "$dir/SKILL.md" herdr-gpui-managed-skill
    rmdir "$dir" 2>/dev/null || :
done"#;

const LIMITS: ScriptLimits = ScriptLimits {
    output: 4096,
    idle: Duration::from_secs(30),
};

/// The hosts tried this run, by saved device, the machine it names, and the
/// choice applied, whether or not it worked: a host without a POSIX shell is
/// not asked again on every tick, while a changed choice, or a device edited
/// to name another machine, is applied again.
#[derive(Default)]
struct Tried(HashSet<(crate::browser::Scope, ConnectTarget, Choice)>);

impl Global for Tried {}

impl Tried {
    /// Whether this is the first try of `choice` on the machine `target`
    /// names for the saved device `scope`, recording it.
    fn first(
        &mut self,
        scope: crate::browser::Scope,
        target: &ConnectTarget,
        choice: Choice,
    ) -> bool {
        self.0.insert((scope, target.clone(), choice))
    }
}

/// Blocking: short SSH sessions. Run off the UI thread.
enum InstallHost {
    Remote(sheprd_remote::teleport::Host),
    Wsl(String),
}

fn apply(host: &InstallHost, choice: Choice) -> crate::Result<()> {
    let never = AtomicBool::new(false);
    let run = |script: &str, input: &[u8]| {
        match host {
            InstallHost::Remote(host) => {
                host.stream(script, input, std::io::sink(), LIMITS, &never)
            }
            InstallHost::Wsl(distro) => run_script(
                ScriptHost::Wsl(distro),
                script,
                input,
                std::io::sink(),
                LIMITS,
                &never,
            ),
        }
        .map(drop)
        .map_err(crate::Error::RemoteAgentTools)
    };
    match choice {
        Choice::Installed => {
            run(SHIM_SCRIPT, SHIM.as_bytes())?;
            let skill = crate::agent_skill::text_naming(SHIM_COMMAND);
            run(SKILL_SCRIPT, skill.as_bytes())
        }
        Choice::Declined => run(REMOVE_SCRIPT, &[]),
    }
}

impl SheprdWindow {
    /// Applies the user's skill choice to each connected saved SSH host not
    /// tried with it yet, unless the settings keep the app off SSH hosts. Other
    /// clients cannot run host scripts.
    pub(super) fn install_remote_agent_tools(&mut self, cx: &mut Context<Self>) {
        if !cfg!(any(target_os = "linux", target_os = "macos", windows)) {
            return;
        }
        let Some(choice) = AgentSkill::remote_choice(cx) else {
            return;
        };
        for (index, endpoint) in self.endpoints.iter().enumerate() {
            let live = if index == self.selected_endpoint {
                &self.live
            } else {
                &endpoint.live
            };
            let host = match endpoint.saved_target() {
                ConnectTarget::Wsl { distro, .. } => InstallHost::Wsl(distro.clone()),
                target if target.is_remote() => {
                    let Ok(host) = sheprd_remote::teleport::Host::new(target) else {
                        continue;
                    };
                    InstallHost::Remote(host)
                }
                _ => continue,
            };
            // Connected to its Herdr, or to its herdr-core alone.
            let connected = live.snapshot.is_some()
                || self
                    .core_terminals
                    .is_connected(&crate::core_terminals::Host::of(&crate::browser::scope(
                        endpoint,
                    )));
            if !endpoint.enabled || !connected {
                continue;
            }
            if !cx.default_global::<Tried>().first(
                crate::browser::scope(endpoint),
                endpoint.saved_target(),
                choice,
            ) {
                continue;
            }
            cx.background_executor()
                .spawn(async move {
                    match apply(&host, choice) {
                        Ok(()) => tracing::info!(?choice, "Updated the agent tools on an SSH host"),
                        Err(error) => {
                            tracing::warn!(%error, ?choice, "Cannot update the agent tools on an SSH host");
                        }
                    }
                })
                .detach();
        }
    }
}

#[cfg(test)]
mod tests;
