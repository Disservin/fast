//! Layout helpers shared by the settings pages: sections, titles and labelled rows.

use super::*;

pub(super) fn section(title: &'static str) -> Div {
    panel().flex().flex_col().p(px(16.)).child(
        div()
            .mb(px(10.))
            .text_size(px(15.))
            .font_weight(FontWeight::BOLD)
            .child(ui::i18n::t(title)),
    )
}

pub(super) fn page_title(title: &'static str) -> Div {
    div()
        .text_size(px(24.))
        .font_weight(FontWeight::SEMIBOLD)
        .child(ui::i18n::t(title))
}

/// Two-column option row: label + hint on the left, control on the right.
pub(super) fn option_row(
    label: &'static str,
    hint: Option<&'static str>,
    control: impl IntoElement,
) -> Div {
    h_flex()
        .min_h(px(44.))
        .py(px(5.))
        .border_b_as(T.divider)
        .child(
            v_flex()
                .col_w(232.)
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_size(px(14.))
                        .child(ui::i18n::t(label)),
                )
                .children(hint.map(|hint| {
                    div()
                        .text_as(13., T.muted)
                        .pr(px(12.))
                        .child(ui::i18n::t(hint))
                })),
        )
        .child(control)
}

pub(super) fn detail_row(
    label: &str,
    hint: Option<&'static str>,
    control: impl IntoElement,
) -> Div {
    h_flex()
        .min_h(px(60.))
        .py(px(5.))
        .border_b_as(T.divider)
        .child(
            v_flex()
                .col_w(236.)
                .child(
                    div()
                        .font_weight(FontWeight::MEDIUM)
                        .text_size(px(14.))
                        .child(SharedString::from(ui::i18n::t(label).to_owned())),
                )
                .children(hint.map(|hint| div().text_as(13., T.muted).child(ui::i18n::t(hint)))),
        )
        .child(div().flex_1().min_w_0().child(control))
}

/// A small delete button.
pub(super) fn icon_x(id: impl Into<gpui::ElementId>) -> ui::widgets::Btn {
    ui::widgets::icon_btn(id, "icons/x.svg")
        .w(22.)
        .h(28.)
        .pad(0.)
        .tooltip(ui::i18n::t("Delete"))
}
