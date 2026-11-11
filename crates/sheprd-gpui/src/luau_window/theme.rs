//! Moonframe's widgets in Sheprd's colors and font. Luau's text, buttons and
//! status bar draw from the toolkit's theme, so without this a mod's rows stay
//! in Moonframe's own look beside Sheprd's panes.

use {
    crate::embed::Palette,
    moonframe::gpui_kit::{
        App, Hsla, SharedString,
        component::{ActiveTheme, Theme, ThemeConfig, ThemeConfigColors, ThemeMode},
        px,
    },
    std::rc::Rc,
};

fn color(value: u32) -> SharedString {
    format!("#{:06x}", value & 0xff_ffff).into()
}

/// `percent` of `over` blended onto `base`, per channel.
fn mix(base: u32, over: u32, percent: u32) -> u32 {
    let channel = |shift: u32| {
        let (base, over) = ((base >> shift) & 255, (over >> shift) & 255);
        ((base * (100 - percent) + over * percent) / 100) << shift
    };
    channel(16) | channel(8) | channel(0)
}

/// The toolkit's theme for `palette`. Panels stand on the sidebar's color, as
/// the window's own do.
fn config(palette: &Palette) -> ThemeConfig {
    let Palette {
        accent,
        active,
        background,
        cursor,
        danger,
        dark,
        foreground,
        info,
        muted,
        sidebar,
        success,
        surface,
        warning,
        ..
    } = *palette;
    let blend = |percent| color(mix(sidebar, foreground, percent));
    let mut colors = ThemeConfigColors::default();
    colors.accent = Some(color(active));
    colors.accent_foreground = Some(color(foreground));
    colors.background = Some(color(background));
    colors.border = Some(blend(14));
    colors.caret = Some(color(cursor));
    colors.danger = Some(color(danger));
    colors.foreground = Some(color(foreground));
    colors.info = Some(color(info));
    colors.input = Some(blend(18));
    colors.list_active = Some(color(active));
    colors.list_active_border = Some(color(accent));
    colors.list_hover = Some(blend(8));
    colors.muted = Some(color(active));
    colors.muted_foreground = Some(color(muted));
    colors.popover = Some(color(surface));
    colors.popover_foreground = Some(color(foreground));
    colors.primary = Some(color(accent));
    colors.primary_active = Some(color(mix(accent, background, 12)));
    colors.primary_foreground = Some(color(background));
    colors.primary_hover = Some(color(mix(accent, foreground, 12)));
    colors.ring = Some(color(accent));
    colors.scrollbar_thumb = Some(blend(25));
    colors.scrollbar_thumb_hover = Some(blend(35));
    colors.secondary = Some(blend(8));
    colors.secondary_active = Some(blend(16));
    colors.secondary_foreground = Some(color(foreground));
    colors.secondary_hover = Some(blend(12));
    colors.selection = Some(color(mix(background, accent, 30)));
    colors.sidebar = Some(color(sidebar));
    colors.sidebar_border = Some(blend(14));
    colors.sidebar_foreground = Some(color(foreground));
    colors.success = Some(color(success));
    colors.title_bar = Some(color(sidebar));
    colors.title_bar_border = Some(blend(14));
    colors.warning = Some(color(warning));
    ThemeConfig {
        colors,
        font_family: Some(palette.font_family.clone().into()),
        font_size: Some(palette.font_size),
        mode: mode(dark),
        name: "Sheprd".into(),
        ..ThemeConfig::default()
    }
}

fn mode(dark: bool) -> ThemeMode {
    if dark {
        ThemeMode::Dark
    } else {
        ThemeMode::Light
    }
}

/// Keeps the toolkit on Sheprd's theme. Moonframe sets its own again when the
/// system's appearance changes, so what was last applied is checked as well.
#[derive(Default)]
pub(crate) struct Follow {
    applied: Option<(Palette, Hsla)>,
}

impl Follow {
    pub(crate) fn apply(&mut self, palette: &Palette, cx: &mut App) {
        let current = cx.theme().background;
        if self
            .applied
            .as_ref()
            .is_some_and(|(applied, background)| applied == palette && *background == current)
        {
            return;
        }
        let config = Rc::new(config(palette));
        let theme = Theme::global_mut(cx);
        if palette.dark {
            theme.dark_theme = config;
        } else {
            theme.light_theme = config;
        }
        Theme::change(mode(palette.dark), None, cx);
        // The stylesheet's font is set apart from the colors, after them.
        let theme = Theme::global_mut(cx);
        theme.font_family = palette.font_family.clone().into();
        theme.font_size = px(palette.font_size);
        // No Moonframe terminal is shown, so `font = "mono"` is free for the
        // sidebar's own font.
        theme.mono_font_family = palette.sidebar_font_family.clone().into();
        theme.mono_font_size = px(palette.sidebar_font_size);
        self.applied = Some((palette.clone(), cx.theme().background));
        cx.refresh_windows();
    }
}

#[cfg(test)]
mod tests;
