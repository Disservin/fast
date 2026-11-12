//! Buttons, header tabs and segmented controls.

use super::*;

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
pub(super) struct HeaderTab {
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

/// One option of a segmented control: its value, its label and whether it can be
/// chosen. Built from `(value, label)` or `(value, label, disabled)`.
pub struct Segment<V> {
    value: V,
    label: SharedString,
    disabled: bool,
}

impl<V, S: Into<SharedString>> From<(V, S)> for Segment<V> {
    fn from((value, label): (V, S)) -> Self {
        Self {
            value,
            label: label.into(),
            disabled: false,
        }
    }
}

impl<V, S: Into<SharedString>> From<(V, S, bool)> for Segment<V> {
    fn from((value, label, disabled): (V, S, bool)) -> Self {
        Self {
            value,
            label: label.into(),
            disabled,
        }
    }
}

/// A row of buttons where exactly one option is selected. Labels are translated.
pub fn segmented<V: Copy + PartialEq + 'static>(
    id: &'static str,
    options: impl IntoIterator<Item = impl Into<Segment<V>>>,
    selected: V,
    on_select: impl Fn(V, &mut Window, &mut App) + 'static,
) -> Div {
    let on_select = Rc::new(on_select);
    let mut row = h_flex().gap(px(6.));
    for (index, option) in options.into_iter().enumerate() {
        let Segment {
            value,
            label,
            disabled,
        } = option.into();
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
