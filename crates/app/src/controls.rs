//! Thin wrappers styling the component library's stateful inputs for Fast.

use gpui::{
    AnyElement, App, Context, Div, Entity, Focusable as _, IntoElement, RenderOnce, SharedString,
    Window, div, prelude::*, px, rgb,
};
use gpui_component::{
    input::{Input, InputState},
    select::{SearchableVec, Select, SelectState},
};
use ui::{
    T,
    widgets::{Listen, Segment, TextExt, check, h_flex, segmented},
};

pub type TextSelect = Entity<SelectState<SearchableVec<SharedString>>>;

/// A single-line input in a dark well: 36px tall, radius 4, fg border when focused.
#[derive(IntoElement)]
pub struct TextField {
    state: Entity<InputState>,
    height: f32,
    suffix: Option<SharedString>,
    icon: Option<&'static str>,
    hint: Option<AnyElement>,
    invalid: bool,
    mono: bool,
}

pub fn text_input(state: &Entity<InputState>) -> TextField {
    TextField {
        state: state.clone(),
        height: 34.,
        suffix: None,
        icon: None,
        hint: None,
        invalid: false,
        mono: false,
    }
}

impl TextField {
    /// Uses aligned data typography for numbers and FENs.
    pub fn mono(mut self) -> Self {
        self.mono = true;
        self
    }

    pub fn height(mut self, height: f32) -> Self {
        self.height = height;
        self
    }

    pub fn suffix(mut self, suffix: Option<impl Into<SharedString>>) -> Self {
        self.suffix = suffix.map(Into::into);
        self
    }

    /// Shows a small icon before the text, such as a magnifier on a search box.
    pub fn icon(mut self, path: &'static str) -> Self {
        self.icon = Some(path);
        self
    }

    /// Marks the field's value as unreadable with a `warn` outline.
    pub fn invalid(mut self, invalid: bool) -> Self {
        self.invalid = invalid;
        self
    }

    /// Shows a hint at the right end, such as the shortcut that focuses the field. It hides
    /// while the field has focus.
    pub fn hint(mut self, hint: impl IntoElement) -> Self {
        self.hint = Some(hint.into_any_element());
        self
    }
}

impl RenderOnce for TextField {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let focused = self.state.read(cx).focus_handle(cx).is_focused(window);
        div()
            .h(px(self.height))
            .w_full()
            .flex()
            .items_center()
            .rounded(px(4.))
            .bg(rgb(T.bg))
            .border_1()
            .when(focused, |field| field.border_2())
            .border_color(rgb(if self.invalid {
                T.warn
            } else if focused {
                T.accent
            } else {
                T.border_strong
            }))
            .child(
                Input::new(&self.state)
                    .when(self.mono, |input| input.font_family(ui::MONO))
                    .appearance(false)
                    .text_size(px(13.))
                    .when_some(self.icon, |input, path| {
                        input.prefix(ui::widgets::icon(path, px(14.), T.muted))
                    })
                    .when_some(self.suffix, |input, suffix| {
                        input.suffix(div().text_as(13., T.muted).child(suffix))
                    })
                    .when_some(self.hint.filter(|_| !focused), |input, hint| {
                        input.suffix(hint)
                    }),
            )
    }
}

/// A 13px checkbox whose new state is stored with `set`, redrawing the screen.
pub fn toggle<V: 'static, F: Fn(&mut V, bool) + 'static>(
    id: &'static str,
    on: bool,
    label: &'static str,
    cx: &mut Context<V>,
    set: F,
) -> impl IntoElement + use<V, F> {
    let this = cx.entity();
    check(id, on, label)
        .text(13.)
        .on_change(this.listen_with(move |screen, on, _, cx| {
            set(screen, on);
            cx.notify();
        }))
}

/// A segmented control whose choice is stored with `set`, redrawing the screen.
pub fn choice<V: 'static, C: Copy + PartialEq + 'static>(
    id: &'static str,
    options: impl IntoIterator<Item = impl Into<Segment<C>>>,
    selected: C,
    cx: &mut Context<V>,
    set: impl Fn(&mut V, C) + 'static,
) -> Div {
    segmented(
        id,
        options,
        selected,
        cx.entity().listen_with(move |screen, value, _, cx| {
            set(screen, value);
            cx.notify();
        }),
    )
}

/// The unit after a hash size field: MB, with the size in GB beside it once it
/// reaches 1024 MB, so 10000 reads as 9.8 GB.
pub fn hash_suffix(value: &str) -> SharedString {
    match value.trim().parse::<i64>() {
        Ok(mb) if mb >= 1024 => format!("MB ({})", engine::format_hash(mb)).into(),
        _ => "MB".into(),
    }
}

pub fn new_input(
    value: impl Into<SharedString>,
    placeholder: &'static str,
    window: &mut Window,
    cx: &mut App,
) -> Entity<InputState> {
    let value = value.into();
    cx.new(|cx| {
        let mut state = InputState::new(window, cx).placeholder(ui::i18n::t(placeholder));
        state.set_value(value, window, cx);
        state
    })
}

