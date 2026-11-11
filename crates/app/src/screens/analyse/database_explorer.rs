//! Local game frequencies and results for the position being analysed.

use chess_core::{display_san, shakmaty::san::San};
use db::MoveStat;
use gpui::{Context, IntoElement, div, prelude::*, px, rgb};
use ui::{
    T,
    widgets::{btn, h_flex, mono, section_label, v_flex},
};

use super::AnalyseScreen;
use crate::screens::database::format_count;

/// Adds the move counts and results for the explorer's total row.
fn total_stats(moves: &[MoveStat]) -> MoveStat {
    moves.iter().fold(
        MoveStat {
            san: String::new(),
            games: 0,
            white_wins: 0,
            draws: 0,
            black_wins: 0,
        },
        |mut total, stat| {
            total.games += stat.games;
            total.white_wins += stat.white_wins;
            total.draws += stat.draws;
            total.black_wins += stat.black_wins;
            total
        },
    )
}

/// Shares of all recorded games, keeping unfinished games separate from draws.
fn result_shares(stat: &MoveStat) -> [f64; 4] {
    let unknown = stat
        .games
        .saturating_sub(stat.white_wins + stat.draws + stat.black_wins);
    [stat.white_wins, stat.draws, stat.black_wins, unknown]
        .map(|count| count as f64 / stat.games.max(1) as f64)
}

/// Shows White wins, draws and Black wins with exact percentages available on hover.
fn results_bar(stat: &MoveStat) -> gpui::AnyElement {
    let shares = result_shares(stat);
    let tooltip = ui::i18n::tf(
        "White wins: {}%, draws: {}%, Black wins: {}%, unfinished: {}%",
        &shares
            .map(|share| format!("{:.1}", share * 100.))
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
    );
    h_flex()
        .id(gpui::SharedString::from(format!(
            "database-results-{}",
            stat.san
        )))
        .flex_1()
        .min_w_0()
        .h(px(20.))
        .border_1()
        .border_color(rgb(T.border_strong))
        .rounded(px(4.))
        .overflow_hidden()
        .tooltip(move |window, cx| {
            gpui_component::tooltip::Tooltip::new(tooltip.clone()).build(window, cx)
        })
        .children(
            shares
                .into_iter()
                .enumerate()
                .filter(|(_, share)| *share > 0.)
                .map(|(index, share)| {
                    let (bg, fg) = match index {
                        0 => (T.fg, T.bg),
                        1 => (T.selected, T.fg),
                        2 => (T.bg, T.fg),
                        _ => (T.surface_2, T.fg_2),
                    };
                    h_flex()
                        .h_full()
                        .flex_grow(share as f32)
                        .flex_basis(px(0.))
                        .min_w_0()
                        .overflow_hidden()
                        .justify_center()
                        .bg(rgb(bg))
                        .text_color(rgb(fg))
                        .when(share >= 0.2, |part| {
                            part.child(
                                mono(if index == 3 {
                                    "—".to_owned()
                                } else {
                                    format!("{:.1}%", share * 100.)
                                })
                                .text_size(px(10.)),
                            )
                        })
                }),
        )
        .into_any_element()
}

