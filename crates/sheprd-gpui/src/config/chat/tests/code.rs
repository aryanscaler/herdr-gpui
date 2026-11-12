use super::*;

#[test]
fn code_defaults_keep_todays_look_and_are_read_and_saved() {
    let config = crate::config::Config::parse("").unwrap();
    assert!(!config.chat.wrap_code);
    assert!(!config.chat.code_line_numbers);
    assert!(!config.chat.code_stripes);
    assert_eq!(config.chat.render_html, HtmlRender::Default);
    for &mode in HtmlRender::ALL {
        let text = format!("[chat]\nrender_html = \"{}\"\n", mode.name());
        let config = crate::config::Config::parse(&text).unwrap();
        assert_eq!(config.chat.render_html, mode);
    }
    assert!(crate::config::Config::parse("[chat]\nrender_html = \"sometimes\"\n").is_err());

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config-gpui.local.toml");
    std::fs::write(&path, "[chat]\nlayout = \"cards\"\n").unwrap();
    use crate::config::preferences::Preference;
    for edit in [
        Preference::ChatWrapCode(true),
        Preference::ChatCodeLineNumbers(true),
        Preference::ChatCodeStripes(true),
        Preference::ChatRenderHtml(HtmlRender::Always),
    ] {
        crate::config::Config::save_preference_path(edit, &path).unwrap();
    }
    let config = crate::config::Config::parse(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert!(config.chat.wrap_code);
    assert!(config.chat.code_line_numbers);
    assert!(config.chat.code_stripes);
    assert_eq!(config.chat.render_html, HtmlRender::Always);
    assert_eq!(config.chat.layout, ChatLayoutMode::Cards);
}

#[test]
fn every_html_mode_has_its_own_name_and_label() {
    let names: std::collections::HashSet<_> = HtmlRender::ALL.iter().map(|m| m.name()).collect();
    let labels: std::collections::HashSet<_> = HtmlRender::ALL.iter().map(|m| m.label()).collect();
    assert_eq!(names.len(), HtmlRender::ALL.len());
    assert_eq!(labels.len(), HtmlRender::ALL.len());
}
