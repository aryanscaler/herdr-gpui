//! Opt-in manifest adapters. Only declared link actions are read; build,
//! startup, event hooks and daemon API claims are never imported or executed.
use super::{Error, Handler, Handlers, Platform, absolute_directory};
use serde::Deserialize;
use sheprd_client::{ConnectTarget, ScriptLimits, shell_quote};
use std::{io::Read, sync::atomic::AtomicBool, time::Duration};

const MAX_MANIFEST: u64 = 256 * 1024;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Plugin {
    pub(crate) host: String,
    pub(crate) directory: String,
    #[serde(default = "super::enabled")]
    pub(crate) enabled: bool,
    #[serde(default)]
    pub(crate) config_directory: Option<String>,
    #[serde(default)]
    pub(crate) state_directory: Option<String>,
}

pub(crate) fn validate_plugins(plugins: &[Plugin]) -> Result<(), Error> {
    if plugins.len() > 16 {
        return Err(Error::Invalid("at most 16 manifest adapters are allowed"));
    }
    for plugin in plugins {
        if plugin.host.is_empty()
            || plugin.host.len() > 256
            || plugin.host.chars().any(char::is_control)
            || [
                &Some(plugin.directory.clone()),
                &plugin.config_directory,
                &plugin.state_directory,
            ]
            .into_iter()
            .flatten()
            .any(|path| {
                !absolute_directory(path) || path.len() > 4096 || path.chars().any(char::is_control)
            })
        {
            return Err(Error::Invalid("invalid manifest adapter host or directory"));
        }
    }
    Ok(())
}

#[derive(Deserialize)]
struct Manifest {
    id: String,
    platforms: Option<Vec<Platform>>,
    #[serde(default)]
    actions: Vec<Action>,
    #[serde(default)]
    link_handlers: Vec<Link>,
}

#[derive(Deserialize)]
struct Action {
    id: String,
    command: Vec<String>,
    platforms: Option<Vec<Platform>>,
}

#[derive(Deserialize)]
struct Link {
    id: String,
    pattern: String,
    action: String,
    platforms: Option<Vec<Platform>>,
}

fn manifest(plugin: &Plugin, text: &str) -> Result<Vec<Handler>, Error> {
    let manifest: Manifest = toml::from_str(text)?;
    if manifest.actions.len() > 64 || manifest.link_handlers.len() > 64 {
        return Err(Error::Invalid(
            "at most 64 actions and handlers per manifest",
        ));
    }
    let mut ids = std::collections::HashSet::new();
    if manifest
        .actions
        .iter()
        .any(|action| !ids.insert(&action.id))
    {
        return Err(Error::Invalid("duplicate manifest action ID"));
    }
    manifest
        .link_handlers
        .into_iter()
        .map(|link| {
            let action = manifest
                .actions
                .iter()
                .find(|action| action.id == link.action)
                .ok_or(Error::Invalid("link handler references a missing action"))?;
            let handler_platforms = link.platforms.as_ref().or(manifest.platforms.as_ref());
            let action_platforms = action.platforms.as_ref().or(manifest.platforms.as_ref());
            let platforms = [Platform::Linux, Platform::Macos, Platform::Windows]
                .into_iter()
                .filter(|platform| {
                    handler_platforms.is_none_or(|list| list.contains(platform))
                        && action_platforms.is_none_or(|list| list.contains(platform))
                })
                .collect();
            Ok(Handler {
                plugin: manifest.id.clone(),
                id: link.id,
                host: plugin.host.clone(),
                pattern: link.pattern,
                action: Some(action.id.clone()),
                command: action.command.clone(),
                directory: plugin.directory.clone(),
                config_directory: plugin.config_directory.clone(),
                state_directory: plugin.state_directory.clone(),
                enabled: plugin.enabled,
                platforms: Some(platforms),
            })
        })
        .collect()
}

fn read(plugin: &Plugin, target: &ConnectTarget, cancelled: &AtomicBool) -> Result<String, Error> {
    let mut bytes = Vec::new();
    if target.is_remote() {
        let path = format!(
            "{}/herdr-plugin.toml",
            plugin.directory.trim_end_matches('/')
        );
        super::run::remote_script(
            target,
            &format!("exec cat {}", shell_quote(&path)),
            &mut bytes,
            ScriptLimits {
                output: MAX_MANIFEST,
                idle: Duration::from_secs(10),
            },
            cancelled,
        )?;
    } else {
        let mut options = std::fs::OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(rustix::fs::OFlags::NONBLOCK.bits() as i32);
        }
        let file =
            options.open(std::path::Path::new(&plugin.directory).join("herdr-plugin.toml"))?;
        if !file.metadata()?.is_file() {
            return Err(Error::Invalid("manifest must be a regular file"));
        }
        file.take(MAX_MANIFEST + 1).read_to_end(&mut bytes)?;
    }
    if bytes.len() as u64 > MAX_MANIFEST {
        return Err(Error::Invalid("manifest exceeds 256 KiB"));
    }
    String::from_utf8(bytes).map_err(Error::Utf8)
}

fn platform(target: &ConnectTarget, cancelled: &AtomicBool) -> Result<Platform, Error> {
    if !target.is_remote() {
        return Ok(if cfg!(windows) {
            Platform::Windows
        } else if cfg!(target_os = "macos") {
            Platform::Macos
        } else {
            Platform::Linux
        });
    }
    let mut output = Vec::new();
    super::run::remote_script(
        target,
        "exec uname -s",
        &mut output,
        ScriptLimits {
            output: 64,
            idle: Duration::from_secs(10),
        },
        cancelled,
    )?;
    match output.as_slice() {
        b"Linux\n" => Ok(Platform::Linux),
        b"Darwin\n" => Ok(Platform::Macos),
        _ => Err(Error::Invalid("unsupported remote handler platform")),
    }
}

/// True only once a supported first-match action was run. False is a safe
/// fallback: no command has started. Read/probe/run errors spend the click.
pub(crate) fn activate(
    handlers: &Handlers,
    plugins: &[Plugin],
    host: &str,
    target: &ConnectTarget,
    click: &crate::core_terminals::LinkClick,
    cancelled: &AtomicBool,
) -> Result<bool, Error> {
    let Some(url) = click.handler_url.as_deref() else {
        return Ok(false);
    };
    let mut all: Vec<Handler> = handlers
        .0
        .iter()
        .filter(|(handler, _)| handler.host == host)
        .map(|(handler, _)| handler.clone())
        .collect();
    for plugin in plugins
        .iter()
        .filter(|plugin| plugin.enabled && plugin.host == host)
    {
        all.extend(manifest(plugin, &read(plugin, target, cancelled)?)?);
    }
    let all = Handlers::compile(all)?;
    if all.matching(host, url).is_none() {
        return Ok(false);
    }
    let platform = platform(target, cancelled)?;
    let handler = all.0.iter().find(|(handler, pattern)| {
        handler.enabled && handler.supports(platform) && pattern.is_match(url)
    });
    let Some((handler, _)) = handler else {
        return Ok(false);
    };
    super::run(
        handler,
        target,
        url,
        click.terminal.as_ref(),
        click.cwd.as_deref(),
        cancelled,
    )?;
    Ok(true)
}

#[cfg(test)]
#[path = "tests/manifests.rs"]
mod tests;
