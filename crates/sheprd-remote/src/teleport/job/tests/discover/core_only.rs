use super::*;
use crate::teleport::{Host, Place};
use sheprd_client::ConnectTarget;

#[test]
fn an_unconnected_app_destination_does_not_query_herdr_one()
-> Result<(), Box<dyn std::error::Error>> {
    let home = tempfile::tempdir()?;
    // Development hosts deliberately refuse every herdr_cli call, providing
    // a deterministic guard against accidentally requiring a Herdr 1 daemon.
    let mut host = Host::new(&ConnectTarget::Session {
        name: "isolated".into(),
        development: true,
    })?;
    host.env
        .push(("HOME".into(), home.path().to_string_lossy().into_owned()));
    let destination = HostRepositories {
        place: Place {
            endpoint_id: "core-only".into(),
            label: "Core only".into(),
            host,
        },
        repositories: None,
        retired: Vec::new(),
        lands_in: Keeper::App,
    };
    let source = provision::SourceRepository {
        relative: "code/app".into(),
        origin: None,
    };
    let found = discover_host(
        &destination,
        &identity("app", None),
        &source,
        None,
        &AtomicBool::new(false),
    )?;
    assert!(matches!(
        found.as_slice(),
        [Candidate {
            destination: Destination::Arrive(Arrival::Clone { .. }),
            lands_in: Keeper::App,
            ..
        }]
    ));
    assert!(home.path().read_dir()?.next().is_none());
    Ok(())
}
