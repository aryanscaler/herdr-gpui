//! Revalidate both generations after the potentially long repository transfer.
//! These reads narrow the race; neither daemon offers an atomic session-bound
//! stop, so they cannot fence a replacement between the last read and retirement.

use super::*;
use crate::teleport::snapshot::SnapshotResult;

pub(super) fn sessions(host: &Host, review: &Review, cancelled: &AtomicBool) -> Result<()> {
    for generation in [Generation::Herdr1, Generation::Herdr2] {
        let expected: Vec<&PanePlan> = review
            .tabs
            .iter()
            .filter(|tab| tab.generation == generation)
            .flat_map(|tab| &tab.panes)
            .filter(|pane| matches!(pane.work, Work::Session { .. }))
            .collect();
        if expected.is_empty() {
            continue;
        }
        let ids: Vec<String> = expected.iter().map(|pane| pane.pane_id.clone()).collect();
        match generation {
            Generation::Herdr1 => {
                let snapshot: SnapshotResult =
                    host.herdr(Step::Review, &["api", "snapshot"], cancelled)?;
                let processes = plan::process_infos(host, &ids, cancelled)?;
                for expected in expected {
                    let pane = snapshot
                        .snapshot
                        .pane(&expected.pane_id)
                        .ok_or(Error::SourceSessionChanged)?;
                    let process = processes
                        .get(&expected.pane_id)
                        .ok_or(Error::SourceSessionChanged)?;
                    if !matches!(
                        pane.status,
                        crate::teleport::snapshot::PaneStatus::Idle
                            | crate::teleport::snapshot::PaneStatus::Done
                    ) {
                        return Err(Error::SessionAgentWorking);
                    }
                    let cwd = process
                        .foreground_job()
                        .and_then(|job| job.cwd.as_deref())
                        .or(pane.foreground_cwd.as_deref())
                        .or(pane.cwd.as_deref());
                    unchanged(expected, &Work::classify(pane, process), cwd)?;
                }
            }
            Generation::Herdr2 => {
                let terminals = core::terminals(host, &ids, cancelled)?;
                let sessions = core::sessions(host, &terminals, cancelled)?;
                for expected in expected {
                    let terminal = terminals
                        .get(&expected.pane_id)
                        .ok_or(Error::SourceSessionChanged)?;
                    if !terminal.idle() {
                        return Err(Error::SessionAgentWorking);
                    }
                    unchanged(
                        expected,
                        &core::work(terminal, sessions.sessions.get(&expected.pane_id)),
                        terminal.cwd().as_deref(),
                    )?;
                    if expected.binding.as_ref() != sessions.bindings.get(&expected.pane_id) {
                        return Err(Error::SourceSessionChanged);
                    }
                }
            }
        }
    }
    Ok(())
}

fn unchanged(expected: &PanePlan, work: &Work, cwd: Option<&str>) -> Result<()> {
    if expected.work != *work || expected.cwd.as_deref() != cwd {
        return Err(Error::SourceSessionChanged);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
