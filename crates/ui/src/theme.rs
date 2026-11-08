//! Application palettes shared by custom views and library components.

use gpui::{App, Hsla, rgb};
use gpui_component::{Theme as ComponentTheme, ThemeTokens};

pub const SANS: &str = "Geist";
pub const MONO: &str = "Geist Mono";

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
    /// Tournament tab: page background, cards and the tiles inside them.
    pub tour_bg: u32,
    pub tour_card: u32,
    pub tour_card_border: u32,
    pub tour_tile: u32,
    pub tour_tile_border: u32,
    /// Engine A (the first engine of a match) and engine B.
    pub engine_a: u32,
    pub engine_b: u32,
    /// Status pill and "to move" clock.
    pub live_bg: u32,
    pub live_fg: u32,
    /// Stop button.
    pub danger_bg: u32,
    pub danger_border: u32,
    pub danger_fg: u32,
    /// Result chips: engine A win, engine B win, draw.
    pub chip_a_bg: u32,
    pub chip_a_fg: u32,
    pub chip_b_bg: u32,
    pub chip_b_fg: u32,
    pub chip_draw_bg: u32,
    pub chip_draw_fg: u32,
}

const SLATE: Theme = Theme {
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
    tour_bg: 0x0F1115,
    tour_card: 0x171A21,
    tour_card_border: 0x262A33,
    tour_tile: 0x12151B,
    tour_tile_border: 0x232731,
    engine_a: 0x4C8DFF,
    engine_b: 0xF0883E,
    live_bg: 0x12321F,
    live_fg: 0x56D364,
    danger_bg: 0x3A1717,
    danger_border: 0x6E2A2A,
    danger_fg: 0xFF9B9B,
    chip_a_bg: 0x15284A,
    chip_a_fg: 0x9CC2FF,
    chip_b_bg: 0x3B2614,
    chip_b_fg: 0xF6B27F,
    chip_draw_bg: 0x232731,
    chip_draw_fg: 0x9AA1AD,
};

/// Persisted application appearance. Selecting it also sets a matching board default.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
#[repr(usize)]
pub enum UiTheme {
    Slate,
    Paper,
    Sand,
    #[default]
    Forest,
    Plum,
    Obsidian,
}

impl UiTheme {
    pub const ALL: [Self; 6] = [
        Self::Slate,
        Self::Paper,
        Self::Sand,
        Self::Forest,
        Self::Plum,
        Self::Obsidian,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Slate => "Slate",
            Self::Paper => "Paper",
            Self::Sand => "Sand",
            Self::Forest => "Forest",
            Self::Plum => "Plum",
            Self::Obsidian => "Obsidian",
        }
    }

    pub fn palette(self) -> &'static Theme {
        &PALETTES[self as usize]
    }

    pub fn is_light(self) -> bool {
        matches!(self, Self::Paper | Self::Sand)
    }
}

