use super::{Palette, config, mix};

fn palette(dark: bool) -> Palette {
    Palette {
        accent: 0x5e5ce6,
        active: 0x2b2933,
        background: 0x101419,
        cursor: 0xd8dee9,
        danger: 0xcc0000,
        dark,
        font_family: "Inter".into(),
        font_size: 12.,
        foreground: 0xd8dee9,
        info: 0x00cccc,
        muted: 0x827e91,
        sidebar: 0x1c1c22,
        sidebar_font_family: "Menlo".into(),
        sidebar_font_size: 12.,
        success: 0x00cc00,
        surface: 0x1c1c22,
        warning: 0xcccc00,
    }
}

#[test]
fn mixing_goes_from_the_base_to_the_other_color() {
    assert_eq!(mix(0x000000, 0xffffff, 0), 0x000000);
    assert_eq!(mix(0x000000, 0xffffff, 100), 0xffffff);
    assert_eq!(mix(0x102030, 0x102030, 50), 0x102030);
}

#[test]
fn text_takes_the_windows_foreground_and_muted_colors() {
    let config = config(&palette(true));
    assert_eq!(config.colors.foreground.as_deref(), Some("#d8dee9"));
    assert_eq!(config.colors.muted_foreground.as_deref(), Some("#827e91"));
    assert_eq!(config.colors.sidebar.as_deref(), Some("#1c1c22"));
}

#[test]
fn the_mode_follows_the_palette() {
    assert!(config(&palette(true)).mode.is_dark());
    assert!(!config(&palette(false)).mode.is_dark());
}
