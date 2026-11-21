//! Above and below the board: who is to move with the evaluation and what it means,
//! the players with their material, and the legend for the engine arrows.

use super::*;

/// Words for a White-relative evaluation, as a player would say it.
pub(super) fn assessment(score: Score) -> String {
    let side = |white: bool| ui::i18n::t(if white { "White" } else { "Black" });
    if let Score::Mate(mate) = score {
        return ui::i18n::t("{} mates in {}")
            .replacen("{}", side(mate > 0), 1)
            .replacen("{}", &mate.abs().to_string(), 1);
    }
    let chances = engine::quality::winning_chances(score);
    let template = match chances.abs() {
        c if c < 0.1 => return ui::i18n::t("Equal position").to_owned(),
        c if c < 0.25 => "Slight edge for {}",
        c if c < 0.5 => "Clear advantage for {}",
        _ => "{} is winning",
    };
    ui::i18n::t(template).replace("{}", side(chances > 0.))
}

/// The colour that marks an engine in arrows and legends: its own arrow colour, else
/// one of the palette's engine colours by its place in the list.
pub(super) fn engine_colour(config: &engine::EngineConfig, index: usize) -> u32 {
    config
        .arrow_color
        .unwrap_or([T.engine_a, T.engine_b, T.warn, T.good][index % 4])
}

impl AnalyseScreen {
    /// The score the header shows: the main engine's, else the stored one.
    pub(super) fn shown_score(&self, cx: &gpui::App) -> Option<Score> {
        if self.editor.is_some() {
            return None;
        }
        self.live
            .read(cx)
            .primary()
            .and_then(|engine| engine.lines.get(&1))
            .and_then(|line| line.score)
            .or_else(|| {
                let cp = self.evals.get(&self.current)?;
                self.pv_view
                    .is_none()
                    .then(|| match self.mates.get(&self.current) {
                        Some(mate) => Score::Mate(*mate),
                        None => Score::Cp(*cp),
                    })
            })
    }

    /// The best move of each engine that has one, with the engine's name and the colour
    /// its arrow is drawn in: the engine's own colour, or with quality arrows on, the
    /// move's quality unless the engine has an arrow colour set.
    pub(super) fn engine_best_moves(&self, cx: &gpui::App) -> Vec<(String, String, u32)> {
        let quality_arrows = self.model.read(cx).config.view.quality_arrows;
        let live = self.live.read(cx);
        live.engines
            .iter()
            .enumerate()
            .filter_map(|(index, engine)| {
                let san = engine.lines.get(&1)?.sans.first()?.clone();
                let quality = self
                    .rows
                    .iter()
                    .find(|row| row.engine_id == Some(engine.config.id) && row.san == san)
                    .and_then(|row| row.quality);
                let colour = match (quality_arrows, engine.config.arrow_color, quality) {
                    (true, None, Some(quality)) => quality_color(quality),
                    _ => engine_colour(&engine.config, index),
                };
                Some((engine.config.name.clone(), san, colour))
            })
            .collect()
    }

    /// The header over the board: the move just played and the move number, who is to
    /// move, and on the right what the evaluation means beside the number itself.
    pub(super) fn render_board_header(
        &self,
        end: Option<chess_core::GameEnd>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let shown = self.shown_pos();
        let turn = if shown.turn().is_white() {
            ui::i18n::t("White")
        } else {
            ui::i18n::t("Black")
        };
        let status = if self.editor.is_some() {
            ui::i18n::t("Edit position").to_owned()
        } else if let Some(end) = end {
            end.describe().to_owned()
        } else {
            ui::i18n::t("{} to move").replace("{}", turn)
        };
        let ply = self.tree.node(self.current).ply;
        let context = if self.editor.is_some() {
            None
        } else if let Some(preview) = self.pv_preview_label(ply) {
            Some(preview)
        } else if self.current == GameTree::ROOT {
            Some(ui::i18n::t("Starting position").to_owned())
        } else {
            Some(
                ui::i18n::t("After {}, move {}")
                    .replacen("{}", &self.tree.move_label(self.current), 1)
                    .replacen("{}", &(ply / 2 + 1).to_string(), 1),
            )
        };
        let score = self.shown_score(cx).filter(|_| end.is_none());
        let eval_text = match end.filter(|_| self.editor.is_none()) {
            // A finished game shows its result, not an evaluation.
            Some(end) => end.result().replace("1/2", "½"),
            None => score.map_or_else(|| "—".to_owned(), Score::display),
        };
        h_flex()
            .mt(px(10.))
            .mb(px(4.))
            .gap(px(12.))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .h(px(18.))
                            .truncate()
                            .text_as(12., T.muted)
                            .children(context),
                    )
                    .child(
                        div()
                            .truncate()
                            .text_size(px(20.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(status),
                    ),
            )
            .children(score.map(|score| {
                div()
                    .flex_none()
                    .text_as(13., T.fg_2)
                    .whitespace_nowrap()
                    .child(assessment(score))
            }))
            .child(
                mono(eval_text)
                    .flex_none()
                    .px(px(10.))
                    .py(px(4.))
                    .rounded(px(4.))
                    .bg(rgb(T.surface_2))
                    .text_size(px(18.))
                    .font_weight(FontWeight::SEMIBOLD),
            )
    }

    /// Under the board: a swatch for each engine's arrow with its best move, then the
    /// last-move highlight.
    pub(super) fn render_arrow_legend(&self, cx: &mut Context<Self>) -> Option<gpui::Div> {
        let engines = self.engine_best_moves(cx);
        let last = self.current != GameTree::ROOT;
        if engines.is_empty() && !last {
            return None;
        }
        let swatch = |colour: u32| {
            div()
                .flex_none()
                .w(px(12.))
                .h(px(3.))
                .rounded(px(2.))
                .bg(rgb(colour))
        };
        let theme = self.model.read(cx).config.board.prefs().theme;
        let (light, _) = theme.colors();
        let mut row = h_flex()
            .flex_wrap()
            .gap_x(px(14.))
            .gap_y(px(2.))
            .mt(px(4.))
            .text_as(12., T.muted);
        for (name, san, colour) in engines {
            row = row.child(
                h_flex()
                    .gap(px(6.))
                    .child(swatch(colour))
                    .child(name)
                    .child(mono(san).text_color(rgb(T.fg_2))),
            );
        }
        Some(row.when(last, |row| {
            row.child(
                h_flex()
                    .gap(px(6.))
                    .child(
                        div()
                            .flex_none()
                            .size(px(10.))
                            .rounded(px(2.))
                            .bg(rgb(ui::board::last_move_color(light)))
                            .border_as(T.border_strong),
                    )
                    .child(ui::i18n::t("Last move")),
            )
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assessments_name_the_better_side() {
        assert_eq!(assessment(Score::Cp(10)), "Equal position");
        assert_eq!(assessment(Score::Cp(-60)), "Slight edge for Black");
        assert_eq!(assessment(Score::Cp(150)), "Clear advantage for White");
        assert_eq!(assessment(Score::Cp(-600)), "Black is winning");
        assert_eq!(assessment(Score::Mate(-3)), "Black mates in 3");
    }
}
