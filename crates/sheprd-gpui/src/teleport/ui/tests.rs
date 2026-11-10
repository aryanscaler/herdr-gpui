//! A Teleport dialog at each stage, with what the stage shows as data.
use super::*;
use crate::teleport::{
    Host, Keeping, Landed, Node, Repository, SourceState, TabPlan, launch::Work,
};
use sheprd_client::ConnectTarget;

mod closing;
mod en_xa;
mod kept;
mod opening;

fn place(label: &str) -> Place {
    Place {
        endpoint_id: label.into(),
        label: label.into(),
        host: Host::new(&ConnectTarget::Local).unwrap(),
    }
}

fn source() -> Source {
    Source {
        place: place("laptop"),
        workspace_id: "w1".into(),
        keeping: Keeping::Herdr1,
        custom_label: None,
        repo_key: "repo".into(),
        repo_label: "herdr".into(),
        branch: Some("feature".into()),
        tab_labels: Default::default(),
        core_tabs: Vec::new(),
        core_preferred: false,
    }
}

fn candidate() -> Candidate {
    Candidate {
        place: place("buildbox"),
        destination: job::Destination::Open {
            repository: Repository {
                key: "repo".into(),
                label: "herdr".into(),
                workspace_id: "w9".into(),
            },
            reason: MatchReason::Name,
        },
        origin: None,
        lands_in: Keeper::Herdr1,
    }
}

fn review(github: GitHubAccess) -> Review {
    let pane = |id: &str, action: Action| job::PanePlan {
        pane_id: id.into(),
        cwd: None,
        work: Work::Shell,
        action,
    };
    Review {
        checkout: "/src/herdr".into(),
        branch: "feature".into(),
        state: SourceState {
            branch: Some("feature".into()),
            head: "abc".into(),
            unpushed: 1,
            changed: 2,
            untracked: 3,
        },
        tabs: vec![
            TabPlan {
                generation: Generation::Herdr1,
                into: Generation::Herdr2,
                label: None,
                tree: Node::Pane("p1".into()),
                panes: vec![
                    pane("p1", Action::Shell),
                    pane("p2", Action::Run(vec!["cargo".into(), "test".into()])),
                ],
            },
            TabPlan {
                generation: Generation::Herdr1,
                into: Generation::Herdr1,
                label: Some("agents".into()),
                tree: Node::Pane("p3".into()),
                panes: vec![pane("p3", Action::Missing("aider".into()))],
            },
        ],
        reason: Some(MatchReason::Name),
        github,
    }
}

/// The dialog in each stage it shows.
pub(super) fn stages() -> Vec<(&'static str, Teleport)> {
    let at = |stage: Stage| {
        let mut teleport = Teleport::start(source(), "feature".into(), Vec::new());
        teleport.stage = stage;
        teleport
    };
    let mut choosing = Teleport::start(source(), "feature".into(), Vec::new());
    choosing.hosts = vec![
        HostRepositories {
            place: place("buildbox"),
            repositories: Some(Vec::new()),
            retired: Vec::new(),
            lands_in: Keeper::Herdr1,
        },
        HostRepositories {
            place: place("nas"),
            repositories: None,
            retired: Vec::new(),
            lands_in: Keeper::Herdr1,
        },
    ];
    vec![
        ("no hosts", at(Stage::Choosing)),
        ("choosing", choosing),
        ("reviewing", at(Stage::Reviewing(place("buildbox")))),
        (
            "ready, with a token",
            at(Stage::Ready(
                candidate(),
                Box::new(review(GitHubAccess::Token)),
            )),
        ),
        (
            "ready, without GitHub",
            at(Stage::Ready(
                candidate(),
                Box::new(review(GitHubAccess::Unavailable)),
            )),
        ),
        ("starting", at(Stage::Moving(candidate(), None))),
        ("moving", at(Stage::Moving(candidate(), Some(Step::Fetch)))),
    ]
}

/// The host and worktree names the stages show.
pub(super) const DATA: &[&str] = &["laptop", "buildbox", "nas", "feature", "herdr"];
