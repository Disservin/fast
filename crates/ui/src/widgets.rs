//! Small flat widgets following the design system: buttons, segmented controls,
//! section labels, panels, table cells.

use std::{rc::Rc, time::Duration};

use gpui::{
    Animation, AnimationExt as _, AnyElement, App, Div, ElementId, FontWeight, Hsla, IntoElement,
    MouseButton, Pixels, RenderOnce, SharedString, Window, div, prelude::*, px, relative, rgb, svg,
};

use crate::theme::{MONO, SANS, T};

/// Sets text size and colour together, the pair nearly every label needs.
pub trait TextExt: Styled + Sized {
    /// Text at `size` pixels in the palette colour `colour`, such as `T.muted`.
    fn text_as(self, size: f32, colour: u32) -> Self {
        self.text_size(px(size)).text_color(rgb(colour))
    }
}

impl<E: Styled> TextExt for E {}

pub fn h_flex() -> Div {
    div().flex().flex_row().items_center()
}

pub fn v_flex() -> Div {
    div().flex().flex_col()
}

/// Monospace text run.
pub fn mono(text: impl Into<SharedString>) -> Div {
    div().font_family(MONO).child(text.into())
}

pub fn sans(text: impl Into<SharedString>) -> Div {
    div().font_family(SANS).child(text.into())
}

