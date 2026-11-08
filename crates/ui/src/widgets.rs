//! Small flat widgets following the design system: buttons, segmented controls,
//! section labels, panels, table cells.

use std::rc::Rc;

use gpui::{
    AnyElement, App, Div, ElementId, FontWeight, Hsla, IntoElement, MouseButton, Pixels,
    RenderOnce, SharedString, Window, div, prelude::*, px, rgb, svg,
};

use crate::theme::{MONO, SANS, T};

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
        let (bg, hover, fg, border): (Option<u32>, u32, u32, Option<u32>) = match self.kind {
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