// Every palette changes the full surface hierarchy, typography, and accent.
const PALETTES: [Theme; 6] = [
    SLATE,
    Theme {
        bg: 0xF3F5F8,
        surface: 0xFFFFFF,
        surface_2: 0xEAF0F5,
        hover: 0xE1E8F1,
        selected: 0xDCE8FC,
        border: 0xD3DBE6,
        border_strong: 0xA6B4C7,
        divider: 0xE1E6EE,
        fg: 0x172339,
        fg_2: 0x34455F,
        muted: 0x53657E,
        faint: 0x61718A,
        accent: 0x245DC1,
        accent_hover: 0x184A9F,
        accent_fg: 0xFFFFFF,
        good: 0x237C42,
        warn: 0x9A580C,
        arrow: 0xBD5C16,
        bar: 0x74849B,
        q_best: 0x245DC1,
        q_good: 0x237C42,
        q_inaccuracy: 0x8F7100,
        q_mistake: 0xBD5C16,
        q_blunder: 0xC02C3B,
        tour_bg: 0xF3F5F8,
        tour_card: 0xFFFFFF,
        tour_card_border: 0xD3DBE6,
        tour_tile: 0xEAF0F5,
        tour_tile_border: 0xE1E6EE,
        engine_a: 0x245DC1,
        engine_b: 0xBD5C16,
        live_bg: 0xD9F0E0,
        live_fg: 0x237C42,
        danger_bg: 0xF8E1E3,
        danger_border: 0xE2A3AA,
        danger_fg: 0xC02C3B,
        chip_a_bg: 0xDCE8FC,
        chip_a_fg: 0x184A9F,
        chip_b_bg: 0xF9E3D2,
        chip_b_fg: 0x9A4A12,
        chip_draw_bg: 0xE1E6EE,
        chip_draw_fg: 0x53657E,
        ..SLATE
    },
    Theme {
        bg: 0xF3EBDD,
        surface: 0xFFFAEF,
        surface_2: 0xEDE2CF,
        hover: 0xE7D8BF,
        selected: 0xEAD5BD,
        border: 0xD6C5A9,
        border_strong: 0xB09A7C,
        divider: 0xE3D5BF,
        fg: 0x382B22,
        fg_2: 0x594537,
        muted: 0x74604D,
        faint: 0x7E6A56,
        accent: 0x994A2C,
        accent_hover: 0x7D3821,
        accent_fg: 0xFFFFFF,
        good: 0x477039,
        warn: 0x925C0C,
        arrow: 0xAD551B,
        bar: 0x9A8266,
        q_best: 0x994A2C,
        q_good: 0x477039,
        q_inaccuracy: 0x8A710F,
        q_mistake: 0xAD551B,
        q_blunder: 0xB32E41,
        tour_bg: 0xF3EBDD,
        tour_card: 0xFFFAEF,
        tour_card_border: 0xD6C5A9,
        tour_tile: 0xEDE2CF,
        tour_tile_border: 0xE3D5BF,
        engine_a: 0x2F5FA8,
        engine_b: 0xAD551B,
        live_bg: 0xDCEBD3,
        live_fg: 0x477039,
        danger_bg: 0xF6DCD5,
        danger_border: 0xD9A193,
        danger_fg: 0xB32E41,
        chip_a_bg: 0xDCE6F4,
        chip_a_fg: 0x2A4F8C,
        chip_b_bg: 0xF6E2CE,
        chip_b_fg: 0x8A4416,
        chip_draw_bg: 0xE7D8BF,
        chip_draw_fg: 0x74604D,
        ..SLATE
    },
    Theme {
        bg: 0x0B1311,
        surface: 0x111C19,
        surface_2: 0x0E1815,
        hover: 0x1C2B26,
        selected: 0x1F3329,
        border: 0x22332C,
        border_strong: 0x34493F,
        divider: 0x1A2823,
        fg: 0xDDE6DE,
        fg_2: 0xBCCABE,
        muted: 0x93A598,
        faint: 0x84988A,
        accent: 0x8FBF74,
        accent_hover: 0xA5D18A,
        accent_fg: 0x0F1A0B,
        good: 0x6FBF98,
        warn: 0xD9AC66,
        arrow: 0xD9965A,
        bar: 0x63806E,
        q_best: 0x8FBF74,
        q_good: 0x6FBF98,
        tour_bg: 0x0B1311,
        tour_card: 0x111C19,
        tour_card_border: 0x22332C,
        tour_tile: 0x0E1815,
        tour_tile_border: 0x1A2823,
        engine_a: 0x6E9FE6,
        engine_b: 0xD9965A,
        live_bg: 0x15291F,
        live_fg: 0x6FBF98,
        danger_bg: 0x2C1A18,
        danger_border: 0x5A302C,
        danger_fg: 0xE09595,
        chip_a_bg: 0x172940,
        chip_a_fg: 0x9DBDEB,
        chip_b_bg: 0x32220F,
        chip_b_fg: 0xE2A574,
        chip_draw_bg: 0x1A2823,
        chip_draw_fg: 0x93A598,
        ..SLATE
    },
    Theme {
        bg: 0x211627,
        surface: 0x302037,
        surface_2: 0x281B2E,
        hover: 0x47304F,
        selected: 0x55345D,
        border: 0x503757,
        border_strong: 0x6B4B73,
        divider: 0x402B47,
        fg: 0xF7ECF6,
        fg_2: 0xE3CDE3,
        muted: 0xC3A4C5,
        faint: 0xAE8EB2,
        accent: 0xE4A1CD,
        accent_hover: 0xF3BCDF,
        accent_fg: 0x30172B,
        good: 0x85D0AF,
        warn: 0xF1BE7A,
        arrow: 0xEEA16F,
        bar: 0xA184AA,
        q_best: 0xE4A1CD,
        q_good: 0x85D0AF,
        tour_bg: 0x211627,
        tour_card: 0x302037,
        tour_card_border: 0x503757,
        tour_tile: 0x281B2E,
        tour_tile_border: 0x402B47,
        engine_a: 0x7FAEF5,
        engine_b: 0xF0A060,
        live_bg: 0x22402F,
        live_fg: 0x85D0AF,
        danger_bg: 0x3D1F2C,
        danger_border: 0x6E3044,
        danger_fg: 0xF5A3B5,
        chip_a_bg: 0x233759,
        chip_a_fg: 0xB8D2FF,
        chip_b_bg: 0x44281F,
        chip_b_fg: 0xF8BE94,
        chip_draw_bg: 0x402B47,
        chip_draw_fg: 0xC3A4C5,
        ..SLATE
    },
    Theme {
        bg: 0x08090B,
        surface: 0x101114,
        surface_2: 0x0C0D10,
        hover: 0x202226,
        selected: 0x222C3A,
        border: 0x26282D,
        border_strong: 0x3A3D44,
        divider: 0x1D1F24,
        fg: 0xF1F2F4,
        fg_2: 0xCED1D7,
        muted: 0xA2A7B1,
        faint: 0x858B96,
        accent: 0x91B7FF,
        accent_hover: 0xB0CCFF,
        accent_fg: 0x0A1426,
        tour_bg: 0x08090B,
        tour_card: 0x101114,
        tour_card_border: 0x26282D,
        tour_tile: 0x0C0D10,
        tour_tile_border: 0x1D1F24,
        chip_draw_bg: 0x202226,
        chip_draw_fg: 0xA2A7B1,
        ..SLATE
    },
];

