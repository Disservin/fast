//! The game review under the board: who searched the line and how deep, each side's
//! accuracy and mistakes, and the graph of the evaluation, time or depth per move.

use super::*;

/// One side's review: accuracy and how many inaccuracies, mistakes and blunders.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct SideReview {
    pub accuracy: Option<f64>,
    /// Inaccuracies, mistakes and blunders.
    pub counts: [usize; 3],
}

/// Reviews a line from its evaluations: `evals[0]` is the position before the first
/// move (if known) and `evals[i]` the position after move `i`, all White-relative.
/// Returns both sides (White first) and the quality of each move that lost ground,
/// by its index in the line.
pub(super) fn review_line(
    evals: &[Option<i32>],
    first_ply: u32,
) -> ([SideReview; 2], Vec<(usize, Quality)>) {
    let mut losses: [Vec<f64>; 2] = [Vec::new(), Vec::new()];
    let mut moves = [0usize; 2];
    let mut sides = [SideReview::default(); 2];
    let mut marks = Vec::new();
    for (index, pair) in evals.windows(2).enumerate() {
        let white = (first_ply as usize + index).is_multiple_of(2);
        let side = usize::from(!white);
        moves[side] += 1;
        let (Some(before), Some(after)) = (pair[0], pair[1]) else {
            continue;
        };
        let flip = |cp: i32| if white { cp } else { -cp };
        let loss =
            crate::review::win_percent(flip(before)) - crate::review::win_percent(flip(after));
        losses[side].push(loss.max(0.));
        let quality = engine::quality::classify(Score::Cp(flip(before)), Score::Cp(flip(after)));
        let slot = match quality {
            Quality::Inaccuracy => Some(0),
            Quality::Mistake => Some(1),
            Quality::Blunder => Some(2),
            _ => None,
        };
        if let Some(slot) = slot {
            sides[side].counts[slot] += 1;
            marks.push((index, quality));
        }
    }
    for side in 0..2 {
        // Accuracy needs nearly every move of the side to be judged.
        let judged = losses[side].len();
        if judged > 0 && judged * 10 >= moves[side] * 9 {
            let average = losses[side].iter().sum::<f64>() / judged as f64;
            sides[side].accuracy = Some(crate::review::accuracy_from_loss(average));
        }
    }
    (sides, marks)
}

impl AnalyseScreen {
    /// How deep the line's evaluations were searched: "depth 22 per move", or a range.
    fn review_source(depths: &[Option<u32>]) -> Option<String> {
        let min = depths.iter().flatten().min()?;
        let max = depths.iter().flatten().max()?;
        let depth = if min == max {
            min.to_string()
        } else {
            format!("{min}–{max}")
        };
        Some(ui::i18n::t("depth {} per move").replace("{}", &depth))
    }