/// Tracked uppercase section label (11px, 600, muted, 0.08em letter spacing).
/// GPUI text has no letter-spacing, so each glyph is laid out on its own.
pub fn section_label(text: impl AsRef<str>) -> Div {
    let text = crate::i18n::t(text.as_ref()).to_uppercase();
    let mut row = h_flex()
        .flex_none()
        .text_size(px(11.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgb(T.muted))
        .font_family(SANS);
    for ch in text.chars() {
        row = if ch == ' ' {
            row.child(div().w(px(4.)))
        } else {
            row.child(div().mr(px(0.88)).child(ch.to_string()))
        };
    }
    row
}

/// A small bordered tag, such as a license or a count.
pub fn badge(text: impl Into<SharedString>) -> Div {
    h_flex()
        .flex_none()
        .h(px(22.))
        .px(px(7.))
        .gap(px(5.))
        .rounded(px(4.))
        .bg(rgb(T.selected))
        .border_1()
        .border_color(rgb(T.border_strong))
        .text_size(px(11.))
        .text_color(rgb(T.fg_2))
        .child(text.into())
}

/// A badge holding a muted label and its value, such as `DTZ 21`.
pub fn stat(label: impl Into<SharedString>, value: impl Into<SharedString>) -> Div {
    h_flex()
        .flex_none()
        .h(px(22.))
        .px(px(7.))
        .gap(px(5.))
        .rounded(px(4.))
        .bg(rgb(T.selected))
        .border_1()
        .border_color(rgb(T.border_strong))
        .child(
            div()
                .text_size(px(11.))
                .text_color(rgb(T.muted))
                .child(crate::i18n::t(&label.into()).to_owned()),
        )
        .child(
            mono(value)
                .text_size(px(12.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(T.fg)),
        )
}

/// Bordered surface panel with the standard 6px radius.
pub fn panel() -> Div {
    div()
        .bg(rgb(T.surface))
        .border_1()
        .border_color(rgb(T.border))
        .rounded(px(6.))
}

/// A thin 2px bar along the bottom of a row for work of unknown length: a `divider`
/// track with an `accent` segment sliding along it.
pub fn busy_bar(id: impl Into<ElementId>) -> impl IntoElement {
    div()
        .relative()
        .w_full()
        .h(px(2.))
        .flex_none()
        .overflow_hidden()
        .bg(rgb(T.divider))
        .child(
            div()
                .absolute()
                .top_0()
                .h_full()
                .w(relative(0.3))
                .bg(rgb(T.accent))
                .with_animation(
                    id.into(),
                    Animation::new(Duration::from_millis(1200)).repeat(),
                    |bar, delta| bar.left(relative(delta * 1.3 - 0.3)),
                ),
        )
}

/// A 6px rounded bar filled to `fraction` (0 to 1) in `colour`, for scores and odds.
pub fn meter(fraction: f32, colour: u32) -> Div {
    div()
        .h(px(6.))
        .w_full()
        .rounded(px(3.))
        .bg(rgb(T.tour_tile_border))
        .child(
            div()
                .h_full()
                .rounded(px(3.))
                .bg(rgb(colour))
                .w(relative(fraction.clamp(0., 1.))),
        )
}

/// A thin 2px progress bar: a `divider` track with an `accent` fill for `fraction`
/// of the work, from 0 to 1.
pub fn progress_bar(fraction: f32) -> impl IntoElement {
    div()
        .w_full()
        .h(px(2.))
        .flex_none()
        .bg(rgb(T.divider))
        .child(
            div()
                .h_full()
                .w(relative(fraction.clamp(0., 1.)))
                .bg(rgb(T.accent)),
        )
}

pub fn icon(path: &'static str, size: Pixels, color: u32) -> impl IntoElement {
    svg()
        .path(path)
        .size(size)
        .flex_none()
        .text_color(rgb(color))
}

type ClickHandler = Rc<dyn Fn(&mut Window, &mut App)>;
type ChangeHandler = Rc<dyn Fn(bool, &mut Window, &mut App)>;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum BtnKind {
    Normal,
    Primary,
    /// Segmented-control member that is currently chosen.
    Selected,
    /// Top-bar mode switch's active item.
    Active,
    /// Flat item without a border.
    Ghost,
}

/// The app's button. Focusable, keyboard-activated, 36px tall by default.
#[derive(IntoElement)]
pub struct Btn {
    id: ElementId,
    label: Option<SharedString>,
    icon: Option<&'static str>,
    kind: BtnKind,
    mono: bool,
    height: Pixels,
    width: Option<Pixels>,
    full: bool,
    flex: bool,
    padding: Pixels,
    text_size: Pixels,
    weight: FontWeight,
    disabled: bool,
    on_click: Option<ClickHandler>,
    tooltip: Option<SharedString>,
    dot: Option<u32>,
    left: bool,
}

pub fn btn(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Btn {
    Btn {
        id: id.into(),
        label: Some(crate::i18n::t(&label.into()).to_owned().into()),
        icon: None,
        kind: BtnKind::Normal,
        mono: false,
        height: px(36.),
        width: None,
        full: false,
        flex: false,
        padding: px(13.),
        text_size: px(13.),
        weight: FontWeight::MEDIUM,
        disabled: false,
        on_click: None,
        tooltip: None,
        dot: None,
        left: false,
    }
}

pub fn icon_btn(id: impl Into<ElementId>, path: &'static str) -> Btn {
    Btn {
        label: None,
        icon: Some(path),
        ..btn(id, "")
    }
}

impl Btn {
    pub fn primary(mut self) -> Self {
        self.kind = BtnKind::Primary;
        self
    }

    pub fn kind(mut self, kind: BtnKind) -> Self {
        self.kind = kind;
        self
    }

    /// Selected state for segmented controls.
    pub fn selected(mut self, selected: bool) -> Self {
        if selected {
            self.kind = BtnKind::Selected;
        }
        self
    }

    /// The size for toolbar and panel controls: smaller than a form button, but
    /// with text still at body size so short labels stay legible.
    pub fn compact(mut self) -> Self {
        self.height = px(28.);
        self.padding = px(10.);
        self.text_size = px(13.);
        self
    }

    /// The smallest readable button, for tab strips inside a panel header.
    pub fn mini(mut self) -> Self {
        self.height = px(24.);
        self.padding = px(8.);
        self.text_size = px(12.);
        self
    }

    pub fn mono(mut self) -> Self {
        self.mono = true;
        self
    }

    pub fn h(mut self, height: f32) -> Self {
        self.height = px(height);
        self
    }

    pub fn w(mut self, width: f32) -> Self {
        self.width = Some(px(width));
        self
    }

    pub fn full(mut self) -> Self {
        self.full = true;
        self
    }

    pub fn flex_1(mut self) -> Self {
        self.flex = true;
        self
    }

    pub fn pad(mut self, padding: f32) -> Self {
        self.padding = px(padding);
        self
    }

    pub fn text(mut self, size: f32) -> Self {
        self.text_size = px(size);
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn tooltip(mut self, text: impl Into<SharedString>) -> Self {
        self.tooltip = Some(crate::i18n::t(&text.into()).to_owned().into());
        self
    }

    pub fn on_click(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }

    /// A small status dot before the label.
    /// Shows an icon before the label, as in "+ New tab".
    pub fn icon(mut self, path: &'static str) -> Self {
        self.icon = Some(path);
        self
    }

    /// Lines the icon and label up on the left, for buttons that read like list rows.
    pub fn left(mut self) -> Self {
        self.left = true;
        self
    }

    pub fn dot(mut self, color: u32) -> Self {
        self.dot = Some(color);
        self
    }
}

impl RenderOnce for Btn {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let focus = window
            .use_keyed_state(self.id.clone(), cx, |_, cx| {
                cx.focus_handle().tab_stop(true)
            })
            .read(cx)
            .clone();
        // A disabled filled button would put `faint` text on `accent`, which is
        // unreadable in the light palettes: it looks like a disabled normal one instead.
        let kind = match self.kind {
            BtnKind::Primary | BtnKind::Active if self.disabled => BtnKind::Normal,
            kind => kind,
        };
        let (bg, hover, fg, border): (Option<u32>, u32, u32, Option<u32>) = match kind {
            BtnKind::Normal => (Some(T.surface), T.hover, T.fg, Some(T.border_strong)),
            BtnKind::Primary => (Some(T.accent), T.accent_hover, T.accent_fg, None),
            BtnKind::Selected => (Some(T.selected), T.selected, T.fg, Some(T.accent)),
            BtnKind::Active => (Some(T.accent), T.accent, T.accent_fg, None),
            BtnKind::Ghost => (None, T.hover, T.fg_2, None),
        };
        let fg = if self.disabled { T.faint } else { fg };
        let hover_color: Hsla = rgb(hover).into();
        let on_click = self.on_click.clone();
        let on_key = self.on_click.clone();
        let disabled = self.disabled;
        let mut el = div()
            .id(self.id)
            .key_context("FastButton")
            .track_focus(&focus)
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .h(self.height)
            .rounded(px(6.))
            .text_size(self.text_size)
            .font_weight(self.weight)
            .text_color(rgb(fg))
            .font_family(if self.mono { MONO } else { SANS })
            .whitespace_nowrap();
        if let Some(bg) = bg {
            el = el.bg(rgb(bg));
        }
        if let Some(border) = border {
            el = el.border_1().border_color(rgb(border));
        } else {
            el = el.border_1().border_color(gpui::transparent_black());
        }
        if let Some(width) = self.width {
            el = el.w(width);
        } else if !self.full && !self.flex {
            el = el.px(self.padding);
        }
        if self.full {
            el = el.w_full();
        }
        if self.flex {
            el = el.flex_1().min_w_0();
        }
        if !disabled {
            el = el
                .cursor_pointer()
                .hover(move |style| style.bg(hover_color))
                .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
                .on_click(move |_, window, cx| {
                    if let Some(handler) = &on_click {
                        handler(window, cx);
                    }
                })
                .on_key_down(move |event, window, cx| {
                    let key = event.keystroke.key.as_str();
                    if (key == "enter" || key == "space")
                        && !event.keystroke.modifiers.modified()
                        && let Some(handler) = &on_key
                    {
                        handler(window, cx);
                        cx.stop_propagation();
                    }
                });
        }
        el = el.focus_visible(|style| style.border_2().border_color(rgb(T.accent)));
        if self.left {
            el = el.justify_start().overflow_hidden();
        }
        if let Some(color) = self.dot {
            el = el
                .gap(px(8.))
                .child(div().size(px(8.)).rounded_full().bg(rgb(color)));
        }
        if let Some(path) = self.icon {
            el = el.gap(px(6.)).child(icon(path, px(16.), fg));
        }
        if let Some(label) = self.label
            && !label.is_empty()
        {
            el = el.child(label);
        }
        if let Some(text) = self.tooltip {
            el = el.tooltip(move |window, cx| {
                gpui_component::tooltip::Tooltip::new(text.clone()).build(window, cx)
            });
        }
        el
    }
}

/// A tab in a panel header. It fills the header's height, and the chosen one is in
/// `fg` with a 2px `accent` line along its bottom edge; the others are `muted`.
pub fn header_tab(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    selected: bool,
    on_select: impl Fn(&mut Window, &mut App) + 'static,
) -> impl IntoElement {
    HeaderTab {
        id: id.into(),
        label: crate::i18n::t(&label.into()).to_owned().into(),
        selected,
        on_select: Rc::new(on_select),
    }
}

#[derive(IntoElement)]
struct HeaderTab {
    id: ElementId,
    label: SharedString,
    selected: bool,
    on_select: ClickHandler,
}

impl RenderOnce for HeaderTab {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let focus = window
            .use_keyed_state(self.id.clone(), cx, |_, cx| {
                cx.focus_handle().tab_stop(true)
            })
            .read(cx)
            .clone();
        let (on_click, on_key) = (self.on_select.clone(), self.on_select);
        let hover: Hsla = rgb(T.fg).into();
        div()
            .id(self.id)
            .key_context("FastButton")
            .track_focus(&focus)
            .flex()
            .flex_none()
            .items_center()
            .h_full()
            .px(px(4.))
            .border_b_2()
            .border_color(if self.selected {
                rgb(T.accent).into()
            } else {
                gpui::transparent_black()
            })
            .text_size(px(13.))
            .font_weight(FontWeight::MEDIUM)
            .text_color(rgb(if self.selected { T.fg } else { T.muted }))
            .whitespace_nowrap()
            .cursor_pointer()
            .hover(move |style| style.text_color(hover))
            .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
            .on_click(move |_, window, cx| on_click(window, cx))
            .on_key_down(move |event, window, cx| {
                let key = event.keystroke.key.as_str();
                if (key == "enter" || key == "space") && !event.keystroke.modifiers.modified() {
                    on_key(window, cx);
                    cx.stop_propagation();
                }
            })
            .focus_visible(|style| style.border_2().border_color(rgb(T.accent)).rounded(px(4.)))
            .child(self.label)
    }
}

/// A row of buttons where exactly one option is selected.
pub fn segmented<V: Copy + PartialEq + 'static>(
    id: &'static str,
    options: Vec<(V, SharedString, bool)>,
    selected: V,
    on_select: impl Fn(V, &mut Window, &mut App) + 'static,
) -> Div {
    let on_select = Rc::new(on_select);
    let mut row = h_flex().gap(px(6.));
    for (index, (value, label, disabled)) in options.into_iter().enumerate() {
        let on_select = on_select.clone();
        row = row.child(
            btn((id, index), label)
                .pad(10.)
                .selected(value == selected)
                .disabled(disabled)
                .on_click(move |window, cx| on_select(value, window, cx)),
        );
    }
    row
}

/// A one-line number stepper: `−`, a fixed-width value and `+`, sized to its content.
/// A side whose handler is missing or disabled shows a disabled button.
#[derive(IntoElement)]
pub struct Stepper {
    id: ElementId,
    value: AnyElement,
    value_width: Pixels,
    button: f32,
    down: Option<(bool, ClickHandler)>,
    up: Option<(bool, ClickHandler)>,
    tooltips: (Option<SharedString>, Option<SharedString>),
}

/// A stepper showing `value` as 13px mono text. Buttons are 28px (`compact`) by default.
pub fn stepper(id: impl Into<ElementId>, value: impl Into<SharedString>) -> Stepper {
    Stepper {
        id: id.into(),
        value: mono(value)
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .whitespace_nowrap()
            .text_size(px(13.))
            .into_any_element(),
        value_width: px(44.),
        button: 28.,
        down: None,
        up: None,
        tooltips: (None, None),
    }
}

impl Stepper {
    /// Small 24px buttons, for steppers inside a panel header or a dense row.
    pub fn mini(mut self) -> Self {
        self.button = 24.;
        self
    }

    /// The width of the value between the buttons, so it never moves as it changes.
    pub fn value_width(mut self, width: f32) -> Self {
        self.value_width = px(width);
        self
    }

    /// Replaces the value text with another element, such as a field to type into.
    pub fn value_element(mut self, element: impl IntoElement) -> Self {
        self.value = element.into_any_element();
        self
    }

    /// What `−` does, and whether it can be pressed.
    pub fn on_down(
        mut self,
        enabled: bool,
        handler: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        self.down = Some((enabled, Rc::new(handler)));
        self
    }

    /// What `+` does, and whether it can be pressed.
    pub fn on_up(
        mut self,
        enabled: bool,
        handler: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        self.up = Some((enabled, Rc::new(handler)));
        self
    }

    /// Tooltips for the `−` and `+` buttons.
    pub fn tooltips(mut self, down: impl Into<SharedString>, up: impl Into<SharedString>) -> Self {
        self.tooltips = (Some(down.into()), Some(up.into()));
        self
    }
}

impl RenderOnce for Stepper {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let size = self.button;
        let mini = size < 28.;
        let button = |index: usize,
                      label: &'static str,
                      side: Option<(bool, ClickHandler)>,
                      tooltip: Option<SharedString>| {
            let id = ElementId::NamedChild(
                std::sync::Arc::new(self.id.clone()),
                if index == 0 { "down" } else { "up" }.into(),
            );
            let button = btn(id, label);
            let mut button = if mini {
                button.mini()
            } else {
                button.compact()
            }
            .pad(0.)
            .w(size);
            if let Some(text) = tooltip {
                button = button.tooltip(text);
            }
            match side {
                Some((enabled, handler)) => button
                    .disabled(!enabled)
                    .on_click(move |window, cx| handler(window, cx)),
                None => button.disabled(true),
            }
        };
        let (down_tip, up_tip) = self.tooltips;
        h_flex()
            .flex_none()
            .gap(px(4.))
            .child(button(0, "−", self.down, down_tip))
            .child(
                div()
                    .flex_none()
                    .w(self.value_width)
                    .h(px(size))
                    .child(self.value),
            )
            .child(button(1, "+", self.up, up_tip))
    }
}

