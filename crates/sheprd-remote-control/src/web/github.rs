//! Authenticated HTML views of the same bounded reads the iOS client uses.
//! Log text is escaped by Maud, never interpreted as markup or terminal input.

use super::{Device, Shared};
use crate::{
    control::changes::{Answer, CheckState, Wanted},
    native::changes,
};
use axum::{
    extract::{Path, State},
    response::Html,
};
use maud::html;

pub(super) async fn checks(
    State(state): State<Shared>,
    _device: Device,
    Path(workspace): Path<String>,
) -> Html<String> {
    let path = format!("/checks/{}", super::render::urlencode(&workspace));
    let answer = changes::ask(&state, workspace, Wanted::Checks).await;
    Html(html! {
        section class="p-4" {
            a href="/" data-nav { "‹ Workspaces" }
            h1 class="text-[19px] font-bold py-4" { "Pull request checks" }
            @if !state.github.enabled() {
                p class="text-muted" { "Standalone GitHub reads are not enabled on this computer. Its owner can sign in with gh and enable remote GitHub CLI reads there." }
            }
            a href=(&path) data-nav { "Refresh" }
            @match answer {
                Ok(Answer::Checks(checks)) => {
                    @if let Some(number) = checks.number {
                        p class="py-4" { "Pull request #" (number) }
                    } @else {
                        p class="py-4" { "No pull request for this branch." }
                    }
                    @if checks.number.is_some() && checks.checks.is_empty() {
                        p { "No checks reported." }
                    }
                    ul {
                        @for check in checks.checks {
                            li class="py-4" {
                                b { (check.name) } " · "
                                (match check.state {
                                    CheckState::Passed => "Passed",
                                    CheckState::Failed => "Failed",
                                    CheckState::Pending => "Pending",
                                    CheckState::Skipped => "Skipped",
                                })
                                @if let Some(job) = check.job {
                                    " · " a href={ (path) "/" (job) } data-nav { "Read failing log" }
                                }
                            }
                        }
                    }
                }
                Err(error) => p class="py-4 text-muted" role="alert" { (error.to_string()) },
                _ => p role="alert" { "The computer returned an unexpected answer." },
            }
        }
    }.into_string())
}

pub(super) async fn log(
    State(state): State<Shared>,
    _device: Device,
    Path((workspace, job)): Path<(String, u64)>,
) -> Html<String> {
    let path = format!("/checks/{}", super::render::urlencode(&workspace));
    let answer = changes::ask(&state, workspace, Wanted::CheckLog { job }).await;
    Html(html! {
        section class="p-4" {
            a href=(&path) data-nav { "‹ Checks" }
            h1 class="text-[19px] font-bold py-4" { "Failing check log" }
            a href={ (path) "/" (job) } data-nav { "Retry" }
            @match answer {
                Ok(Answer::CheckLog(log)) => pre class="whitespace-pre-wrap wrap-anywhere py-4" { (log.excerpt) },
                Err(error) => p class="py-4 text-muted" role="alert" { (error.to_string()) },
                _ => p role="alert" { "The computer returned an unexpected answer." },
            }
        }
    }.into_string())
}
