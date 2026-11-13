//! Explicit receipt recovery and restart. Neither operation retries an
//! ambiguous initial prompt; only an explicit restart sends the saved prompt.
use super::*;

pub(super) fn recover(site: &Site, id: &str, cancelled: &AtomicBool) -> Result<()> {
    let (mut store, run, mut session, host) = own_run(site, id, cancelled)?;
    let work::Started::Herdr2 { terminal } =
        herdr2::inspect(&host, &run, &session, Purpose::End, cancelled)?
    else {
        return Err(Error::NothingToOpen);
    };
    session.pane = Pane::Herdr2 { terminal };
    session.updated_at = Utc::now();
    store.update_run(&run, Some(&session))
}

pub(super) fn restart(site: &Site, id: &str, cancelled: &AtomicBool) -> Result<()> {
    // Not `own_run`: a pending agent is inspected below, where an interrupted
    // restart that never launched it can still be proven gone.
    let (mut store, mut run, session) = owned(site, id)?;
    let mut session = session.ok_or_else(|| Error::RunNotFound(id.to_owned()))?;
    let host = host_of(site, &session)?;
    if matches!(session.pane, Pane::Herdr1 { .. }) {
        return Err(Error::NothingToOpen);
    }
    let prompt = store.prompt(id)?;
    let previous = match herdr2::inspect(&host, &run, &session, Purpose::End, cancelled) {
        Ok(work::Started::Herdr2 { terminal }) => Some(terminal),
        Ok(_) => return Err(Error::NothingToOpen),
        // A restart stopped after ending the previous agent, or before the
        // next one ran, leaves nothing to end. Continue it only on proof.
        Err(error) => {
            if !herdr2::gone(&host, &run, &session, cancelled)? {
                return Err(error);
            }
            None
        }
    };
    let workspace = run.workspace.as_ref().ok_or(Error::NothingToOpen)?;
    let path = workspace
        .path
        .as_ref()
        .ok_or(Error::NothingToOpen)?
        .to_string_lossy()
        .into_owned();
    let identity = work::identity(&host, &path, cancelled)?;
    if !identity.linked || identity.branch.as_deref() != Some(&workspace.branch) {
        return Err(Error::InvalidCheckout);
    }
    let next = agent_name();
    let tree = Worktree {
        path,
        opened: Opened::Herdr2,
    };
    if let Some(token) = &session.checkout_token {
        work::owned_checkout::verify(&host, &tree, &workspace.branch, token, cancelled)?;
    }
    // perform() holds the cross-process action lease. Keep the old receipt
    // until its program is ended, so a crash cannot orphan a live agent.
    let outcome = (|| {
        if let Some(previous) = &previous {
            herdr2::end(&host, previous)?;
        }
        store.claim_restart(id, &session.agent_name, &next)?;
        session.adopted = None;
        session.agent_name = next.clone();
        session.pane = Pane::Pending;
        run.session_id = Some(next.clone());
        run.state = RunState::Starting;
        run.updated_at = Utc::now();
        store.update_run(&run, Some(&session))?;
        let agent = work::Agent {
            kind: AgentKind::parse(&run.agent).ok_or(Error::NothingToOpen)?,
            name: &next,
            model: run.model.as_deref(),
            prompt: None,
        };
        let started =
            work::owned_agent::start(&host, &tree, &agent, herdr2::owner(&next), cancelled)?;
        let work::Started::Herdr2 { terminal } = &started else {
            return Err(Error::NothingToOpen);
        };
        session.pane = Pane::Herdr2 {
            terminal: terminal.clone(),
        };
        store.update_run(&run, Some(&session))?;
        work::prompt(&host, &started, &prompt, cancelled)?;
        run.state = RunState::Running;
        run.message = None;
        store.update_run(&run, Some(&session))
    })();
    if let Err(error) = &outcome {
        run.state = RunState::Failed;
        run.message = Some(error.to_string().chars().take(500).collect());
        // If ending the previous terminal failed, keep its original receipt
        // so a user can inspect it and explicitly retry safely.
        store.update_run(&run, Some(&session))?;
    }
    outcome
}
