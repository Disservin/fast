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
    /// What is active or chosen: the current move, the selected row, primary buttons.
    pub accent: u32,
    pub accent_hover: u32,
    /// Text and icons on the accent.
    pub accent_fg: u32,
    /// MultiPV arrow and label colours by move quality, best to worst.
    pub q_best: u32,
    pub q_good: u32,
    pub q_inaccuracy: u32,
    pub q_mistake: u32,
    pub q_blunder: u32,
}

/// The active theme.
/// Slate surfaces with a cool tint, a step lighter than near-black so panels read as
/// layers, and one blue accent for whatever is active.
pub const T: Theme = Theme {
    bg: 0x14171D,
    surface: 0x1C2028,
    surface_2: 0x181C23,
    hover: 0x252B35,
    selected: 0x1F3047,
    border: 0x2A303A,
    border_strong: 0x353D49,
    divider: 0x232832,
    fg: 0xEDF0F4,
    fg_2: 0xC9D0D9,
    muted: 0x9DA7B4,
    faint: 0x818B98,
    good: 0x46C25A,
    warn: 0xF0A33A,
    arrow: 0xE8892B,
    bar: 0x6F7987,
    last_move_light: 0xF3E3A6,
    last_move_dark: 0xC9B56E,
    accent: 0x5AA2FF,
    accent_hover: 0x7AB5FF,
    accent_fg: 0x0B1626,
    q_best: 0x5AA2FF,
    q_good: 0x46C25A,
    q_inaccuracy: 0xE6C229,
    q_mistake: 0xE8892B,
    q_blunder: 0xE5534B,
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
    c.ring = hsla(T.accent);
    c.caret = hsla(T.fg);
    c.selection = hsla(0x2E4F7C);
    c.primary = hsla(T.accent);
    c.primary_hover = hsla(T.accent_hover);
    c.primary_active = hsla(T.accent_hover);
    c.primary_foreground = hsla(T.accent_fg);
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
    c.list_active_border = hsla(T.accent);
    c.slider_bar = hsla(T.accent);
    c.slider_thumb = hsla(T.accent);
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
    c.info = hsla(T.accent);
    c.info_foreground = hsla(T.accent_fg);
    // Components also read this derived copy (tooltips, popups); keep it in step.
    theme.tokens = ThemeTokens::from(theme.colors);
}