/// A table column heading that sorts the table by its column when clicked, or with
/// Enter or Space. `sorted` is `Some(descending)` for the column the table is sorted by,
/// which shows in `fg` with an `accent` arrow; the others are `muted`.
pub fn sort_heading(
    id: impl Into<ElementId>,
    label: impl AsRef<str>,
    sorted: Option<bool>,
    on_click: impl Fn(&mut Window, &mut App) + 'static,
) -> SortHeading {
    SortHeading {
        id: id.into(),
        label: crate::i18n::t(label.as_ref()).to_uppercase().into(),
        sorted,
        on_click: Rc::new(on_click),
    }
}

/// See [`sort_heading`].
#[derive(IntoElement)]
pub struct SortHeading {
    id: ElementId,
    label: SharedString,
    sorted: Option<bool>,
    on_click: ClickHandler,
}

impl RenderOnce for SortHeading {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let focus = window
            .use_keyed_state(self.id.clone(), cx, |_, cx| {
                cx.focus_handle().tab_stop(true)
            })
            .read(cx)
            .clone();
        let arrow = match self.sorted {
            Some(true) => Some("icons/arrow-down.svg"),
            Some(false) => Some("icons/arrow-up.svg"),
            None => None,
        };
        let (click, keys) = (self.on_click.clone(), self.on_click);
        h_flex()
            .id(self.id)
            .key_context("FastButton")
            .track_focus(&focus)
            .h(px(24.))
            .gap(px(3.))
            .px(px(3.))
            // The label lines up with the column's values, not with its own padding.
            .ml(px(-4.))
            .rounded(px(4.))
            .border_1()
            .border_color(gpui::transparent_black())
            .cursor_pointer()
            .hover(|style| style.bg(rgb(T.hover)))
            .focus_visible(|style| style.border_2().border_color(rgb(T.accent)))
            .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
            .on_click(move |_, window, cx| click(window, cx))
            .on_key_down(move |event, window, cx| {
                let key = event.keystroke.key.as_str();
                if (key == "enter" || key == "space") && !event.keystroke.modifiers.modified() {
                    cx.stop_propagation();
                    keys(window, cx);
                }
            })
            .child(
                div()
                    .whitespace_nowrap()
                    .text_size(px(11.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(if arrow.is_some() { T.fg } else { T.muted }))
                    .child(self.label),
            )
            .child(
                div()
                    .size(px(12.))
                    .flex_none()
                    .children(arrow.map(|path| icon(path, px(12.), T.accent))),
            )
    }
}

