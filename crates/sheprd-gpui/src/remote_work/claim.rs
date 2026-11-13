//! Claiming a remote work request. Desktops driving the same terminal race
//! for it through a versioned core document, whose write herdr-core refuses
//! once another has changed it, so exactly one wins. The claim is then also
//! a label beside the request rather than a rewrite of it, so a caller's
//! cancellation, which removes the request, can never be undone by a claim
//! written after it.

use sheprd_backend::TerminalId;
use sheprd_core_client::CoreBackend;
use std::collections::BTreeMap;

/// The value a claim marker holds; its key names the request it claims.
const CLAIMED: &str = "1";
/// Requests claimed on one terminal that its document remembers. A request
/// nonce is never reused, and a request is answered or abandoned within its
/// timeout, long before this many later ones are claimed beside it.
const RECENT: usize = 64;
/// Versioned writes tried before giving up when other claims keep winning.
const ATTEMPTS: usize = 4;

pub(super) fn start(
    backend: CoreBackend,
    terminal: TerminalId,
    key: String,
    claim: String,
    expected: String,
) -> Option<std::sync::mpsc::Receiver<sheprd_core_client::Result<bool>>> {
    let (send, receive) = std::sync::mpsc::sync_channel(1);
    let spawned = std::thread::Builder::new()
        .name("remote-work-claim".into())
        .spawn(move || {
            let result = (|| {
                let listed = backend.core().list(false)?;
                let Some(labels) = listed
                    .terminals
                    .iter()
                    .find(|info| info.address == terminal.as_str())
                    .map(|info| &info.labels)
                else {
                    return Ok(false);
                };
                if !open(labels, &key, &claim, &expected)
                    || !won(backend.core(), terminal.as_str(), &key)?
                {
                    return Ok(false);
                }
                // The core applies label writes in order and answers with the
                // labels after this one. A request already withdrawn is gone
                // from that answer; one withdrawn later finds the marker.
                let marked = backend.core().label(
                    terminal.as_str(),
                    &BTreeMap::from([(claim.as_str(), CLAIMED)]),
                )?;
                if held(&marked.labels, &key, &claim, &expected) {
                    return Ok(true);
                }
                backend
                    .core()
                    .label(terminal.as_str(), &BTreeMap::from([(claim.as_str(), "")]))?;
                Ok(false)
            })();
            let _ = send.send(result);
        });
    match spawned {
        Ok(_) => Some(receive),
        Err(error) => {
            tracing::warn!(%error, "Could not start remote work claim");
            None
        }
    }
}

/// Records `key` in the terminal's claim document with a versioned write.
/// A write that lost its reply finds its own key next time and reports a
/// loss, so the request goes unanswered rather than running twice.
fn won(
    core: &sheprd_core_client::Core,
    terminal: &str,
    key: &str,
) -> sheprd_core_client::Result<bool> {
    let name = format!("sheprd-work-claims-{terminal}");
    for _ in 0..ATTEMPTS {
        let doc = core.doc_get(&name)?;
        let Some(next) = with_claim(&doc.value, key) else {
            return Ok(false);
        };
        if core.doc_set(&name, &next, doc.version).is_ok() {
            return Ok(true);
        }
    }
    Ok(false)
}

/// The document after claiming `key`, or `None` when it is already claimed.
fn with_claim(value: &serde_json::Value, key: &str) -> Option<serde_json::Value> {
    let mut keys: Vec<&str> = value
        .as_array()
        .map(|keys| keys.iter().filter_map(serde_json::Value::as_str).collect())
        .unwrap_or_default();
    if keys.contains(&key) {
        return None;
    }
    keys.push(key);
    let start = keys.len().saturating_sub(RECENT);
    Some(serde_json::json!(keys[start..]))
}

/// The request still stands as it was read and nothing has claimed it.
fn open(labels: &BTreeMap<String, String>, key: &str, claim: &str, expected: &str) -> bool {
    labels.get(key).is_some_and(|value| value == expected) && !labels.contains_key(claim)
}

/// After marking: the request still stands and the marker is this claim's.
fn held(labels: &BTreeMap<String, String>, key: &str, claim: &str, expected: &str) -> bool {
    labels.get(key).is_some_and(|value| value == expected)
        && labels.get(claim).is_some_and(|value| value == CLAIMED)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests;
