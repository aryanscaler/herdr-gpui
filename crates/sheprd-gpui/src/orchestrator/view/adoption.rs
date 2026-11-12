use super::{OrchestratorView, dispatch::Confirm};
use gpui::Context;
use std::sync::atomic::AtomicBool;

impl OrchestratorView {
    pub(crate) fn confirm_adoption(&mut self, run: &str, cx: &mut Context<Self>) -> bool {
        let Some(index) =
            self.snapshot().runs.iter().position(|found| {
                found.id == run && found.owner == crate::orchestrator::Owner::Sheprd
            })
        else {
            return false;
        };
        if self.adoption_pending {
            return false;
        }
        self.offer_adoption(index, cx);
        self.adoption_pending
    }
    /// Read-only discovery. Authority is granted only by the later confirmation.
    pub(super) fn offer_adoption(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.adoption_pending {
            return;
        }
        let Some(run) = self.snapshot().runs.get(index).cloned() else {
            return;
        };
        let Some(session) = self
            .snapshot()
            .sessions
            .iter()
            .find(|session| session.run_id == run.id)
            .cloned()
        else {
            return;
        };
        let id = run.id.clone();
        let expected = session.clone();
        self.adoption_pending = true;
        let background = cx.background_executor().spawn(async move {
            let target = crate::orchestrator::actions::target_of(&session);
            let host = sheprd_remote::teleport::Host::new(&target)
                .map_err(|_| crate::orchestrator::Error::UnsupportedHost)?;
            crate::orchestrator::actions::legacy::proposal(
                &host,
                &run,
                &session,
                &AtomicBool::new(false),
            )
        });
        cx.spawn(async move |this, cx| {
            let result = background.await;
            let _ = this.update(cx, |this, cx| {
                this.adoption_pending = false;
                if this.confirm.is_some()
                    || !this
                        .snapshot()
                        .sessions
                        .iter()
                        .any(|session| session == &expected)
                {
                    return;
                }
                match result {
                    Ok(receipt) => this.ask(
                        Confirm::Adopt {
                            run: id,
                            receipt: Box::new(receipt),
                        },
                        cx,
                    ),
                    Err(error) => this.report(error, cx),
                }
            });
        })
        .detach();
    }
}
