//! Explicit, host-local opt-in installation. No discovery or launch path calls this.

mod error;
pub use error::{Error, Result};

use super::super::AgentKind;
use crate::settings_files;
use serde_json::{Value, json};
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

/// Install owned assets and add their registrations without replacing user hooks.
/// The caller must obtain explicit authorization (the CLI's session-install command).
pub fn run(agent: AgentKind, home: &Path, env: impl Fn(&str) -> Option<OsString>) -> Result<()> {
    if !cfg!(any(target_os = "linux", target_os = "macos", windows)) {
        return Err(Error::UnsupportedPlatform);
    }
    let directory = match agent {
        AgentKind::Claude => env("CLAUDE_CONFIG_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".claude")),
        AgentKind::Codex => env("CODEX_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".codex")),
        AgentKind::Copilot => env("COPILOT_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".copilot")),
        AgentKind::Opencode => env("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".config"))
            .join("opencode"),
        AgentKind::Pi => env("PI_CODING_AGENT_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".pi/agent")),
        AgentKind::Omp => env("PI_CODING_AGENT_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                home.join(env("PI_CONFIG_DIR").unwrap_or_else(|| ".omp".into()))
                    .join("agent")
            }),
        AgentKind::Letta => home.join(".letta"),
        _ => return Err(Error::UnsupportedAgent),
    };
    path_text(&directory)?;
    let owned = directory.join("sheprd-session-report");
    let program = env("SHEPRD_ACP_BINARY").map(PathBuf::from);
    if let Some(program) = &program {
        path_text(program)?;
    }
    let mut writes = Vec::new();
    match agent {
        AgentKind::Claude | AgentKind::Codex | AgentKind::Copilot | AgentKind::Letta => {
            let codex = agent == AgentKind::Codex;
            let (name, source) = if codex {
                (
                    "codex-session-start.sh",
                    include_str!("assets/codex-session-start.sh"),
                )
            } else if agent == AgentKind::Claude {
                (
                    "claude-session-start.sh",
                    include_str!("assets/claude-session-start.sh"),
                )
            } else if agent == AgentKind::Copilot {
                (
                    "copilot-session-start.sh",
                    include_str!("assets/copilot-session-start.sh"),
                )
            } else {
                (
                    "letta-session-start.sh",
                    include_str!("assets/letta-session-start.sh"),
                )
            };
            let mut script = owned.join(name);
            let mut source = source.to_owned();
            let command = if cfg!(windows) {
                script.set_extension("ps1");
                source =
                    include_str!("assets/session-start.ps1").replace("__AGENT__", agent.name());
                if let Some(program) = &program {
                    source = source.replace(
                        "'sheprd-acp'",
                        &format!("'{}'", path_text(program)?.replace('\'', "''")),
                    );
                }
                format!(
                    "powershell.exe -NoProfile -NonInteractive -File \"{}\"",
                    path_text(&script)?
                )
            } else {
                match &program {
                    Some(program) => format!(
                        "env SHEPRD_ACP_BINARY={} sh {}",
                        crate::shell_quote(path_text(program)?),
                        crate::shell_quote(path_text(&script)?)
                    ),
                    None => format!("sh {}", crate::shell_quote(path_text(&script)?)),
                }
            };
            let registration = if codex || agent == AgentKind::Claude {
                json!({"hooks": [{"type": "command", "command": command, "timeout": 5}]})
            } else if agent == AgentKind::Letta {
                json!({"hooks": [{"type":"command","command":command,"timeout":5000,"quiet":true}]})
            } else {
                let shell = if cfg!(windows) { "powershell" } else { "bash" };
                json!({"type": "command", shell: command, "timeoutSec": 5})
            };
            let config = directory.join(if codex { "hooks.json" } else { "settings.json" });
            let mut value = read_json(&config)?;
            let hooks = object(&mut value, &config)?
                .entry("hooks")
                .or_insert_with(|| json!({}));
            let starts = object(hooks, &config)?
                .entry("SessionStart")
                .or_insert_with(|| json!([]));
            replace_hook(starts, registration, &script, &config)?;
            writes.push((script, source));
            writes.push((config.clone(), encode(&value, &config)?));
            if codex {
                let config = directory.join("config.toml");
                let mut document = read(&config)?
                    .unwrap_or_default()
                    .parse::<toml_edit::DocumentMut>()
                    .map_err(|source| Error::Toml {
                        path: config.clone(),
                        source,
                    })?;
                if !document.contains_key("features") {
                    document["features"] = toml_edit::Item::Table(toml_edit::Table::new());
                }
                let features =
                    document["features"]
                        .as_table_like_mut()
                        .ok_or_else(|| Error::Shape {
                            path: config.clone(),
                            expected: "a features table",
                        })?;
                features.insert("hooks", toml_edit::value(true));
                writes.push((config, document.to_string()));
            }
        }
        AgentKind::Opencode => {
            let config = directory.join("cli.json");
            let mut value = read_json(&config)?;
            let plugins = object(&mut value, &config)?
                .entry("plugins")
                .or_insert_with(|| json!([]));
            append(plugins, json!({"package": path_text(&owned)?}), &config)?;
            for (name, source) in [
                ("package.json", include_str!("assets/opencode/package.json")),
                ("reporter.mjs", include_str!("assets/opencode/reporter.mjs")),
                ("tui.mjs", include_str!("assets/opencode/tui.mjs")),
            ] {
                let source = if name == "reporter.mjs" {
                    match &program {
                        Some(program) => source.replacen(
                            "\"sheprd-acp\"",
                            encode(&json!(path_text(program)?), &owned)?.trim(),
                            1,
                        ),
                        None => source.to_owned(),
                    }
                } else {
                    source.to_owned()
                };
                writes.push((owned.join(name), source));
            }
            writes.push((config.clone(), encode(&value, &config)?));
        }
        AgentKind::Pi | AgentKind::Omp => {
            let source = include_str!("assets/extension.mjs").replace("__AGENT__", agent.name());
            let source = if let Some(program) = &program {
                source.replacen(
                    "\"sheprd-acp\"",
                    encode(&json!(path_text(program)?), &owned)?.trim(),
                    1,
                )
            } else {
                source
            };
            writes.push((
                directory
                    .join("extensions")
                    .join(format!("sheprd-{}-session.ts", agent.name())),
                source,
            ));
        }
        _ => return Err(Error::UnsupportedAgent),
    }
    // Validate every document before touching any file. Each replacement is atomic
    // and private; a failed multi-file install can be retried idempotently.
    for (path, text) in writes {
        settings_files::replace(&path, &text).map_err(|source| Error::Io { path, source })?;
    }
    Ok(())
}

fn path_text(path: &Path) -> Result<&str> {
    if !path.is_absolute() {
        return Err(Error::Path(path.to_owned()));
    }
    path.to_str().ok_or_else(|| Error::Path(path.to_owned()))
}

fn read(path: &Path) -> Result<Option<String>> {
    settings_files::read(path).map_err(|source| Error::Io {
        path: path.to_owned(),
        source,
    })
}

fn read_json(path: &Path) -> Result<Value> {
    match read(path)? {
        Some(text) => serde_json::from_str(&text).map_err(|source| Error::Json {
            path: path.to_owned(),
            source,
        }),
        None => Ok(json!({})),
    }
}

fn object<'a>(value: &'a mut Value, path: &Path) -> Result<&'a mut serde_json::Map<String, Value>> {
    value.as_object_mut().ok_or_else(|| Error::Shape {
        path: path.to_owned(),
        expected: "a JSON object",
    })
}

fn append(value: &mut Value, entry: Value, path: &Path) -> Result<()> {
    let array = value.as_array_mut().ok_or_else(|| Error::Shape {
        path: path.to_owned(),
        expected: "a JSON array",
    })?;
    if !array.contains(&entry) {
        array.push(entry);
    }
    Ok(())
}

fn replace_hook(value: &mut Value, entry: Value, script: &Path, path: &Path) -> Result<()> {
    let array = value.as_array_mut().ok_or_else(|| Error::Shape {
        path: path.to_owned(),
        expected: "a JSON array",
    })?;
    let script = path_text(script)?;
    let posix = format!("sh {}", crate::shell_quote(script));
    let windows = format!("powershell.exe -NoProfile -NonInteractive -File \"{script}\"");
    let owned = |hook: &Value| {
        ["command", "bash", "powershell"].iter().any(|key| {
            hook.get(key)
                .and_then(Value::as_str)
                .is_some_and(|command| {
                    command == posix
                        || command == windows
                        || (command.starts_with("env SHEPRD_ACP_BINARY=")
                            && command.ends_with(&posix))
                })
        })
    };
    array.retain_mut(|group| {
        if owned(group) {
            return false;
        }
        if let Some(hooks) = group.get_mut("hooks").and_then(Value::as_array_mut) {
            let had_owned = hooks.iter().any(&owned);
            hooks.retain(|hook| !owned(hook));
            return !had_owned || !hooks.is_empty();
        }
        true
    });
    array.push(entry);
    Ok(())
}

fn encode(value: &Value, path: &Path) -> Result<String> {
    serde_json::to_string_pretty(value)
        .map(|text| text + "\n")
        .map_err(|source| Error::Json {
            path: path.to_owned(),
            source,
        })
}

#[cfg(test)]
mod tests;
