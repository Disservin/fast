//! The status bar along the bottom of Analyse: the shown position's FEN with a copy
//! button, and the main keyboard shortcuts.

use super::*;
use crate::screens::settings::shortcut_hint;

impl AnalyseScreen {
    /// The status bar: FEN on the left, shortcut hints on the right. The hints follow
    /// the shortcuts set in Settings and leave out any that have none.
    pub(super) fn render_status_bar(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let this = cx.entity();
        let fen = GameTree::fen_of(&self.shown_pos());
        let shortcuts = self.model.read(cx).config.shortcuts.clone();
        let hint = |actions: &[&str], label: &'static str| {
            let keys: Vec<_> = actions
                .iter()
                .filter_map(|action| shortcuts.get(*action).filter(|key| !key.is_empty()))
                .map(|key| shortcut_hint(key))
                .collect();
            (!keys.is_empty()).then(|| {
                h_flex()
                    .flex_none()
                    .gap(px(4.))
                    .children(keys)
                    .child(div().text_as(11., T.muted).child(ui::i18n::t(label)))
            })
        };
        h_flex()
            .flex_none()
            .h(px(30.))
            .px(px(16.))
            .gap(px(8.))
            .bg(rgb(T.surface))
            .border_t_as(T.border)
            .child(div().flex_none().text_as(10., T.faint).child("FEN"))
            .child(mono(fen).flex_1().min_w_0().truncate().text_as(12., T.fg_2))
            .child(
                icon_btn("status-copy-fen", "icons/copy.svg")
                    .mini()
                    .w(24.)
                    .pad(0.)
                    .kind(ui::widgets::BtnKind::Ghost)
                    .tooltip("Copy FEN")
                    .on_click(this.listen(|screen, _, cx| screen.copy_fen(cx))),
            )
            .child(div().w(px(12.)))
            .children(hint(&["prev_move", "next_move"], "Navigate"))
            .children(hint(&["toggle_engine"], "Engines"))
            .children(hint(&["pause_engine"], "Pause"))
            .children(hint(&["flip_board"], "Flip"))
    }
}
