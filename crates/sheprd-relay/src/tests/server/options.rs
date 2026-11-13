//! Settings from variables and flags, their units, and their refusals.

use crate::{
    Error,
    limits::{parse_bytes, parse_duration},
    options::{Kind, Options, SETTINGS, variable},
};
use secrecy::ExposeSecret;
use std::{collections::HashMap, ffi::OsString, time::Duration};

fn parse(arguments: &[&str], variables: &[(&str, &str)]) -> crate::Result<Option<Options>> {
    let variables: HashMap<String, OsString> = variables
        .iter()
        .map(|(name, value)| ((*name).to_owned(), OsString::from(value)))
        .collect();
    Options::parse(arguments.iter().map(OsString::from), |name| {
        variables.get(name).cloned()
    })
}

#[test]
fn nothing_set_gives_safe_defaults() {
    let options = parse(&[], &[]).unwrap().unwrap();
    assert_eq!(options.listen.to_string(), "127.0.0.1:8080");
    assert_eq!(options.metrics.unwrap().to_string(), "127.0.0.1:9090");
    assert!(options.dashboard_token.is_none());
    assert_eq!(options.limits, crate::Limits::default());
    assert!(!options.behind_proxy);
}

#[test]
fn flags_win_over_variables() {
    let options = parse(
        &[
            "--max-connections",
            "7",
            "--session-bandwidth",
            "1MiB",
            "--behind-proxy",
        ],
        &[
            ("RELAY_MAX_CONNECTIONS", "3"),
            ("RELAY_IDLE_TIMEOUT", "2m"),
            ("RELAY_METRICS", "off"),
            ("RELAY_DASHBOARD_TOKEN", "  a long enough dashboard token "),
        ],
    )
    .unwrap()
    .unwrap();
    assert_eq!(options.limits.max_connections, 7);
    assert_eq!(options.limits.session_bandwidth, 1 << 20);
    assert_eq!(options.limits.idle_timeout, Duration::from_secs(120));
    assert!(options.metrics.is_none());
    assert!(options.behind_proxy);
    assert_eq!(
        options.dashboard_token.unwrap().expose_secret(),
        "a long enough dashboard token"
    );
}

#[test]
fn every_setting_is_read_from_its_variable() {
    let dir = std::env::temp_dir().join(format!("sheprd-relay-options-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("secret");
    std::fs::write(&file, "a secret of enough length\n").unwrap();
    for (name, kind) in SETTINGS {
        let value = match kind {
            Kind::Address => "0.0.0.0:1",
            Kind::Bytes => "64k",
            Kind::Count => "3",
            Kind::Duration => "5s",
            Kind::File => file.to_str().unwrap(),
            Kind::Flag => "1",
            Kind::Port => "443",
            Kind::Text => match *name {
                "auto-ban" => "off",
                "ban" => "203.0.113.0/24, 2001:db8::/32",
                "client-ip-header" => "do-connecting-ip",
                _ => "operator",
            },
        };
        let variable = variable(name);
        assert!(
            parse(&[], &[(&variable, value)]).is_ok(),
            "{variable}={value} was refused"
        );
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn bad_values_are_refused_with_their_name() {
    for (flag, value) in [
        ("--max-connections", "0"),
        ("--max-connections", "many"),
        ("--idle-timeout", "5"),
        ("--max-frame", "10"),
        ("--listen", "nowhere"),
        ("--session-bandwidth", "4 parsecs"),
    ] {
        let error = parse(&[flag, value], &[]).unwrap_err();
        assert!(
            matches!(error, Error::Invalid { .. }),
            "{flag} {value}: {error:?}"
        );
    }
    assert!(matches!(
        parse(&["--max-connections"], &[]).unwrap_err(),
        Error::Argument { .. }
    ));
    assert!(matches!(
        parse(&["--dashboard-token", "x"], &[]).unwrap_err(),
        Error::Argument { .. }
    ));
}

#[test]
fn help_asks_for_usage() {
    assert!(parse(&["--help"], &[]).unwrap().is_none());
}

#[test]
fn debug_hides_secrets() {
    let options = parse(&[], &[("RELAY_DASHBOARD_TOKEN", "hunter2hunter2")])
        .unwrap()
        .unwrap();
    assert!(!format!("{options:?}").contains("hunter2"));
}

#[test]
fn units_parse() {
    assert_eq!(parse_bytes("65536"), Some(65536));
    assert_eq!(parse_bytes("64k"), Some(65536));
    assert_eq!(parse_bytes("4MiB"), Some(4 << 20));
    assert_eq!(parse_bytes("1G"), Some(1 << 30));
    assert_eq!(parse_bytes("1T"), None);
    assert_eq!(parse_bytes("k"), None);
    assert_eq!(parse_duration("500ms"), Some(Duration::from_millis(500)));
    assert_eq!(parse_duration("24h"), Some(Duration::from_secs(86_400)));
    assert_eq!(parse_duration("7d"), Some(Duration::from_secs(604_800)));
    assert_eq!(parse_duration("10"), None);
    assert_eq!(parse_duration("s"), None);
}

#[test]
fn port_is_honoured_unless_listen_says_otherwise() {
    let options = parse(&[], &[("PORT", "8080")]).unwrap().unwrap();
    assert_eq!(options.listen.to_string(), "0.0.0.0:8080");
    let options = parse(&[], &[("PORT", "8080"), ("RELAY_LISTEN", "127.0.0.1:9000")])
        .unwrap()
        .unwrap();
    assert_eq!(options.listen.to_string(), "127.0.0.1:9000");
    let options = parse(&["--listen", "127.0.0.1:9001"], &[("PORT", "8080")])
        .unwrap()
        .unwrap();
    assert_eq!(options.listen.to_string(), "127.0.0.1:9001");
    for bad in ["0", "http", "70000"] {
        assert!(matches!(
            parse(&[], &[("PORT", bad)]),
            Err(Error::Invalid { name: "PORT", .. })
        ));
    }
}

#[test]
fn a_client_ip_header_is_a_plain_header_name() {
    let options = parse(&["--client-ip-header", "DO-Connecting-IP"], &[])
        .unwrap()
        .unwrap();
    assert_eq!(
        options.client_ip_header.as_deref(),
        Some("do-connecting-ip")
    );
    for bad in ["", "bad header", "x:y", &"a".repeat(65)] {
        assert!(parse(&["--client-ip-header", bad], &[]).is_err(), "{bad:?}");
    }
}
