//! Small building blocks of the Tournament screen: cards, tiles, headings and meters.

use super::*;

pub(super) fn card() -> Div {
    v_flex()
        .gap(px(14.))
        .p(px(20.))
        .rounded(px(12.))
        .bg(rgb(T.tour_card))
        .border_1()
        .border_color(rgb(T.tour_card_border))
}

/// A tile inside a panel.
pub(super) fn tile(label: &str) -> Div {
    v_flex()
        .min_w_0()
        .overflow_hidden()
        .gap(px(4.))
        .p(px(14.))
        .rounded(px(10.))
        .bg(rgb(T.tour_tile))
        .border_1()
        .border_color(rgb(T.tour_tile_border))
        .child(
            div()
                .text_size(px(12.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(T.faint))
                .child(SharedString::from(ui::i18n::t(label).to_uppercase())),
        )
}

pub(super) fn heading(text: impl Into<SharedString>) -> Div {
    let text: SharedString = text.into();
    div()
        .text_size(px(13.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgb(T.muted))
        .child(SharedString::from(ui::i18n::t(&text).to_uppercase()))
}

pub(super) fn small(text: impl Into<SharedString>) -> Div {
    div()
        .text_size(px(12.))
        .text_color(rgb(T.muted))
        .child(ui::i18n::t(&text.into()).to_owned())
}

pub(super) fn big(text: impl Into<SharedString>) -> Div {
    div()
        .text_size(px(30.))
        .font_weight(FontWeight::BOLD)
        .child(ui::i18n::t(&text.into()).to_owned())
}

/// A thin bar filled to `fraction`.
pub(super) fn meter(fraction: f32, colour: u32) -> Div {
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

pub(super) fn swatch(colour: u32) -> Div {
    div()
        .size(px(8.))
        .flex_none()
        .rounded_full()
        .bg(rgb(colour))
}

pub(super) fn name_chip(name: &str, colour: u32, size: f32) -> Div {
    h_flex()
        .gap(px(8.))
        .items_center()
        .child(swatch(colour))
        .child(
            div()
                .text_size(px(size))
                .font_weight(FontWeight::BOLD)
                .child(SharedString::from(name.to_owned())),
        )
}

pub(super) fn ratio(label: &str, value: String) -> Div {
    h_flex()
        .justify_between()
        .gap(px(12.))
        .text_size(px(13.))
        .child(
            div()
                .text_color(rgb(T.muted))
                .child(SharedString::from(ui::i18n::t(label).to_owned())),
        )
        .child(div().font_family(MONO).child(SharedString::from(value)))
}

pub(super) fn def_row(label: &str, value: String) -> Div {
    h_flex()
        .justify_between()
        .gap(px(12.))
        .text_size(px(13.))
        .child(
            div()
                .flex_none()
                .text_color(rgb(T.muted))
                .child(SharedString::from(ui::i18n::t(label).to_owned())),
        )
        .child(
            div()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .text_right()
                .child(SharedString::from(value)),
        )
}
