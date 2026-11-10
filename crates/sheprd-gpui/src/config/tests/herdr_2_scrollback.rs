use super::*;
use crate::config::preferences::Preference;

#[test]
fn herdr_2_scrollback_defaults_and_is_clamped() -> anyhow::Result<()> {
    assert_eq!(Config::default().herdr_2_scrollback, HERDR_2_SCROLLBACK);
    assert_eq!(Config::parse("")?.herdr_2_scrollback, HERDR_2_SCROLLBACK);
    assert_eq!(
        Config::parse(DEFAULT_CONFIG)?.herdr_2_scrollback,
        HERDR_2_SCROLLBACK
    );
    assert_eq!(
        Config::parse("herdr_2_scrollback = 50000")?.herdr_2_scrollback,
        50_000
    );
    assert_eq!(
        Config::parse("herdr_2_scrollback = 5")?.herdr_2_scrollback,
        1_000
    );
    assert_eq!(
        Config::parse("herdr_2_scrollback = 9000000")?.herdr_2_scrollback,
        100_000
    );
    assert!(Config::parse("herdr_2_scrollback = -1").is_err());
    assert!(Config::parse("herdr_2_scrollback = 'lots'").is_err());
    Ok(())
}

#[test]
fn choosing_a_herdr_2_scrollback_saves_in_place() -> anyhow::Result<()> {
    let temp = TempDirectory::new()?;
    let path = temp.0.join("local.toml");
    fs::write(&path, "# personal\n")?;
    Config::save_preference_path(Preference::Herdr2Scrollback(100_000), &path)?;
    let text = fs::read_to_string(&path)?;
    assert!(text.contains("# personal"));
    assert_eq!(Config::parse(&text)?.herdr_2_scrollback, 100_000);
    Ok(())
}