    /// One side's tile: swatch, accuracy and the counts of each kind of error.
    fn review_tile(
        &self,
        side: chess_core::Color,
        review: SideReview,
        cx: &gpui::App,
    ) -> gpui::Div {
        let theme = self.model.read(cx).config.board.prefs().theme;
        let count = |colour: u32, value: usize, one: &'static str, many: &'static str| {
            h_flex()
                .gap(px(4.))
                .whitespace_nowrap()
                .child(div().size(px(6.)).rounded_full().bg(rgb(colour)))
                .child(mono(value.to_string()).text_as(12., T.fg_2))
                .child(
                    div()
                        .text_as(12., T.muted)
                        .child(ui::i18n::t(if value == 1 { one } else { many })),
                )
        };
        v_flex()
            .flex_1()
            .min_w_0()
            .gap(px(4.))
            .px(px(10.))
            .py(px(8.))
            .rounded(px(4.))
            .bg(rgb(T.surface_2))
            .child(
                h_flex()
                    .gap(px(6.))
                    .child(
                        div()
                            .flex_none()
                            .size(px(10.))
                            .rounded(px(2.))
                            .bg(rgb(ui::board::piece_colour(theme, side)))
                            .border_as(T.border_strong),
                    )
                    .child(
                        div()
                            .text_as(10., T.faint)
                            .child(ui::i18n::t("Accuracy").to_uppercase()),
                    )
                    .child(div().flex_1())
                    .child(
                        mono(
                            review
                                .accuracy
                                .map_or_else(|| "—".to_owned(), |a| format!("{a:.0}%")),
                        )
                        .text_as(14., T.fg)
                        .font_weight(FontWeight::SEMIBOLD),
                    ),
            )
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_x(px(12.))
                    .child(count(
                        T.q_inaccuracy,
                        review.counts[0],
                        "inaccuracy",
                        "inaccuracies",
                    ))
                    .child(count(T.q_mistake, review.counts[1], "mistake", "mistakes"))
                    .child(count(T.q_blunder, review.counts[2], "blunder", "blunders")),
            )
    }

    /// The review card: header with source, graph tabs and the analyse button; a tile
    /// per side; the graph; and a legend.
    pub(super) fn render_review(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let this = cx.entity();
        let tab = self.model.read(cx).config.view.graph;
        let ids = self.timeline_ids();
        let (evals, depths) = self.line_evals(&ids, cx);
        let (root_eval, _) = self.line_evals(&[GameTree::ROOT], cx);
        let first_ply = ids.first().map_or(0, |id| self.tree.node(*id).ply - 1);
        let line: Vec<Option<i32>> = root_eval.into_iter().chain(evals.iter().copied()).collect();
        let (sides, qualities) = review_line(&line, first_ply);
        // Move symbols in the game come first; the review fills in the rest.
        let marks: Vec<(usize, u32)> = ids
            .iter()
            .enumerate()
            .filter_map(|(index, id)| {
                let nag = self
                    .tree
                    .node(*id)
                    .nags
                    .iter()
                    .copied()
                    .find(|nag| *nag <= 6)?;
                Some((index, ui::scoresheet::nag_color(nag)))
            })
            .chain(
                qualities
                    .iter()
                    .map(|(index, quality)| (*index, quality_color(*quality))),
            )
            .fold(
                Vec::new(),
                |mut marks: Vec<(usize, u32)>, (index, colour)| {
                    if !marks.iter().any(|(seen, _)| *seen == index) {
                        marks.push((index, colour));
                    }
                    marks
                },
            );
        let analysed = evals.iter().any(Option::is_some);
        let current = ids.iter().position(|&id| id == self.current);
        let scrub_ids = ids.clone();
        let scrub_this = this.clone();
        let on_scrub: ui::timeline::ScrubHandler = Rc::new(move |index, window, cx| {
            if let Some(&id) = scrub_ids.get(index) {
                scrub_this.update(cx, |screen, cx| {
                    if screen.current != id {
                        screen.jump(id, window, cx);
                    }
                });
            }
        });
        let count = ids.len();
        let x_ticks = ui::graph::move_ticks(count, first_ply, 8);
        let body = match tab {
            GraphTab::Timeline => timeline(TimelineProps {
                evals: evals.clone(),
                current,
                on_scrub,
            })
            .into_any_element(),
            GraphTab::Eval => {
                let theme = self.model.read(cx).config.board.prefs().theme;
                ui::graph::graph(ui::graph::GraphProps {
                    series: vec![ui::graph::Series {
                        values: evals.iter().map(|cp| cp.map(|cp| cp as f32)).collect(),
                        color: T.muted,
                        fill: false,
                        connect: false,
                    }],
                    count,
                    current,
                    scale: ui::graph::Scale::SymLog {
                        max: chess_core::annotation::MATE_CP as f32,
                        unit: 100.0,
                    },
                    ticks: ui::graph::eval_ticks(),
                    height: Some(140.),
                    on_scrub,
                    empty: "No evaluations yet. Analyse the game to see each side's accuracy and mistakes.",
                    name: "Evaluation graph",
                    sides: Some((
                        ui::board::piece_colour(theme, chess_core::Color::White),
                        ui::board::piece_colour(theme, chess_core::Color::Black),
                    )),
                    marks,
                    x_ticks,
                })
                .into_any_element()
            }
            GraphTab::Time => {
                let times = chess_core::move_times(&self.tree, &ids);
                let max = times.iter().flatten().fold(0.0f64, |a, b| a.max(*b)) as f32;
                let side = |white: bool| ui::graph::Series {
                    values: ids
                        .iter()
                        .zip(&times)
                        .map(|(id, time)| {
                            let is_white = self.tree.node(*id).ply % 2 == 1;
                            time.filter(|_| is_white == white).map(|t| t as f32)
                        })
                        .collect(),
                    color: if white { T.fg } else { T.warn },
                    fill: false,
                    connect: true,
                };
                ui::graph::graph(ui::graph::GraphProps {
                    series: vec![side(true), side(false)],
                    count,
                    current,
                    scale: ui::graph::Scale::Linear {
                        max: (max * 1.1).max(1.0),
                    },
                    ticks: ui::graph::linear_ticks(max, "s"),
                    height: None,
                    on_scrub,
                    empty: "No clock data. Games with [%clk] or [%emt] comments show time per move.",
                    name: "Time graph",
                    sides: None,
                    marks: Vec::new(),
                    x_ticks,
                })
                .into_any_element()
            }
            GraphTab::Depth => {
                let max = depths.iter().flatten().copied().max().unwrap_or(0) as f32;
                ui::graph::graph(ui::graph::GraphProps {
                    series: vec![ui::graph::Series {
                        values: depths.iter().map(|d| d.map(|d| d as f32)).collect(),
                        color: T.accent,
                        fill: false,
                        connect: true,
                    }],
                    count,
                    current,
                    scale: ui::graph::Scale::Linear {
                        max: (max * 1.1).max(1.0),
                    },
                    ticks: ui::graph::linear_ticks(max, ""),
                    height: None,
                    on_scrub,
                    empty: "No search depths yet.",
                    name: "Depth graph",
                    sides: None,
                    marks: Vec::new(),
                    x_ticks,
                })
                .into_any_element()
            }
        };
        let tiles = (analysed && tab == GraphTab::Eval).then(|| {
            h_flex()
                .gap(px(8.))
                .mb(px(8.))
                .child(self.review_tile(chess_core::Color::White, sides[0], cx))
                .child(self.review_tile(chess_core::Color::Black, sides[1], cx))
        });
        v_flex()
            .flex_1()
            .child(self.render_review_header(analysed, &depths, cx))
            .children(tiles)
            .child(body)
            .child(self.render_review_legend(tab))
            .into_any_element()
    }

    /// The review header: title, where the evaluations come from, the graph tabs and
    /// the button that analyses the whole game.
    fn render_review_header(
        &self,
        analysed: bool,
        depths: &[Option<u32>],
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let this = cx.entity();
        let tab = self.model.read(cx).config.view.graph;
        let tabs = ui::widgets::tab_group(
            "graph-tab",
            GraphTab::ALL.map(|option| (option, option.label())),
            tab,
            {
                let this = this.clone();
                move |option, _, cx| {
                    this.update(cx, |screen, cx| {
                        screen.set_view(cx, |view| view.graph = option)
                    });
                }
            },
        );
        let running = self.backward.is_some();
        let nodes = self.model.read(cx).config.backward_nodes;
        let label = if running {
            "Stop"
        } else if analysed {
            "Re-analyse game"
        } else {
            "Analyse game"
        };
        h_flex()
            .flex_none()
            .flex_wrap()
            .gap(px(8.))
            .mt(px(14.))
            .mb(px(8.))
            .child(section_label("Game review"))
            .children(
                Self::review_source(depths)
                    .map(|source| div().min_w_0().truncate().text_as(12., T.muted).child(source)),
            )
            .child(div().flex_1())
            .child(
                btn("analyse-backward", label)
                    .compact()
                    .when(!running, |button| button.primary())
                    .tooltip(format!(
                        "{} {}",
                        ui::i18n::t(
                            "Search every position from the last move to the first, nodes per position:"
                        ),
                        engine::format_nodes(nodes)
                    ))
                    .on_click(
                        this.listen(move |screen, window, cx| screen.toggle_backward(window, cx)),
                    ),
            )
            .child(tabs)
    }

    /// Under the graph: how to use it and what its marks mean.
    fn render_review_legend(&self, tab: GraphTab) -> impl IntoElement + use<> {
        let dot = |colour: u32, label: &'static str| {
            h_flex()
                .gap(px(4.))
                .child(div().size(px(6.)).rounded_full().bg(rgb(colour)))
                .child(ui::i18n::t(label))
        };
        h_flex()
            .flex_wrap()
            .gap_x(px(12.))
            .gap_y(px(2.))
            .mt(px(6.))
            .text_as(11., T.muted)
            .child(ui::i18n::t("Click the graph to go to a move"))
            .child(div().flex_1())
            .when(tab == GraphTab::Eval, |row| {
                row.child(dot(T.q_inaccuracy, "Inaccuracy"))
                    .child(dot(T.q_mistake, "Mistake"))
                    .child(dot(T.q_blunder, "Blunder"))
            })
            .child(
                h_flex()
                    .gap(px(4.))
                    .child(div().w(px(2.)).h(px(10.)).bg(rgb(T.accent)))
                    .child(ui::i18n::t("Current move")),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn review_counts_errors_per_side() {
        // White keeps the balance; Black blunders on the second move.
        let evals = [Some(20), Some(25), Some(30), Some(30), Some(400)];
        let (sides, marks) = review_line(&evals, 0);
        assert_eq!(sides[0].counts, [0, 0, 0]);
        assert_eq!(sides[1].counts, [0, 0, 1]);
        assert_eq!(marks, vec![(3, Quality::Blunder)]);
        assert!(sides[0].accuracy.unwrap() > 95.);
        assert!(sides[1].accuracy.unwrap() < sides[0].accuracy.unwrap());
    }

    #[test]
    fn review_needs_most_moves_judged_for_accuracy() {
        let evals = [Some(20), None, None, Some(30), Some(30)];
        let (sides, _) = review_line(&evals, 0);
        assert_eq!(sides[0].accuracy, None);
    }
}
