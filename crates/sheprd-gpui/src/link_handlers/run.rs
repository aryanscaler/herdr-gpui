use super::{Error, Handler};
use sheprd_client::{ConnectTarget, ScriptHost, ScriptLimits, SshLane, shell_quote};
use std::{sync::atomic::AtomicBool, time::Duration};

const LIMITS: ScriptLimits = ScriptLimits {
    output: 64 * 1024,
    idle: Duration::from_secs(30),
};

// Never inherit the GUI's caller pane/session: it may belong to Herdr 1 or
// another core. A compatible adapter receives data, not invented H1 IDs.
const REMOVE: &[&str] = &[
    "HERDR_API_SOCKET",
    "HERDR_SOCKET_PATH",
    "HERDR_CLIENT_SOCKET_PATH",
    "HERDR_SESSION",
    "HERDR_ENV",
    "HERDR_PANE_ID",
    "HERDR_TAB_ID",
    "HERDR_WORKSPACE_ID",
    "HERDR_PLUGIN_ACTION_ID",
    "HERDR_PLUGIN_CONFIG_DIR",
    "HERDR_PLUGIN_STATE_DIR",
    "HERDR_PLUGIN_EVENT",
    "HERDR_PLUGIN_EVENT_JSON",
    "HERDR_BIN_PATH",
    "HC_SOCKET",
    "HC_SESSION",
    "HC_TERMINAL_ID",
    "HC_TERMINAL_SOCKET",
];

fn environment(
    handler: &Handler,
    url: &str,
    terminal: &str,
    cwd: Option<&str>,
) -> Vec<(&'static str, String)> {
    let mut values = vec![
        ("HERDR_PLUGIN_ROOT", handler.directory.clone()),
        ("HERDR_PLUGIN_ID", handler.plugin.clone()),
        ("HERDR_PLUGIN_LINK_HANDLER_ID", handler.id.clone()),
        ("HERDR_PLUGIN_CLICKED_URL", url.to_owned()),
        ("SHEPRD_LINK_TERMINAL_ID", terminal.to_owned()),
        ("SHEPRD_LINK_HOST", handler.host.clone()),
        (
            "HERDR_PLUGIN_CONTEXT_JSON",
            serde_json::json!({
                "invocation_source": "link_click", "clicked_url": url,
                "link_handler_id": handler.id,
                "focused_pane_cwd": cwd,
            })
            .to_string(),
        ),
    ];
    for (key, value) in [
        ("HERDR_PLUGIN_ACTION_ID", &handler.action),
        ("HERDR_PLUGIN_CONFIG_DIR", &handler.config_directory),
        ("HERDR_PLUGIN_STATE_DIR", &handler.state_directory),
    ] {
        if let Some(value) = value {
            values.push((key, value.clone()));
        }
    }
    values
}

fn script(handler: &Handler, url: &str, terminal: &str, cwd: Option<&str>) -> String {
    let mut script = format!("set -eu\nunset {}\n", REMOVE.join(" "));
    for (key, value) in environment(handler, url, terminal, cwd) {
        script.push_str(&format!("export {key}={}\n", shell_quote(&value)));
    }
    script.push_str(&format!(
        "cd {}\nexec {}\n",
        shell_quote(&handler.directory),
        handler
            .command
            .iter()
            .map(|word| shell_quote(word))
            .collect::<Vec<_>>()
            .join(" ")
    ));
    script
}

/// Blocking and bounded. A failure spends the click, never falls back/replays.
pub(crate) fn run(
    handler: &Handler,
    target: &ConnectTarget,
    url: &str,
    terminal: &str,
    cwd: Option<&str>,
    cancelled: &AtomicBool,
) -> Result<(), Error> {
    if !target.is_remote() {
        let program = std::path::Path::new(&handler.command[0]);
        let program = if program.is_relative() && program.components().count() > 1 {
            std::path::Path::new(&handler.directory).join(program)
        } else {
            program.to_owned()
        };
        let mut command = sheprd_client::new_command(program);
        command
            .args(&handler.command[1..])
            .current_dir(&handler.directory);
        for key in REMOVE {
            command.env_remove(key);
        }
        command.envs(environment(handler, url, terminal, cwd));
        sheprd_client::run_script_command(
            command,
            std::io::empty(),
            std::io::sink(),
            LIMITS,
            cancelled,
        )?;
        return Ok(());
    }
    remote_script(
        target,
        &script(handler, url, terminal, cwd),
        std::io::sink(),
        LIMITS,
        cancelled,
    )
}

pub(super) fn remote_script(
    target: &ConnectTarget,
    text: &str,
    output: impl std::io::Write + Send,
    limits: ScriptLimits,
    cancelled: &AtomicBool,
) -> Result<(), Error> {
    let host = match target {
        ConnectTarget::Local | ConnectTarget::Session { .. } | ConnectTarget::Socket(_) => {
            return Err(Error::HostChanged);
        }
        ConnectTarget::Ssh { target, .. } => ScriptHost::Ssh(target, SshLane::Bulk),
        ConnectTarget::Wsl { distro, .. } => ScriptHost::Wsl(distro),
        #[cfg(feature = "cloud")]
        ConnectTarget::Cloud {
            provider,
            account,
            id,
            ..
        } => {
            sheprd_remote::cloud::with_command(
                *provider,
                account,
                id,
                cancelled,
                |mut command| {
                    command.arg(format!("/bin/sh -c {}", shell_quote(text)));
                    sheprd_client::run_script_command(
                        command,
                        std::io::empty(),
                        output,
                        limits,
                        cancelled,
                    )
                },
            )??;
            return Ok(());
        }
    };
    sheprd_client::run_script(host, text, std::io::empty(), output, limits, cancelled)?;
    Ok(())
}

#[cfg(all(test, unix))]
#[path = "tests/transport.rs"]
mod tests;
