//! Line preview: the selected line played out on a small board.

use super::*;

impl AnalyseScreen {
    /// Who found the selected line and how deep: the engine's icon and name, then the
    /// line's depth.
    pub(super) fn render_line_source(&self, row: &MoveRow, cx: &gpui::App) -> gpui::AnyElement {
        let live = self.live.read(cx);
        let engine = row
            .engine_id
            .and_then(|id| live.engines.iter().find(|engine| engine.config.id == id));
        let stats = engine.and_then(|engine| engine.stats());
        let depth = match stats.and_then(|stats| stats.seldepth) {
            Some(sel) if sel > row.depth => format!("{}/{sel}", row.depth),
            _ => row.depth.to_string(),
        };
        // The Engine panel shows the search's other figures.
        let figures: Vec<(&'static str, String)> = vec![("Depth", depth)];
        let name = engine.map(|engine| {
            let config = &engine.config;
            format!("{} {}", config.name, config.version)
                .trim()
                .to_owned()
        });
        // Each figure is a tiny caption over its value, like the Engine panel's columns.
        let figure = |(label, value): (&'static str, String)| {
            v_flex()
                .flex_none()
                .child(
                    div()
                        .text_size(px(10.))
                        .text_color(rgb(T.faint))
                        .child(ui::i18n::t(label).to_uppercase()),
                )
                .child(
                    mono(value)
                        .whitespace_nowrap()
                        .text_size(px(12.))
                        .text_color(rgb(T.fg_2)),
                )
        };
        h_flex()
            .flex_wrap()
            .gap_x(px(14.))
            .gap_y(px(4.))
            .items_center()
            .text_size(px(12.))
            .children(engine.map(|engine| {
                h_flex()
                    .flex_none()
                    .gap(px(6.))
                    .child(crate::controls::engine_icon(&engine.config, 14.))
                    .children(
                        name.clone()
                            .map(|name| div().text_color(rgb(T.fg_2)).child(name)),
                    )
            }))
            .children(figures.into_iter().map(figure))
            .into_any_element()
    }

    pub(super) fn render_line_preview(
        &mut self,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let this = cx.entity();
        let explorer = self.explorer_database(cx);
        let model = self.model.read(cx);
        let demo = model.demo;
        // Database names are often lowercase; this label is not uppercased like the table's.
        let reference_title = explorer
            .map(|entry| {
                let title = reference_column_title(&entry.name);
                let mut chars = title.chars();
                chars
                    .next()
                    .map(|first| first.to_uppercase().chain(chars).collect())
                    .unwrap_or(title)
            })
            .unwrap_or_else(|| ui::i18n::t("Reference").to_owned());
        let ply = self.analysed_ply();
        let white_to_move = self.analysed_pos().turn().is_white();
        let selected = self.selected_row().cloned();
        let meta = selected
            .as_ref()
            .filter(|row| row.depth > 0)
            .map(|row| self.render_line_source(row, cx));
        // The move itself, large, with its score and how it compares with the best line.
        let headline = selected.as_ref().map(|row| {
            let score = row.score.map(|score| {
                h_flex()
                    .flex_none()
                    .h(px(26.))
                    .px(px(8.))
                    .rounded(px(4.))
                    .bg(rgb(T.fg))
                    .text_color(rgb(T.bg))
                    .child(
                        mono(score.display())
                            .text_size(px(13.))
                            .font_weight(FontWeight::SEMIBOLD),
                    )
            });
            let verdict = row.quality.map(|quality| {
                let text = match quality {
                    Quality::Best => "Best move",
                    Quality::Good => "Good",
                    Quality::Inaccuracy => "Inaccuracy",
                    Quality::Mistake => "Mistake",
                    Quality::Blunder => "Blunder",
                };
                div()
                    .flex_none()
                    .text_size(px(12.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(rgb(if quality == Quality::Best {
                        T.muted
                    } else {
                        quality_color(quality)
                    }))
                    .child(ui::i18n::t(text).to_owned())
            });
            h_flex()
                .mt(px(14.))
                .gap(px(10.))
                .children(score)
                .child(
                    mono(chess_core::notation::numbered(ply + 1, &row.san))
                        .flex_1()
                        .min_w_0()
                        .text_size(px(18.))
                        .font_weight(FontWeight::SEMIBOLD),
                )
                .children(verdict)
        });
        let wdl = self.live.read(cx).wdl_engine().map(|_| {
            let wdl = selected.as_ref().and_then(|row| row.wdl);
            let labels = match wdl_percentages(wdl) {
                Some([w, d, l]) => h_flex()
                    .justify_between()
                    .child(format!("{} {w:.1}%", ui::i18n::t("White")))
                    .child(format!("{} {d:.1}%", ui::i18n::t("Draw")))
                    .child(format!("{} {l:.1}%", ui::i18n::t("Black"))),
                None => h_flex().child(ui::i18n::t("Waiting for WDL data")),
            };
            let bar = if wdl_percentages(wdl).is_some() {
                wdl_bar(wdl)
            } else {
                div()
                    .h(px(10.))
                    .w_full()
                    .rounded(px(1.))
                    .bg(rgb(T.divider))
                    .into_any_element()
            };
            v_flex()
                .gap(px(6.))
                .mt(px(12.))
                .child(bar)
                .child(labels.text_size(px(12.)).text_color(rgb(T.muted)))
        });
        // Hook for future move explanations; only demo mode has text today.
        let explanation = selected
            .as_ref()
            .filter(|_| demo)
            .and_then(|row| crate::demo::explanation(&row.san))
            .map(|text| {
                div()
                    .mt(px(12.))
                    .text_size(px(13.))
                    .line_height(px(19.))
                    .text_color(rgb(T.fg_2))
                    .child(text)
            });
        // What the databases and the repertoire say about the selected move.
        // A game count and the score, as two figures side by side.
        let stat_text = |stat: Option<&MoveStat>| -> gpui::AnyElement {
            match stat {
                None => div()
                    .child(ui::i18n::t("No games").to_owned())
                    .into_any_element(),
                Some(stat) => h_flex()
                    .gap(px(12.))
                    .child(format!("{} {}", stat.games, ui::i18n::t("games")))
                    .child(mono(format!("{:.0}%", stat.score_percent(white_to_move))))
                    .into_any_element(),
            }
        };
        let (rep_text, rep_color) = match selected.as_ref().map_or(RepTag::None, |row| row.rep) {
            RepTag::Main => ("Main", T.good),
            RepTag::Side => ("Side line", T.fg_2),
            RepTag::Deviate => ("You deviate", T.warn),
            RepTag::None => ("Not in repertoire", T.fg_2),
        };
        let fact = |title: String, value: gpui::AnyElement, color: u32| {
            v_flex()
                .flex_1()
                .min_w_0()
                .gap(px(3.))
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(rgb(T.muted))
                        .overflow_hidden()
                        .text_ellipsis()
                        .whitespace_nowrap()
                        .child(title),
                )
                .child(
                    div()
                        .text_size(px(13.))
                        .text_color(rgb(color))
                        .whitespace_nowrap()
                        .child(value),
                )
        };
        let my_moves = self.model.read(cx).config.view.my_moves;
        let facts = selected.as_ref().map(|row| {
            h_flex()
                .mt(px(16.))
                .pt(px(12.))
                .gap(px(12.))
                .border_t_1()
                .border_color(rgb(T.divider))
                .child(fact(
                    reference_title.clone(),
                    stat_text(row.reference.as_ref()),
                    T.fg_2,
                ))
                .when(my_moves, |facts| {
                    facts
                        .child(fact(
                            ui::i18n::t("My games").to_owned(),
                            stat_text(row.mine.as_ref()),
                            T.fg_2,
                        ))
                        .child(fact(
                            ui::i18n::t("Repertoire").to_owned(),
                            div()
                                .child(ui::i18n::t(rep_text).to_owned())
                                .into_any_element(),
                            rep_color,
                        ))
                })
        });
        // A previewed line counts even where the table is empty, such as after the
        // mating move, so the whole line can still be played.
        let has_row = self.action_line().is_some();
        let pv = (selected.is_some() || self.pv_view.is_some())
            .then(|| self.render_pv(selected.as_ref(), cx));
        panel()
            .rounded_none()
            .flex_none()
            .p(px(16.))
            .child(
                h_flex()
                    .flex_wrap()
                    .gap(px(8.))
                    .justify_between()
                    .child(section_label("Line preview"))
                    .children(meta),
            )
            .children(headline)
            .children(wdl)
            .child(
                div()
                    .mt(px(12.))
                    .h(px(PV_INSET_H))
                    .flex_none()
                    .px(px(10.))
                    .py(px(6.))
                    .bg(rgb(T.bg))
                    .border_1()
                    .border_color(rgb(T.divider))
                    .when(pv.is_none(), |inset| {
                        inset.child(
                            div()
                                .py(px(4.))
                                .text_size(px(13.))
                                .text_color(rgb(T.muted))
                                .child(ui::i18n::t(
                                    "Click a move in the table to preview its line",
                                )),
                        )
                    })
                    .children(pv),
            )
            .children(self.render_kibitz_row(cx))
            .children(explanation)
            .child(
                h_flex()
                    .mt(px(12.))
                    .gap(px(6.))
                    .child(
                        btn("play-line", "Play on board")
                            .flex_1()
                            .pad(9.)
                            .disabled(!has_row)
                            .tooltip(ui::i18n::t("Play line on board"))
                            .on_click({
                                let this = this.clone();
                                move |window, cx| {
                                    this.update(cx, |screen, cx| screen.play_line(window, cx))
                                }
                            }),
                    )
                    .child(
                        btn("add-variation", "Add variation")
                            .flex_1()
                            .pad(9.)
                            .disabled(!has_row)
                            .tooltip(ui::i18n::t("Add as variation"))
                            .on_click({
                                let this = this.clone();
                                move |_, cx| this.update(cx, |screen, cx| screen.add_variation(cx))
                            }),
                    )
                    .child(
                        btn("add-rep", "To repertoire")
                            .flex_1()
                            .pad(9.)
                            .disabled(!has_row)
                            .tooltip(ui::i18n::t("Add to repertoire"))
                            .on_click({
                                let this = this.clone();
                                move |_, cx| {
                                    this.update(cx, |screen, cx| screen.add_to_repertoire(cx))
                                }
                            }),
                    )
                    .child(
                        icon_btn("copy-line", "icons/copy.svg")
                            .w(36.)
                            .pad(0.)
                            .disabled(!has_row)
                            .tooltip(ui::i18n::t("Copy line"))
                            .on_click({
                                let this = this.clone();
                                move |_, cx| this.update(cx, |screen, cx| screen.copy_pv(cx))
                            }),
                    ),
            )
            .children(facts)
    }
}