/// Flat custom checkbox: 16px, filled with fg when checked.
#[derive(IntoElement)]
pub struct Check {
    id: ElementId,
    checked: bool,
    label: Option<SharedString>,
    on_change: Option<ChangeHandler>,
    text_size: Pixels,
}

pub fn check(id: impl Into<ElementId>, checked: bool, label: impl Into<SharedString>) -> Check {
    Check {
        id: id.into(),
        checked,
        label: Some(crate::i18n::t(&label.into()).to_owned().into()),
        on_change: None,
        text_size: px(14.),
    }
}

impl Check {
    pub fn on_change(mut self, handler: impl Fn(bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(handler));
        self
    }

    pub fn text(mut self, size: f32) -> Self {
        self.text_size = px(size);
        self
    }
}

impl RenderOnce for Check {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let focus = window
            .use_keyed_state(self.id.clone(), cx, |_, cx| {
                cx.focus_handle().tab_stop(true)
            })
            .read(cx)
            .clone();
        let checked = self.checked;
        let on_click = self.on_change.clone();
        let on_key = self.on_change.clone();
        let mark = div()
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(px(17.))
            .mt(px(2.))
            .rounded(px(3.))
            .border_1()
            .border_color(rgb(if checked { T.accent } else { T.bar }))
            .bg(rgb(if checked { T.accent } else { T.bg }))
            .when(checked, |mark| {
                mark.child(icon("icons/check.svg", px(13.), T.accent_fg))
            });
        div()
            .id(self.id)
            .key_context("FastButton")
            .track_focus(&focus)
            .flex()
            .w_full()
            .min_w_0()
            .items_start()
            .gap(px(10.))
            .cursor_pointer()
            .rounded(px(4.))
            .border_1()
            .border_color(gpui::transparent_black())
            .focus_visible(|style| style.border_color(rgb(T.accent)))
            .text_size(self.text_size)
            .text_color(rgb(T.fg_2))
            .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
            .on_click(move |_, window, cx| {
                if let Some(handler) = &on_click {
                    handler(!checked, window, cx);
                }
            })
            .on_key_down(move |event, window, cx| {
                if event.keystroke.key == "space"
                    && let Some(handler) = &on_key
                {
                    handler(!checked, window, cx);
                    cx.stop_propagation();
                }
            })
            .child(mark)
            .children(
                self.label
                    .map(|label| div().flex_1().min_w_0().child(label)),
            )
    }
}

