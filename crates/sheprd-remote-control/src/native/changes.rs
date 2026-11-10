//! `GET /api/v2/workspaces/{workspace}/changes` and its neighbours: a
//! workspace's changed files, one file's diff, and its pull request's checks.
//! All read-only. Sheprd works them out while it is open and lists the
//! workspace (`control::changes`); otherwise this daemon asks Git itself
//! ([`local`]), so a Herdr 2 workspace, or one on a host, still answers.

mod error;
mod local;
mod mailbox;

pub(crate) use error::Error;
pub(crate) use mailbox::Mailbox;

use crate::hub::Keeper;
use crate::{
    control::{
        changes::{Answer, CheckLog, Checks, Failure, FileDiff, FileList, MAX_PATH, Scope, Wanted},
        layout,
    },
    web::{ApiError, Device, Shared},
};
use axum::{
    Json,
    extract::{Path, Query, State},
};
use serde::Deserialize;
use std::time::{Duration, Instant};

/// How long a phone's question waits for Sheprd: a large repository's
/// listing takes a while, and the phone allows a little longer.
const ANSWER_WAIT: Duration = Duration::from_secs(25);

#[derive(Deserialize)]
pub(crate) struct FilesQuery {
    #[serde(default)]
    scope: Scope,
}

#[derive(Deserialize)]
pub(crate) struct DiffQuery {
    #[serde(default)]
    scope: Scope,
    path: String,
}

/// Answers `wanted` about `workspace`: Sheprd's answer while it is open
/// and lists the workspace, else Git's. A workspace this daemon does not
/// list is refused here, so only ones the phone could see are read.
async fn ask(state: &Shared, workspace: String, wanted: Wanted) -> Result<Answer, ApiError> {
    let (boot, cwd) = {
        let layout = state.hub.layout();
        let listed = layout.workspace(&workspace).ok_or(ApiError::NotFound)?;
        (layout.boot.clone(), listed.cwd.clone())
    };
    if state.changes.open() {
        match ask_sheprd(state, boot, workspace, wanted.clone()).await {
            // Closed meanwhile, or a workspace it does not keep.
            Err(Error::NoDesktop | Error::Failed(Failure::Gone)) => {}
            // It quit while the question waited.
            Err(Error::TimedOut) if !state.changes.open() => {}
            answered => return answered.map_err(ApiError::from),
        }
    }
    if cwd.is_empty() {
        return Err(Error::Failed(Failure::NoRepository).into());
    }
    let answer =
        tokio::task::spawn_blocking(move || local::answer(std::path::Path::new(&cwd), &wanted))
            .await
            .map_err(|_| Error::Failed(Failure::Unreadable))?;
    match answer {
        Answer::Failed(failure) => Err(Error::Failed(failure).into()),
        answer => Ok(answer),
    }
}

/// Puts `wanted` to Sheprd and waits for its answer.
async fn ask_sheprd(
    state: &Shared,
    boot: String,
    workspace: String,
    wanted: Wanted,
) -> Result<Answer, Error> {
    let mut pending = state.changes.ask(boot, workspace, wanted, Instant::now())?;
    match tokio::time::timeout(ANSWER_WAIT, pending.answer()).await {
        Ok(Some(Answer::Failed(failure))) => Err(Error::Failed(failure)),
        Ok(Some(answer)) => Ok(answer),
        Ok(None) | Err(_) => Err(Error::TimedOut),
    }
}

/// How long a phone's change to a Herdr 2 tab waits for Sheprd, which
/// makes it at once.
const LAYOUT_WAIT: Duration = Duration::from_secs(10);

/// Asks Sheprd to make `op` at the pane of `terminal` in its saved Herdr 2
/// tab `tab`, and waits until it has.
pub(crate) async fn change_layout(
    state: &Shared,
    tab: u64,
    terminal: String,
    op: layout::Op,
) -> Result<Option<String>, ApiError> {
    let mut pending = state
        .changes
        .ask_layout(tab, terminal, op, Instant::now())
        .map_err(ApiError::from)?;
    match tokio::time::timeout(LAYOUT_WAIT, pending.answer()).await {
        Ok(Some(layout::Answer::Done)) => Ok(None),
        Ok(Some(layout::Answer::Split(created))) if created.len() <= layout::MAX_TERMINAL => {
            Ok(Some(created))
        }
        Ok(Some(layout::Answer::Split(_))) => Err(mismatch()),
        Ok(Some(layout::Answer::Failed(failure))) => Err(Error::Layout(failure).into()),
        Ok(None) | Err(_) => Err(Error::TimedOut.into()),
    }
}

/// Makes `op` where `keeper` says: Sheprd's saved tab is Sheprd's to
/// change while it is open, and this daemon's otherwise.
pub(crate) async fn arrange(
    state: &Shared,
    keeper: Keeper,
    op: layout::Op,
) -> Result<Option<String>, ApiError> {
    match keeper {
        Keeper::Saved { tab, terminal } if state.changes.open() => {
            change_layout(state, tab, terminal, op).await
        }
        keeper => Ok(state.hub.arrange(keeper, op).await?),
    }
}

/// An answer that does not fit its question: never one from this Sheprd.
fn mismatch() -> ApiError {
    Error::Failed(Failure::Unreadable).into()
}

pub(crate) async fn files(
    State(state): State<Shared>,
    _device: Device,
    Path(workspace): Path<String>,
    Query(query): Query<FilesQuery>,
) -> Result<Json<FileList>, ApiError> {
    let wanted = Wanted::Files { scope: query.scope };
    match ask(&state, workspace, wanted).await? {
        Answer::Files(list) => Ok(Json(list)),
        _ => Err(mismatch()),
    }
}

pub(crate) async fn diff(
    State(state): State<Shared>,
    _device: Device,
    Path(workspace): Path<String>,
    Query(query): Query<DiffQuery>,
) -> Result<Json<FileDiff>, ApiError> {
    if query.path.is_empty() || query.path.len() > MAX_PATH {
        return Err(ApiError::Invalid("that is not a changed file"));
    }
    let wanted = Wanted::Diff {
        scope: query.scope,
        path: query.path,
    };
    match ask(&state, workspace, wanted).await? {
        Answer::Diff(diff) => Ok(Json(diff)),
        _ => Err(mismatch()),
    }
}

pub(crate) async fn checks(
    State(state): State<Shared>,
    _device: Device,
    Path(workspace): Path<String>,
) -> Result<Json<Checks>, ApiError> {
    match ask(&state, workspace, Wanted::Checks).await? {
        Answer::Checks(checks) => Ok(Json(checks)),
        _ => Err(mismatch()),
    }
}

pub(crate) async fn check_log(
    State(state): State<Shared>,
    _device: Device,
    Path((workspace, job)): Path<(String, u64)>,
) -> Result<Json<CheckLog>, ApiError> {
    match ask(&state, workspace, Wanted::CheckLog { job }).await? {
        Answer::CheckLog(log) => Ok(Json(log)),
        _ => Err(mismatch()),
    }
}
