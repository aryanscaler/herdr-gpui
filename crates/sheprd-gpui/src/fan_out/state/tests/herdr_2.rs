//! Fan-outs from a workspace the app keeps for Herdr 2: lanes stay on this
//! computer, each agent's shell is shown in a kept workspace of its own, and
//! removed lanes close theirs.

use super::*;

fn kept_origin() -> Origin {
    Origin {
        checkout: Some("/tmp/repo".into()),
        workspace_id: "kept-1".into(),
        ..origin()
    }
}

fn kept_checkout(n: usize) -> Checkout {
    Checkout {
        endpoint_id: "local".into(),
        host: origin().host,
        worktree: work::Worktree {
            path: format!("/tmp/lane-{n}"),
            opened: work::Opened::Herdr2,
        },
        terminal: None,
    }
}

#[test]
fn a_kept_origin_makes_its_lanes_with_git_on_this_computer() {
    assert_eq!(
        kept_origin().repository(),
        work::Repository::Herdr2 {
            checkout: "/tmp/repo".into()
        }
    );
    assert_eq!(
        origin().repository(),
        work::Repository::Herdr1 {
            workspace_id: "w1".into()
        }
    );
    let mut fan_out = idle();
    fan_out.origin = kept_origin();
    fan_out.picks.add(AgentKind::Claude);
    fan_out.spread = true;
    assert!(!fan_out.can_spread());
    assert_eq!(fan_out.lane_hosts(), ["local"]);
}

#[test]
fn a_started_shell_is_shown_once_and_a_removed_lane_closes() {
    let mut fan_out = launched();
    fan_out.origin = kept_origin();
    for index in 0..3 {
        send(
            &fan_out,
            Event::Report(Report::Lane(
                index,
                Progress::Created(Box::new(kept_checkout(index))),
            )),
        );
    }
    send(
        &fan_out,
        Event::Report(Report::Lane(1, Progress::Terminal("t1".into()))),
    );
    fan_out.poll();
    assert_eq!(
        fan_out.lanes[1]
            .checkout
            .as_ref()
            .unwrap()
            .terminal
            .as_deref(),
        Some("t1")
    );
    let (shown, closed) = fan_out.take_kept();
    assert_eq!(
        shown,
        [Shown {
            label: fan_out.lanes[1].lane.branch.clone(),
            path: "/tmp/lane-1".into(),
            terminal: "t1".into(),
        }]
    );
    assert!(closed.is_empty());
    assert_eq!(fan_out.take_kept(), (Vec::new(), Vec::new()), "shown once");

    fan_out.stage = Stage::Removing(2);
    send(&fan_out, Event::Removed(vec![(0, Ok(())), (1, Ok(()))]));
    let (_, kept) = fan_out.poll();
    assert_eq!(
        kept,
        Some(Kept::Herdr2 {
            label: fan_out.lanes[2].lane.branch.clone(),
            path: "/tmp/lane-2".into(),
        })
    );
    let (_, closed) = fan_out.take_kept();
    assert_eq!(closed, ["/tmp/lane-0", "/tmp/lane-1"]);
}

#[test]
fn a_herdr_1_removal_leaves_closing_to_its_daemon() {
    let mut fan_out = launched();
    for index in 0..3 {
        fan_out.lanes[index].checkout = Some(checkout(index));
    }
    fan_out.stage = Stage::Removing(2);
    send(&fan_out, Event::Removed(vec![(0, Ok(())), (1, Ok(()))]));
    fan_out.poll();
    assert_eq!(fan_out.take_kept(), (Vec::new(), Vec::new()));
}