pub fn new_select(
    items: Vec<SharedString>,
    selected: usize,
    window: &mut Window,
    cx: &mut App,
) -> TextSelect {
    cx.new(|cx| {
        SelectState::new(
            SearchableVec::new(
                items
                    .into_iter()
                    .map(|item| ui::i18n::t(&item).to_owned().into())
                    .collect::<Vec<SharedString>>(),
            ),
            Some(gpui_component::IndexPath::new(selected)),
            window,
            cx,
        )
    })
}

pub fn select_el(state: &TextSelect) -> impl IntoElement + use<> {
    div()
        .w_full()
        .bg(rgb(T.bg))
        .rounded(px(4.))
        .text_size(px(14.))
        .child(Select::new(state).h(px(34.)).text_size(px(13.5)))
}

/// A small form label above a field.
pub fn field(label: impl Into<SharedString>, control: impl IntoElement) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(4.))
        .min_w_0()
        .child(
            h_flex()
                .text_size(px(12.5))
                .text_color(rgb(T.muted))
                .child(ui::i18n::t(&label.into()).to_owned()),
        )
        .child(control)
}

/// An engine's picture: its custom icon if the file exists, otherwise a rook.
pub fn engine_icon(config: &engine::EngineConfig, size: f32) -> gpui::AnyElement {
    let frame = div()
        .size(px(size))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(4.))
        .overflow_hidden();
    match config.icon.as_ref().filter(|path| path.is_file()) {
        Some(path) => frame
            .child(gpui::img(path.clone()).size_full())
            .into_any_element(),
        None => frame
            .bg(rgb(T.bg))
            .border_1()
            .border_color(rgb(T.border_strong))
            .child(
                gpui::svg()
                    .path("pieces/merida/wR.svg")
                    .size(px(size * 0.72))
                    .text_color(rgb(T.fg_2)),
            )
            .into_any_element(),
    }
}

/// Opens a native file or folder picker and puts the chosen path in the input.
pub fn browse_path(
    id: &'static str,
    model: &Entity<crate::model::AppModel>,
    input: &Entity<InputState>,
    title: &'static str,
    directory: bool,
) -> impl IntoElement + use<> {
    let model = model.clone();
    let input = input.clone();
    ui::widgets::btn(id, "Browse…")
        .compact()
        .on_click(move |window, cx| {
            let handle = window.window_handle();
            let input = input.clone();
            let on_path = move |path: std::path::PathBuf, cx: &mut App| {
                let _ = handle.update(cx, |_, window, cx| {
                    input.update(cx, |input, cx| {
                        input.set_value(path.display().to_string(), window, cx);
                    });
                });
            };
            if directory {
                crate::model::pick_directory(model.clone(), title, cx, on_path);
            } else {
                crate::model::pick_path(model.clone(), title, None, cx, on_path);
            }
        })
}

/// A duration with an editable value and a choice of time units.
pub struct TimeInput {
    value: Entity<InputState>,
    unit: TextSelect,
}

impl TimeInput {
    /// Creates a duration in milliseconds, seconds, minutes or hours.
    pub fn new(value: &str, unit: usize, window: &mut Window, cx: &mut App) -> Self {
        Self {
            value: new_input(value, "0.1s", window, cx),
            unit: new_select(
                ["Milliseconds", "Seconds", "Minutes", "Hours"]
                    .map(SharedString::from)
                    .to_vec(),
                unit,
                window,
                cx,
            ),
        }
    }

    /// Keeps the exact typed value and selected unit when saving a clock.
    pub fn saved(&self, cx: &App) -> (String, usize) {
        (
            self.value.read(cx).value().to_string(),
            self.unit
                .read(cx)
                .selected_index(cx)
                .map_or(1, |index| index.row),
        )
    }

    /// Reads the duration as milliseconds, honoring an explicit unit suffix.
    pub fn milliseconds(&self, cx: &App) -> Option<u64> {
        let unit = self.unit.read(cx).selected_index(cx)?.row;
        parse_time_ms(
            self.value.read(cx).value().as_ref(),
            *[1, 1_000, 60_000, 3_600_000].get(unit)?,
        )
    }

    /// Shows the duration beside its unit selector.
    pub fn render(&self, label: &'static str) -> Div {
        field(
            label,
            h_flex()
                .gap(px(6.))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(text_input(&self.value).mono()),
                )
                .child(div().w(px(116.)).child(select_el(&self.unit))),
        )
    }
}

/// Separate base time and increment fields for a game clock.
pub struct ClockInput {
    pub base: TimeInput,
    pub increment: TimeInput,
}

impl ClockInput {
    /// Creates a clock with base time in the chosen unit and increment in seconds.
    pub fn new(
        base: &str,
        unit: usize,
        increment: &str,
        window: &mut Window,
        cx: &mut App,
    ) -> Self {
        Self {
            base: TimeInput::new(base, unit, window, cx),
            increment: TimeInput::new(increment, 1, window, cx),
        }
    }