static ACTIVE: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(UiTheme::Forest as usize);

/// Shared palette accessor. Dereferencing always reads the current immutable palette.
/// This keeps paint callbacks and every custom widget in sync without cached colors.
pub struct ActiveTheme;
pub static T: ActiveTheme = ActiveTheme;

impl std::ops::Deref for ActiveTheme {
    type Target = Theme;

    fn deref(&self) -> &'static Theme {
        &PALETTES[ACTIVE.load(std::sync::atomic::Ordering::Relaxed)]
    }
}

pub fn set_theme(choice: UiTheme, cx: &mut App) {
    ACTIVE.store(choice as usize, std::sync::atomic::Ordering::Relaxed);
    ComponentTheme::change(
        if choice.is_light() {
            gpui_component::ThemeMode::Light
        } else {
            gpui_component::ThemeMode::Dark
        },
        None,
        cx,
    );
    apply_component_theme(cx);
    cx.refresh_windows();
}

fn hsla(color: u32) -> Hsla {
    rgb(color).into()
}

/// Maps Fast's tokens onto the component library so inputs, selects, sliders
/// and checkboxes read as part of one design.
pub fn apply_component_theme(cx: &mut App) {
    ComponentTheme::update(cx, |theme| {
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
        c.selection = hsla(T.selected);
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
        c.danger = hsla(T.q_blunder);
        c.danger_foreground = hsla(T.bg);
        c.info = hsla(T.accent);
        c.info_foreground = hsla(T.accent_fg);
        c.accordion = hsla(T.surface);
        c.button = hsla(T.surface);
        c.group_box = hsla(T.surface);
        c.list_head = hsla(T.surface);
        c.sidebar = hsla(T.surface);
        c.tab = hsla(T.surface);
        c.table = hsla(T.surface);
        c.table_head = hsla(T.surface);
        c.table_foot = hsla(T.surface);
        c.title_bar = hsla(T.surface);
        c.status_bar = hsla(T.surface);
        c.list_even = hsla(T.surface_2);
        c.skeleton = hsla(T.surface_2);
        c.tab_bar = hsla(T.surface_2);
        c.tab_bar_segmented = hsla(T.surface_2);
        c.table_even = hsla(T.surface_2);
        c.button_hover = hsla(T.hover);
        c.button_active = hsla(T.selected);
        c.drop_target = hsla(T.selected);
        c.sidebar_accent = hsla(T.selected);
        c.tab_active = hsla(T.selected);
        c.table_active = hsla(T.selected);
        c.table_hover = hsla(T.selected);
        c.button_foreground = hsla(T.fg);
        c.group_box_foreground = hsla(T.fg);
        c.sidebar_foreground = hsla(T.fg);
        c.tab_active_foreground = hsla(T.fg);
        c.table_head_foreground = hsla(T.fg);
        c.table_foot_foreground = hsla(T.fg);
        c.tab_foreground = hsla(T.fg_2);
        c.sidebar_border = hsla(T.border);
        c.table_row_border = hsla(T.border);
        c.title_bar_border = hsla(T.border);
        c.status_bar_border = hsla(T.border);
        c.drag_border = hsla(T.accent);
        c.link = hsla(T.accent);
        c.link_active = hsla(T.accent);
        c.sidebar_primary = hsla(T.accent);
        c.progress_bar = hsla(T.accent);
        c.table_active_border = hsla(T.accent);
        c.link_hover = hsla(T.accent_hover);
        c.sidebar_primary_foreground = hsla(T.accent_fg);
        c.switch_thumb = hsla(T.bg);
        c.switch = hsla(T.border_strong);
        c.button_primary = hsla(T.accent);
        c.button_primary_hover = hsla(T.accent_hover);
        c.button_primary_active = hsla(T.accent);
        c.button_primary_foreground = hsla(T.accent_fg);
        c.button_secondary = hsla(T.surface);
        c.button_secondary_hover = hsla(T.surface);
        c.button_secondary_active = hsla(T.surface);
        c.button_secondary_foreground = hsla(T.fg);
        c.button_danger = hsla(T.q_blunder);
        c.button_danger_hover = hsla(T.q_blunder);
        c.button_danger_active = hsla(T.q_blunder);
        c.button_danger_foreground = hsla(T.bg);
        c.button_info = hsla(T.accent);
        c.button_info_hover = hsla(T.accent_hover);
        c.button_info_active = hsla(T.accent);
        c.button_info_foreground = hsla(T.accent_fg);
        c.button_success = hsla(T.good);
        c.button_success_hover = hsla(T.good);
        c.button_success_active = hsla(T.good);
        c.button_success_foreground = hsla(T.bg);
        c.button_warning = hsla(T.warn);
        c.button_warning_hover = hsla(T.warn);
        c.button_warning_active = hsla(T.warn);
        c.button_warning_foreground = hsla(T.bg);
        // Components also read this derived copy (tooltips, popups); keep it in step.
        theme.tokens = ThemeTokens::from(theme.colors);
    });
}