/// A key cap for shortcut lists.
pub fn key_cap(text: impl Into<SharedString>) -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .h(px(24.))
        .min_w(px(24.))
        .px(px(7.))
        .rounded(px(4.))
        .bg(rgb(T.selected))
        .border_1()
        .border_color(rgb(T.border_strong))
        .font_family(MONO)
        .text_size(px(12.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgb(T.fg))
        .child(text.into())
}

/// A horizontal divider.
pub fn divider() -> Div {
    div().w_full().h(px(1.)).flex_none().bg(rgb(T.divider))
}

/// Boxes `element` for heterogeneous `children` lists.
pub fn any(element: impl IntoElement) -> AnyElement {
    element.into_any_element()
}

/// A modal surface that closes on Escape or a backdrop click.
#[derive(IntoElement)]
pub struct Modal {
    id: ElementId,
    content: AnyElement,
    on_close: Rc<dyn Fn(&mut App)>,
}

/// Wraps dialog content with shared focus and dismissal behavior.
pub fn modal(
    id: impl Into<ElementId>,
    content: impl IntoElement,
    on_close: impl Fn(&mut App) + 'static,
) -> Modal {
    Modal {
        id: id.into(),
        content: content.into_any_element(),
        on_close: Rc::new(on_close),
    }
}

