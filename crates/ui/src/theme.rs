//! Design tokens. Dark only for v1; every colour lives in this one struct.

use gpui::{App, Hsla, rgb};
use gpui_component::{Theme as ComponentTheme, ThemeTokens};

pub const SANS: &str = "IBM Plex Sans";
pub const MONO: &str = "IBM Plex Mono";

/// Colour tokens as `0xRRGGBB`, used through [`gpui::rgb`].
#[derive(Clone, Copy, Debug)]
pub struct Theme {
    pub bg: u32,
    pub surface: u32,
    pub surface_2: u32,
    pub hover: u32,
    pub selected: u32,
    pub border: u32,
    pub border_strong: u32,
    pub divider: u32,
    pub fg: u32,
    pub fg_2: u32,
    pub muted: u32,
    pub faint: u32,
    pub good: u32,
    pub warn: u32,
    pub arrow: u32,
    pub bar: u32,
    pub last_move_light: u32,
    pub last_move_dark: u32,
}

/// The active theme.
pub const T: Theme = Theme {
    bg: 0x0E1013,
    surface: 0x16191E,
    surface_2: 0x12151A,
    hover: 0x1B2028,
    selected: 0x22272E,
    border: 0x262B33,
    border_strong: 0x2C323B,
    divider: 0x1D2127,
    fg: 0xE8EAED,
    fg_2: 0xC3C9D1,
    muted: 0x9AA3AE,
    faint: 0x7F8893,
    good: 0x3FB950,
    warn: 0xF0A33A,
    arrow: 0xE8892B,
    bar: 0x6B7480,
    last_move_light: 0xF3E3A6,
    last_move_dark: 0xC9B56E,
};

fn hsla(color: u32) -> Hsla {
    rgb(color).into()
}

/// Maps Fast's tokens onto the component library so inputs, selects, sliders
/// and checkboxes read as part of one design.
pub fn apply_component_theme(cx: &mut App) {
    let theme = ComponentTheme::global_mut(cx);
    theme.font_family = SANS.into();
    theme.mono_font_family = MONO.into();
    theme.font_size = gpui::px(14.);
    theme.mono_font_size = gpui::px(13.);
    theme.radius = gpui::px(4.);
    theme.radius_lg = gpui::px(6.);
    theme.shadow = false;
    let c = &mut theme.colors;
    c.background = hsla(T.bg);
    c.foreground = hsla(T.fg);
    c.border = hsla(T.border_strong);
    c.input = hsla(T.border_strong);
    c.ring = hsla(T.fg);
    c.caret = hsla(T.fg);
    c.selection = hsla(0x3A424D);
    c.primary = hsla(T.fg);
    c.primary_hover = hsla(T.fg_2);
    c.primary_active = hsla(T.fg_2);
    c.primary_foreground = hsla(T.bg);
    c.secondary = hsla(T.surface);
    c.secondary_hover = hsla(T.hover);
    c.secondary_active = hsla(T.selected);
    c.secondary_foreground = hsla(T.fg);
    c.muted = hsla(T.surface_2);
    c.muted_foreground = hsla(T.muted);
    c.accent = hsla(T.hover);
    c.accent_foreground = hsla(T.fg);
    c.popover = hsla(T.surface);
    c.popover_foreground = hsla(T.fg);
    c.list = hsla(T.surface);
    c.list_active = hsla(T.selected);
    c.list_hover = hsla(T.hover);
    c.list_active_border = hsla(T.fg);
    c.slider_bar = hsla(T.fg);
    c.slider_thumb = hsla(T.fg);
    c.scrollbar = hsla(T.surface);
    c.scrollbar_thumb = hsla(T.border_strong);
    c.scrollbar_thumb_hover = hsla(T.bar);
    c.overlay = gpui::hsla(0., 0., 0., 0.5);
    c.window_border = hsla(T.border);
    c.success = hsla(T.good);
    c.success_foreground = hsla(T.bg);
    c.warning = hsla(T.warn);
    c.warning_foreground = hsla(T.bg);
    c.danger = hsla(T.warn);
    c.danger_foreground = hsla(T.bg);
    c.info = hsla(T.fg);
    c.info_foreground = hsla(T.bg);
    // Components also read this derived copy (tooltips, popups); keep it in step.
    theme.tokens = ThemeTokens::from(theme.colors);
}
