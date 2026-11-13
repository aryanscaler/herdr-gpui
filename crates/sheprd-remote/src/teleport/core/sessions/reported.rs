//! The helper validates records against live process birth on its own host.
//! Its returned binding is retained, not flattened into an unscoped session ID.

use super::*;

pub(super) fn discover(
    host: &Host,
    terminals: &HashMap<String, Terminal>,
    cancelled: &AtomicBool,
) -> Result<(Discovered, Vec<String>)> {
    let mut found = Discovered::default();
    let mut unavailable = Vec::new();
    let mut ids = terminals.keys().collect::<Vec<_>>();
    ids.sort();
    for id in ids {
        let terminal = &terminals[id];
        let Work::Agent { name, .. } = terminal.work() else {
            continue;
        };
        let Some(agent) = AgentKind::parse(&name).filter(|agent| agent.carries()) else {
            continue;
        };
        let Some(pid) = terminal.foreground.as_ref().and_then(|front| front.pid) else {
            continue;
        };
        let output = match host.query(
            Step::Review,
            &host.session_discovery(id, agent.name()),
            &[],
            cancelled,
        ) {
            Ok(output) => output,
            Err(Error::Script {
                source: sheprd_client::Error::ScriptExit { status, .. },
                ..
            }) if status.code() == Some(3) => continue,
            Err(Error::Script {
                source: sheprd_client::Error::ScriptExit { status, .. },
                ..
            }) if status.code() == Some(127) => {
                unavailable.push(id.clone());
                continue;
            }
            Err(error) => return Err(error),
        };
        let binding = validate(&output, id, pid, agent)?;
        if let Some(session) = binding.selected() {
            found.sessions.insert(id.clone(), session.clone());
        }
        found.bindings.insert(id.clone(), binding);
    }
    Ok((found, unavailable))
}

fn validate(output: &[u8], id: &str, pid: u32, agent: AgentKind) -> Result<Binding> {
    if output.len() > sheprd_client::herdr::sessions::MAX_RECORD {
        return Err(Error::SourceSessionChanged);
    }
    let binding: Binding =
        serde_json::from_slice(output).map_err(crate::teleport::error::decode(Step::Review))?;
    if binding.identity.terminal != id
        || binding.identity.pid != pid
        || binding.validate(&binding.identity, agent).is_err()
    {
        return Err(Error::SourceSessionChanged);
    }
    Ok(binding)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests;