/// A dialog over the screen: a `surface` panel `width` wide with a 44px header holding
/// `title` and a close button, then `body`. It closes on Escape, the close button or a
/// click on the backdrop, all of which call `on_close`.
pub fn dialog(
    id: impl Into<ElementId>,
    title: impl AsRef<str>,
    width: f32,
    body: impl IntoElement,
    on_close: impl Fn(&mut App) + 'static,
) -> Modal {
    let id = id.into();
    let child =
        |name: &'static str| ElementId::NamedChild(std::sync::Arc::new(id.clone()), name.into());
    let on_close: Rc<dyn Fn(&mut App)> = Rc::new(on_close);
    let close_button = on_close.clone();
    let panel = v_flex()
        .id(child("panel"))
        .w(px(width))
        .max_w_full()
        .max_h_full()
        .overflow_y_scroll()
        .rounded(px(6.))
        .bg(rgb(T.surface))
        .border_1()
        .border_color(rgb(T.border_strong))
        .child(
            panel_header(title).child(
                icon_btn(child("close"), "icons/x.svg")
                    .mini()
                    .tooltip("Close")
                    .on_click(move |_, cx| close_button(cx)),
            ),
        )
        .child(body);
    Modal {
        id,
        content: panel.into_any_element(),
        on_close,
    }
}

