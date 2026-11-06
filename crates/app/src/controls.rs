//! Thin wrappers styling the component library's stateful inputs for Outpost.

use gpui::{App, Div, Entity, IntoElement, SharedString, Window, div, prelude::*, px, rgb};
use gpui_component::{
    Sizable as _,
    input::{Input, InputState},
    select::{SearchableVec, Select, SelectState},
};
use ui::{T, widgets::h_flex};

pub type TextSelect = Entity<SelectState<SearchableVec<SharedString>>>;

/// A single-line input in a dark well, 36px tall, radius 4.
pub fn text_input(state: &Entity<InputState>) -> impl IntoElement + use<> {
    div()
        .h(px(36.))
        .w_full()
        .flex()
        .items_center()
        .rounded(px(4.))
        .bg(rgb(T.bg))
        .child(Input::new(state).appearance(true).h(px(36.)).text_size(px(14.)))
}

pub fn new_input(
    value: impl Into<SharedString>,
    placeholder: &'static str,
    window: &mut Window,
    cx: &mut App,
) -> Entity<InputState> {
    let value = value.into();
    cx.new(|cx| {
        let mut state = InputState::new(window, cx).placeholder(placeholder);
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
            SearchableVec::new(items),
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
        .child(Select::new(state).with_size(gpui_component::Size::Medium))
}

/// A small form label above a field.
pub fn field(label: impl Into<SharedString>, control: impl IntoElement) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(6.))
        .min_w_0()
        .child(
            h_flex()
                .text_size(px(13.))
                .text_color(rgb(T.muted))
                .child(label.into()),
        )
        .child(control)
}