    /// Reads a positive base time and a nonnegative increment in milliseconds.
    pub fn milliseconds(&self, cx: &App) -> Option<(u64, u64)> {
        let base = self.base.milliseconds(cx)?;
        (base > 0).then_some((base, self.increment.milliseconds(cx)?))
    }

    /// Shows separate rows so both fields fit narrow setup panels.
    pub fn render(&self) -> Div {
        ui::widgets::v_flex()
            .gap(px(8.))
            .child(self.base.render("Time"))
            .child(self.increment.render("Increment"))
    }
}

/// Formats milliseconds as exact decimal seconds for engine time controls.
pub fn time_seconds(ms: u64) -> String {
    if ms.is_multiple_of(1_000) {
        (ms / 1_000).to_string()
    } else {
        format!("{}.{:03}", ms / 1_000, ms % 1_000)
            .trim_end_matches('0')
            .to_owned()
    }
}

/// Parses fractional durations and optional ms, s, min or h suffixes without losing milliseconds.
fn parse_time_ms(text: &str, default_unit: u64) -> Option<u64> {
    let text = text.trim();
    let (number, unit) = [
        ("ms", 1),
        ("min", 60_000),
        ("s", 1_000),
        ("m", 60_000),
        ("h", 3_600_000),
    ]
    .into_iter()
    .find_map(|(suffix, unit)| text.strip_suffix(suffix).map(|n| (n.trim(), unit)))
    .unwrap_or((text, default_unit));
    let (whole, fraction) = number.split_once('.').unwrap_or((number, ""));
    if (whole.is_empty() && fraction.is_empty())
        || !whole
            .bytes()
            .chain(fraction.bytes())
            .all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let whole = if whole.is_empty() {
        0
    } else {
        whole.parse::<u64>().ok()?
    };
    let scale = 10_u64.checked_pow(fraction.len().try_into().ok()?)?;
    let fraction = if fraction.is_empty() {
        0
    } else {
        fraction.parse::<u64>().ok()?
    };
    let fractional_ms = fraction.checked_mul(unit)?;
    if !fractional_ms.is_multiple_of(scale) {
        return None;
    }
    whole.checked_mul(unit)?.checked_add(fractional_ms / scale)
}

/// A name being typed in place of a label, such as a tab or a database in a list.
/// `key` says which item is being renamed. The field starts with the current name
/// selected for typing, and pressing Enter or leaving the field finishes the rename.
pub struct Rename<K> {
    pub key: K,
    pub input: Entity<InputState>,
    _finish: gpui::Subscription,
}

impl<K> Rename<K> {
    /// Opens the field on `current` and focuses it. `finish` runs on Enter or blur.
    pub fn start<T: 'static>(
        key: K,
        current: String,
        window: &mut Window,
        cx: &mut gpui::Context<T>,
        finish: fn(&mut T, &mut Window, &mut gpui::Context<T>),
    ) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx));
        input.update(cx, |input, cx| {
            input.set_value(current, window, cx);
            input.focus(window, cx);
        });
        let _finish = cx.subscribe_in(&input, window, move |this, _, event, window, cx| {
            if matches!(
                event,
                gpui_component::input::InputEvent::PressEnter { .. }
                    | gpui_component::input::InputEvent::Blur
            ) {
                finish(this, window, cx);
            }
        });
        Self {
            key,
            input,
            _finish,
        }
    }

    /// The typed name, without surrounding spaces.
    pub fn text(&self, cx: &App) -> String {
        self.input.read(cx).value().trim().to_owned()
    }
}

#[cfg(test)]
mod time_tests {
    use super::{parse_time_ms, time_seconds};

    /// Engine arguments retain fractional seconds without floating-point rounding.
    #[test]
    fn seconds_preserve_milliseconds() {
        for ms in [0, 1, 100, 1_001, 60_000, u64::MAX / 1_000] {
            assert_eq!(parse_time_ms(&time_seconds(ms), 1_000), Some(ms));
        }
    }

    /// Fractional values and explicit units retain millisecond precision.
    #[test]
    fn durations_accept_units_and_fractions() {
        assert_eq!(parse_time_ms("0.1s", 60_000), Some(100));
        assert_eq!(parse_time_ms(" 0.1 s ", 60_000), Some(100));
        assert_eq!(parse_time_ms("1.5", 60_000), Some(90_000));
        assert_eq!(parse_time_ms("250ms", 1_000), Some(250));
        assert_eq!(parse_time_ms(".5min", 1_000), Some(30_000));
        assert_eq!(parse_time_ms("1h", 1), Some(3_600_000));
        assert_eq!(parse_time_ms("0", 1_000), Some(0));
        for invalid in [
            "",
            ".",
            "-1",
            "NaN",
            "inf",
            "1+2",
            "0.0001s",
            "18446744073709551615h",
        ] {
            assert_eq!(parse_time_ms(invalid, 1_000), None, "{invalid}");
        }
    }
}