/// A panel header: a 44px row with `title` as a section label on the left, the
/// header's children (its actions) on the right, and a 1px bottom border.
pub fn panel_header(title: impl AsRef<str>) -> Div {
    h_flex()
        .h(px(44.))
        .flex_none()
        .px(px(16.))
        .gap(px(6.))
        .border_b_1()
        .border_color(rgb(T.border))
        .child(section_label(title))
        .child(div().flex_1())
}

impl RenderOnce for Modal {
    /// Focuses a newly opened dialog so Escape also works before editing a field.
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let focus = window
            .use_keyed_state(self.id.clone(), cx, |window, cx| {
                let focus = cx.focus_handle();
                focus.focus(window, cx);
                focus
            })
            .read(cx)
            .clone();
        let close = self.on_close;
        let close_key = close.clone();
        div()
            .id(self.id)
            .track_focus(&focus)
            .key_context("Dialog")
            .absolute()
            .inset_0()
            .occlude()
            .flex()
            .items_center()
            .justify_center()
            .p(px(24.))
            .bg(gpui::rgba(0x0000_0099))
            .capture_key_down(move |event, _, cx| {
                if event.keystroke.key == "escape" {
                    close_key(cx);
                    cx.stop_propagation();
                }
            })
            .on_mouse_down(MouseButton::Left, move |_, _, cx| close(cx))
            .child(
                div()
                    .max_w_full()
                    .max_h_full()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(self.content),
            )
    }
}