impl AnalyseScreen {
    /// Lists every recorded continuation, with the same database choice as the moves table.
    pub(super) fn render_database_explorer(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let total = total_stats(&self.reference);
        let loading = self.db_task.is_some();
        let database = self.explorer_database(cx);
        let pos = self.analysed_pos();
        let notation = self.model.read(cx).config.board.notation;
        let mut body = v_flex()
            .min_w_0()
            .child(
                h_flex()
                    .px(px(16.))
                    .h(px(40.))
                    .justify_between()
                    .child(self.render_explorer_picker(180., cx))
                    .child(
                        mono(if loading {
                            ui::i18n::t("Loading…").to_owned()
                        } else {
                            ui::i18n::tf("{} games", &[&format_count(total.games)])
                        })
                        .text_size(px(12.))
                        .text_color(rgb(T.muted)),
                    ),
            )
            .child(
                h_flex()
                    .h(px(28.))
                    .px(px(16.))
                    .gap(px(8.))
                    .bg(rgb(T.surface_2))
                    .child(div().w(px(56.)).flex_none().child(section_label("Move")))
                    .child(div().w(px(48.)).flex_none().child(section_label("Played")))
                    .child(div().w(px(82.)).flex_none().child(section_label("Games")))
                    .child(section_label("White / Draw / Black")),
            );
        let mut rows = v_flex()
            .id("database-explorer-rows")
            .max_h(px(280.))
            .overflow_y_scroll();
        for (index, stat) in self.reference.iter().enumerate() {
            let mv = stat
                .san
                .parse::<San>()
                .ok()
                .and_then(|san| san.to_move(&pos).ok());
            let label = display_san(&stat.san, mv.as_ref(), notation);
            let san = stat.san.clone();
            let this = cx.entity();
            rows = rows.child(
                h_flex()
                    .id(("database-move", index))
                    .h(px(32.))
                    .px(px(16.))
                    .gap(px(8.))
                    .border_b_1()
                    .border_color(rgb(T.divider))
                    .hover(|row| row.bg(rgb(T.hover)))
                    .child(
                        btn(("database-play", index), label)
                            .mini()
                            .mono()
                            .w(56.)
                            .disabled(mv.is_none())
                            .on_click(move |window, cx| {
                                this.update(cx, |screen, cx| {
                                    let mut line: Vec<String> = screen
                                        .analysed_san_path()
                                        .into_iter()
                                        .skip(screen.base_san_path().len())
                                        .collect();
                                    line.push(san.clone());
                                    if let Some(&node) = screen.add_line(&line).last() {
                                        screen.jump(node, window, cx);
                                    }
                                });
                            }),
                    )
                    .child(
                        mono(format!(
                            "{:.1}%",
                            stat.games as f64 * 100. / total.games.max(1) as f64
                        ))
                        .text_size(px(12.))
                        .w(px(48.))
                        .flex_none()
                        .whitespace_nowrap(),
                    )
                    .child(
                        mono(format_count(stat.games))
                            .text_size(px(12.))
                            .w(px(82.))
                            .flex_none()
                            .whitespace_nowrap(),
                    )
                    .child(results_bar(stat)),
            );
        }
        if let Some(error) = &self.reference_error {
            rows = rows.child(
                div()
                    .p(px(16.))
                    .text_size(px(13.))
                    .text_color(rgb(T.warn))
                    .child(ui::i18n::tf("Could not query database: {}", &[error])),
            );
        } else if self.reference.is_empty() {
            rows = rows.child(
                div()
                    .p(px(16.))
                    .text_size(px(13.))
                    .text_color(rgb(T.muted))
                    .child(ui::i18n::t(if loading {
                        "Loading…"
                    } else if database.is_none() {
                        "Add a database in Settings › Databases."
                    } else {
                        "No games reach this position."
                    })),
            );
        }
        body = body.child(rows);
        if total.games > 0 {
            body = body.child(
                h_flex()
                    .h(px(32.))
                    .px(px(16.))
                    .gap(px(8.))
                    .border_t_1()
                    .border_color(rgb(T.border))
                    .child(div().w(px(112.)).flex_none().child(section_label("Total")))
                    .child(
                        mono(format_count(total.games))
                            .text_size(px(12.))
                            .w(px(82.))
                            .flex_none()
                            .whitespace_nowrap(),
                    )
                    .child(results_bar(&total)),
            );
        }
        body.into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Totals preserve every outcome and leave unfinished games out of draws.
    #[test]
    fn totals_and_result_shares_include_unfinished_games() {
        let total = total_stats(&[
            MoveStat {
                san: "e4".into(),
                games: 10,
                white_wins: 4,
                draws: 3,
                black_wins: 2,
            },
            MoveStat {
                san: "d4".into(),
                games: 10,
                white_wins: 2,
                draws: 3,
                black_wins: 4,
            },
        ]);
        assert_eq!(total.games, 20);
        assert_eq!(result_shares(&total), [0.3, 0.3, 0.3, 0.1]);
        assert_eq!(result_shares(&total_stats(&[])), [0.; 4]);
    }
}
