//! Steppers, sortable column headings and checkboxes.

use super::*;

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
