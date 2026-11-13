//! The selected candidate’s interactive principal variation, inside the moves table.

use super::*;

impl AnalyseScreen {
    /// Shows the selected row's line and actions without repeating its table figures.
    pub(super) fn render_line_preview(
        &mut self,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let selected = match self.pv_view.as_ref().filter(|_| self.pv_kibitz) {
            Some(view) => view.source_row(self.selected_row()),
            None => self.selected_row(),
        }
        .cloned();
        let has_row = self.action_line().is_some();
        let pv = (selected.is_some() || self.pv_view.is_some())
            .then(|| self.render_pv(selected.as_ref(), cx));
        v_flex()
            .flex_none()
            .px(px(16.))
            .py(px(8.))
            .border_t_as(T.divider)
            .child(
                div()
                    .when(pv.is_none(), |line| {
                        line.child(
                            div()
                                .text_as(13., T.muted)
                                .child(ui::i18n::t("Select a row to see its engine line")),
                        )
                    })
                    .children(pv),
            )
            .children(self.render_kibitz_row(cx))
            .child(self.render_preview_actions(has_row, cx))
    }

    /// Play on board, Add variation, To repertoire and Copy line for the previewed line.
    fn render_preview_actions(&self, has_row: bool, cx: &mut Context<Self>) -> gpui::Div {
        let this = cx.entity();
        h_flex()
            .mt(px(6.))
            .gap(px(6.))
            .flex_wrap()
            .child(
                btn("play-line", "Play on board")
                    .compact()
                    .pad(9.)
                    .disabled(!has_row)
                    .tooltip(ui::i18n::t("Play line on board"))
                    .on_click(this.listen(move |screen, window, cx| screen.play_line(window, cx))),
            )
            .child(
                btn("add-variation", "Add variation")
                    .compact()
                    .pad(9.)
                    .disabled(!has_row)
                    .tooltip(ui::i18n::t("Add as variation"))
                    .on_click(this.listen(move |screen, _, cx| screen.add_variation(cx))),
            )
            .child(
                btn("add-rep", "To repertoire")
                    .compact()
                    .pad(9.)
                    .disabled(!has_row)
                    .tooltip(ui::i18n::t("Add to repertoire"))
                    .on_click(this.listen(move |screen, _, cx| screen.add_to_repertoire(cx))),
            )
            .child(
                icon_btn("copy-line", "icons/copy.svg")
                    .compact()
                    .w(28.)
                    .pad(0.)
                    .disabled(!has_row)
                    .tooltip(ui::i18n::t("Copy line"))
                    .on_click(this.listen(move |screen, _, cx| screen.copy_pv(cx))),
            )
    }
}
