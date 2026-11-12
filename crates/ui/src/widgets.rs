//! Small flat widgets following the design system: buttons, segmented controls,
//! section labels, panels, table cells.

use std::{rc::Rc, time::Duration};

use gpui::{
    Animation, AnimationExt as _, AnyElement, App, Div, ElementId, FontWeight, Hsla, IntoElement,
    MouseButton, Pixels, RenderOnce, SharedString, Window, div, prelude::*, px, relative, rgb, svg,
};

use crate::theme::{MONO, SANS, T};

mod buttons;
mod dialogs;
mod ext;
mod inputs;

pub use self::{buttons::*, dialogs::*, ext::*, inputs::*};

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
