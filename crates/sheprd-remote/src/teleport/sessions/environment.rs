//! Configuration roots belong to their hosts. Map home/checkouts explicitly,
//! export only the evidence allowlist, and never copy credentials or config.

use super::*;
use sheprd_client::herdr::sessions::Binding;
use std::collections::BTreeMap;

#[derive(Default)]
pub(crate) struct Environment {
    pub(super) source: BTreeMap<String, String>,
    pub(super) destination: BTreeMap<String, String>,
}

impl Environment {
    pub(in crate::teleport) fn map(
        source: &Host,
        destination: &Host,
        binding: Option<&Binding>,
        from: &str,
        to: &str,
        cancelled: &AtomicBool,
    ) -> Result<Self> {
        let Some(binding) = binding.filter(|binding| !binding.environment.is_empty()) else {
            return Ok(Self::default());
        };
        let agent = AgentKind::parse(&binding.agent).ok_or(Error::SourceSessionChanged)?;
        binding
            .validate(&binding.identity, agent)
            .map_err(|_| Error::SourceSessionChanged)?;
        let home = |host: &Host| -> Result<String> {
            let output = host.query(Step::Sessions, "printf '%s' \"$HOME\"\n", &[], cancelled)?;
            let home = String::from_utf8(output).map_err(|_| Error::SessionConfiguration {
                agent: agent.binary(),
            })?;
            if !home.starts_with('/') || home.chars().any(char::is_control) {
                return Err(Error::SessionConfiguration {
                    agent: agent.binary(),
                });
            }
            Ok(home)
        };
        let from_home = home(source)?;
        let to_home = home(destination)?;
        let destination_environment =
            map_roots(agent, &binding.environment, from, to, &from_home, &to_home)?;
        if let Some(config) = destination_environment.get("OPENCODE_CONFIG") {
            let present = destination.query(
                Step::Sessions,
                &format!("if [ -f {} ]; then printf yes; fi\n", shell_quote(config)),
                &[],
                cancelled,
            )?;
            if present != b"yes" {
                return Err(Error::SessionConfiguration {
                    agent: agent.binary(),
                });
            }
        }
        Ok(Self {
            source: binding.environment.clone(),
            destination: destination_environment,
        })
    }

    pub(in crate::teleport) fn resume(
        &self,
        agent: AgentKind,
        reference: &str,
        flags: &[String],
        from: &str,
        to: &str,
    ) -> Vec<String> {
        let argv = crate::teleport::launch::resume_command_with(agent, reference, flags, |path| {
            crate::teleport::launch::remap(path, from, to).or_else(|| {
                self.source
                    .iter()
                    .filter_map(|(key, source)| {
                        let destination = self.destination.get(key)?;
                        crate::teleport::launch::remap(path, source, destination)
                            .map(|mapped| (source.len(), mapped))
                    })
                    .max_by_key(|(length, _)| *length)
                    .map(|(_, mapped)| mapped)
            })
        });
        if self.destination.is_empty() {
            return argv;
        }
        std::iter::once("env".into())
            .chain(
                self.destination
                    .iter()
                    .map(|(key, value)| format!("{key}={value}")),
            )
            .chain(argv)
            .collect()
    }
}

fn map_roots(
    agent: AgentKind,
    environment: &BTreeMap<String, String>,
    from: &str,
    to: &str,
    from_home: &str,
    to_home: &str,
) -> Result<BTreeMap<String, String>> {
    environment
        .iter()
        .map(|(key, value)| {
            // PI_CONFIG_DIR has distinct semantics across pi/omp versions; until
            // its storage contract is confirmed, never substitute a guessed root.
            if key == "PI_CONFIG_DIR" {
                return Err(Error::SessionConfiguration {
                    agent: agent.binary(),
                });
            }
            // `remap` matches a textual prefix: `$HOME/../etc` would pass it
            // and still name a path outside the destination home.
            if !plain(value) {
                return Err(Error::SessionConfiguration {
                    agent: agent.binary(),
                });
            }
            let mapped = crate::teleport::launch::remap(value, from, to)
                .or_else(|| crate::teleport::launch::remap(value, from_home, to_home))
                .ok_or(Error::SessionConfiguration {
                    agent: agent.binary(),
                })?;
            Ok((key.clone(), mapped))
        })
        .collect()
}

/// An absolute path with no `.`, `..`, or empty segments.
fn plain(path: &str) -> bool {
    path.strip_prefix('/').is_some_and(|rest| {
        rest.trim_end_matches('/')
            .split('/')
            .all(|segment| !matches!(segment, "" | "." | ".."))
    })
}

pub(super) fn script(body: &str, environment: Option<&BTreeMap<String, String>>) -> String {
    let mut script = String::new();
    for (key, value) in environment.into_iter().flatten() {
        script.push_str(&format!("export {key}={}\n", shell_quote(value)));
    }
    script.push_str(body);
    script
}

#[cfg(test)]
mod tests;
