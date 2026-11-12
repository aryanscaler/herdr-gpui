//! Explicit app-owned adapters for link-handler commands. No daemon registry
//! or terminal-provided command is imported. Only a completed click calls `run`.

mod error;
mod plugins;
mod run;
pub(crate) use error::Error;
pub(crate) use plugins::{Plugin, activate, validate_plugins};
pub(crate) use run::run;

use regex::Regex;
use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Handler {
    #[serde(default)]
    pub(crate) platforms: Option<Vec<Platform>>,
    /// Original manifest action ID, when the adapter uses it.
    #[serde(default)]
    pub(crate) action: Option<String>,
    #[serde(default)]
    pub(crate) config_directory: Option<String>,
    pub(crate) plugin: String,
    pub(crate) id: String,
    /// Saved device scope, or `local`. Never a wildcard.
    pub(crate) host: String,
    pub(crate) pattern: String,
    pub(crate) command: Vec<String>,
    /// Trusted plugin directory on the host, not the terminal cwd.
    pub(crate) directory: String,
    #[serde(default)]
    pub(crate) state_directory: Option<String>,
    #[serde(default = "enabled")]
    pub(crate) enabled: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Platform {
    Linux,
    Macos,
    Windows,
}

impl Handler {
    fn supports(&self, platform: Platform) -> bool {
        self.platforms
            .as_ref()
            .is_none_or(|platforms| platforms.contains(&platform))
    }
}

fn enabled() -> bool {
    true
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Handlers(Vec<(Handler, Regex)>);

impl Handlers {
    pub(crate) fn compile(mut handlers: Vec<Handler>) -> Result<Self, Error> {
        if handlers.len() > 64 {
            return Err(Error::Invalid("at most 64 handlers are allowed"));
        }
        // Stable sorting preserves manifest order inside each plugin.
        handlers.sort_by(|a, b| a.plugin.cmp(&b.plugin));
        let mut seen = std::collections::HashSet::new();
        let mut compiled = Vec::new();
        for handler in handlers {
            if [&handler.plugin, &handler.id, &handler.host]
                .into_iter()
                .any(|s| s.is_empty() || s.len() > 256 || s.chars().any(char::is_control))
                || !absolute_directory(&handler.directory)
                || handler.directory.len() > 4096
                || handler.directory.chars().any(char::is_control)
                || handler.command.is_empty()
                || handler.command.len() > 64
                || handler.command[0].is_empty()
                || handler
                    .command
                    .iter()
                    .any(|s| s.len() > 8192 || s.contains('\0'))
                || handler.pattern.len() > 4096
                || handler.action.as_ref().is_some_and(|action| {
                    action.is_empty() || action.len() > 256 || action.chars().any(char::is_control)
                })
                || [&handler.config_directory, &handler.state_directory]
                    .into_iter()
                    .flatten()
                    .any(|path| {
                        !absolute_directory(path)
                            || path.len() > 4096
                            || path.chars().any(char::is_control)
                    })
            {
                return Err(Error::Invalid(
                    "invalid identity, directory, command, or pattern bounds",
                ));
            }
            if !seen.insert((
                handler.host.clone(),
                handler.plugin.clone(),
                handler.id.clone(),
            )) {
                return Err(Error::Invalid("duplicate handler ID for a plugin and host"));
            }
            let pattern = regex::RegexBuilder::new(&handler.pattern)
                .size_limit(256 * 1024)
                .build()?;
            compiled.push((handler, pattern));
        }
        Ok(Self(compiled))
    }

    pub(crate) fn matching(&self, host: &str, url: &str) -> Option<&Handler> {
        if url.is_empty() || url.len() > 32768 || url.chars().any(char::is_control) {
            return None;
        }
        self.0
            .iter()
            .find(|(handler, pattern)| {
                handler.enabled && handler.host == host && pattern.is_match(url)
            })
            .map(|(handler, _)| handler)
    }
}

fn absolute_directory(path: &str) -> bool {
    path.starts_with('/')
        || (path.as_bytes().get(1) == Some(&b':')
            && path.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)
            && matches!(path.as_bytes().get(2), Some(b'/' | b'\\')))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests;
