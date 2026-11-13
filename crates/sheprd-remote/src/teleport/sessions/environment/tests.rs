use super::*;

#[test]
fn known_default_and_custom_home_roots_map_to_the_destination_home() -> Result<()> {
    let source = BTreeMap::from([("CODEX_HOME".into(), "/home/source/custom/codex".into())]);
    let mapped = map_roots(
        AgentKind::Codex,
        &source,
        "/work/source",
        "/work/destination",
        "/home/source",
        "/home/destination",
    )?;
    assert_eq!(mapped["CODEX_HOME"], "/home/destination/custom/codex");
    let environment = Environment {
        source,
        destination: mapped,
    };
    let argv = environment.resume(
        AgentKind::Codex,
        "session-1",
        &["--config=/home/source/custom/codex/config.toml".into()],
        "/work/source",
        "/work/destination",
    );
    assert_eq!(
        argv,
        [
            "env",
            "CODEX_HOME=/home/destination/custom/codex",
            "codex",
            "resume",
            "session-1",
            "--config=/home/destination/custom/codex/config.toml"
        ]
    );
    let defaults = BTreeMap::from([("CLAUDE_CONFIG_DIR".into(), "/home/source/.claude".into())]);
    assert_eq!(
        map_roots(
            AgentKind::Claude,
            &defaults,
            "/work/source",
            "/work/destination",
            "/home/source",
            "/home/destination"
        )?["CLAUDE_CONFIG_DIR"],
        "/home/destination/.claude"
    );
    Ok(())
}

#[test]
fn roots_outside_home_and_checkout_require_an_explicit_mapping() {
    let source = BTreeMap::from([("CODEX_HOME".into(), "/srv/private/codex".into())]);
    assert!(matches!(
        map_roots(
            AgentKind::Codex,
            &source,
            "/work/source",
            "/work/destination",
            "/home/source",
            "/home/destination"
        ),
        Err(Error::SessionConfiguration { .. })
    ));
}

#[test]
fn roots_that_climb_out_of_home_or_checkout_are_refused() {
    for value in [
        "/home/source/../../etc",
        "/home/source/./codex",
        "/home/source//codex",
        "/work/source/../outside",
        "home/source/codex",
    ] {
        let source = BTreeMap::from([("CODEX_HOME".into(), value.into())]);
        assert!(
            matches!(
                map_roots(
                    AgentKind::Codex,
                    &source,
                    "/work/source",
                    "/work/destination",
                    "/home/source",
                    "/home/destination",
                ),
                Err(Error::SessionConfiguration { .. })
            ),
            "{value} was mapped"
        );
    }
}
