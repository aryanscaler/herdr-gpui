use super::*;
use crate::daytona::{Error, api::SshAccess, gateway::Gateway};
use std::path::{Path, PathBuf};

fn gateway(known: &Path) -> Gateway {
    let access: SshAccess = serde_json::from_value(serde_json::json!({
        "token": "tok",
        "sshCommand": "ssh tok@gw.example.com",
    }))
    .unwrap();
    Gateway::new(&access, known).unwrap()
}

/// The `-F` file `command` reads.
fn config(command: &std::process::Command) -> PathBuf {
    PathBuf::from(command.get_args().nth(1).unwrap())
}

#[test]
fn the_core_step_runs_while_its_ssh_config_exists() {
    let dir = tempfile::tempdir().unwrap();
    let mut read = None;
    over_gateway(
        Ok(gateway(&dir.path().join("known"))),
        &|| false,
        |command| {
            let path = config(&command);
            read = Some((path.exists(), path));
            Ok(())
        },
    )
    .unwrap();
    let (existed, path) = read.unwrap();
    assert!(existed, "ssh must find the config it is pointed at");
    assert!(!path.exists(), "it goes once the step ends");
}

#[test]
fn a_gateway_that_could_not_be_made_fails_the_core_step() {
    let add = |_| -> cloud::Result<()> { panic!("no command to run") };
    assert!(matches!(
        over_gateway(Err(Error::Gateway), &|| false, add),
        Err(cloud::Error::Install(install::Failure::Core(None)))
    ));
    assert!(matches!(
        over_gateway(Err(Error::Gateway), &|| true, add),
        Err(cloud::Error::Cancelled)
    ));
}
