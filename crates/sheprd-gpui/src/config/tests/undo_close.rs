use super::*;
use crate::config::preferences::Preference;
use std::time::Duration;

#[test]
fn undo_close_is_off_with_ten_seconds_by_default() -> anyhow::Result<()> {
    for config in [
        Config::default(),
        Config::parse("")?,
        Config::parse(DEFAULT_CONFIG)?,
    ] {
        assert_eq!(config.undo_close_tab, UndoCloseTab::default());
        assert!(!config.undo_close_tab.enabled);
        assert_eq!(config.undo_close_tab.window, Duration::from_secs(10));
    }
    Ok(())
}

#[test]
fn undo_close_reads_its_switch_and_window() -> anyhow::Result<()> {
    let config = Config::parse("[undo_close_tab]\nenabled = true\nseconds = 5\nlength = 2\n")?;
    assert_eq!(
        config.undo_close_tab,
        UndoCloseTab {
            enabled: true,
            window: Duration::from_secs(5),
        }
    );
    assert_eq!(config.unknown_keys, ["undo_close_tab.length"]);
    Ok(())
}

#[test]
fn undo_close_refuses_a_window_out_of_range() {
    for seconds in ["0", "61", "-1"] {
        let text = format!("[undo_close_tab]\nseconds = {seconds}\n");
        assert!(Config::parse(&text).is_err(), "{seconds}");
    }
}

#[test]
fn the_settings_switch_saves_undo_close() -> anyhow::Result<()> {
    let temp = TempDirectory::new()?;
    let path = temp.0.join("local.toml");
    fs::write(&path, "# personal\n")?;
    Config::save_preference_path(Preference::UndoCloseTab(true), &path)?;
    let text = fs::read_to_string(&path)?;
    assert!(text.contains("# personal"));
    let config = Config::parse(&text)?;
    assert!(config.undo_close_tab.enabled);
    Ok(())
}
