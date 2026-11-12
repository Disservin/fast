//! The board column: the board with its arrows, the eval bar and the evaluation graph.

use super::*;

impl AnalyseScreen {
    pub(super) fn update_arrows(&mut self, cx: &mut Context<Self>) {
        // A hovered move in Line preview that can be played on the board shows just that
        // move. Later moves are only on the peek board: their arrows would make no sense
        // here. The line can change under a still mouse, so it is read again from the
        // line shown now; a move that is no longer there ends the hover.
        if let Some(index) = self.pv_hover {
            match self.hovered_move(index) {
                Some(mv) if index == self.shown_moves() => {
                    let arrows: Vec<Arrow> = mv
                        .from()
                        .map(|from| Arrow::new(from, mv.to()))
                        .into_iter()
                        .collect();
                    self.board
                        .update(cx, |board, cx| board.set_arrows(arrows, cx));
                    return;
                }
                Some(_) => {}
                None => self.pv_hover = None,
            }
        }
        // A highlighted range is already drawn on the board.
        if self.pv_view.as_ref().is_some_and(PvView::is_range) {
            return;
        }
        // The board shows the previewed position, which only the kibitz analyses.
        if self.pv_kibitz {
            let arrows: Vec<Arrow> = self
                .kibitz
                .read(cx)
                .line
                .as_ref()
                .and_then(|line| {
                    let (_, moves) =
                        panels::replay(&self.shown_pos(), &line.sans[..line.sans.len().min(1)]);
                    let mv = moves.first()?;
                    Some(Arrow::new(mv.from()?, mv.to()))
                })
                .into_iter()
                .collect();
            self.board
                .update(cx, |board, cx| board.set_arrows(arrows, cx));
            return;
        }
        let pos = self.analysed_pos();
        let arrow_for = |san: &str| -> Option<Arrow> {
            let parsed: chess_core::shakmaty::san::San = san.parse().ok()?;
            let mv = parsed.to_move(&pos).ok()?;
            Some(Arrow::new(mv.from()?, mv.to()))
        };
        let quality_arrows = self.model.read(cx).config.view.quality_arrows;
        let relative = |score: Score| {
            if pos.turn().is_white() {
                score
            } else {
                score.negate()
            }
        };
        let best = self
            .rows
            .iter()
            .filter_map(|row| row.score.map(relative))
            .max_by_key(|score| score.sort_key());
        let engine_color = |row: &MoveRow| {
            row.engine_id
                .and_then(|id| self.model.read(cx).engine(id)?.arrow_color)
        };
        let mut arrows: Vec<Arrow> = Vec::new();
        if quality_arrows {
            // Every engine line gets an arrow coloured by quality, worst drawn first
            // so the best end up on top; the selected one is drawn last of all.
            let mut graded: Vec<(&MoveRow, Quality)> = self
                .rows
                .iter()
                .filter(|row| !row.pv.is_empty())
                .filter_map(|row| Some((row, row.quality?)))
                .collect();
            // Several engines on one move share its arrow: the best-ranked row's.
            let mut seen: Vec<&str> = Vec::new();
            graded.retain(|(row, _)| {
                let first = !seen.contains(&row.san.as_str());
                seen.push(&row.san);
                first
            });
            graded.sort_by_key(|(_, quality)| std::cmp::Reverse(*quality));
            let selected_san = self.selected.as_ref().map(|key| key.san.as_str());
            for (row, quality) in graded {
                if selected_san == Some(row.san.as_str()) {
                    continue;
                }
                if let Some(arrow) = arrow_for(&row.san) {
                    let scale = best
                        .zip(row.score.map(relative))
                        .map_or(1.0, |(best, score)| {
                            engine::quality::secondary_arrow_scale(best, score)
                        });
                    let arrow = arrow.with_scale(scale);
                    arrows.push(
                        arrow.with_color(
                            engine_color(row).unwrap_or_else(|| quality_color(quality)),
                        ),
                    );
                }
            }
        }
        if let Some(key) = &self.selected
            && let Some(arrow) = arrow_for(&key.san)
        {
            let quality = self
                .selected_row()
                .and_then(|row| row.quality)
                .filter(|_| quality_arrows);
            let arrow = match quality {
                Some(quality) => arrow.with_color(quality_color(quality)),
                None => arrow,
            };
            arrows.push(match self.selected_row().and_then(engine_color) {
                Some(color) => arrow.with_color(color),
                None => arrow,
            });
        }
        self.board
            .update(cx, |board, cx| board.set_arrows(arrows, cx));
    }

    pub(super) fn render_board_column(
        &mut self,
        board_size: Pixels,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let this = cx.entity();
        let compact = board_size < px(440.);
        // Eight buttons share the row under the board, so they narrow with it.
        let nav_width = if board_size < px(340.) {
            32.
        } else if compact {
            34.
        } else {
            42.
        };
        let shown = self.shown_pos();
        let turn = if shown.turn().is_white() {
            ui::i18n::t("White")
        } else {
            ui::i18n::t("Black")
        };
        let end = self.live.read(cx).end;
        let status = if self.editor.is_some() {
            ui::i18n::t("Edit position").to_owned()
        } else if let Some(end) = end {
            end.describe().to_owned()
        } else {
            format!("{turn} to move")
        };
        let after = (self.current != GameTree::ROOT && self.editor.is_none())
            .then(|| format!("after {}", self.tree.move_label(self.current)));
        let after = self
            .pv_preview_label(self.tree.node(self.current).ply)
            .or(after);
        let eval_text = self.eval_text(end, cx);
        let graph = self.render_graph(cx);
        let flipped = self.board.read(cx).is_flipped();
        let (top_side, bottom_side) = if flipped {
            (chess_core::Color::White, chess_core::Color::Black)
        } else {
            (chess_core::Color::Black, chess_core::Color::White)
        };
        let top_strip = self.render_player_strip(top_side, end, cx);
        let bottom_strip = self.render_player_strip(bottom_side, end, cx);
        // Where the shown position sits in the line, in full moves.
        let move_counter = {
            let ply = self.tree.node(self.current).ply;
            let last = self
                .timeline_ids()
                .last()
                .map_or(ply, |id| self.tree.node(*id).ply)
                .max(ply);
            mono(format!(
                "{} {} / {}",
                ui::i18n::t("Move"),
                ply.div_ceil(2),
                last.div_ceil(2)
            ))
            .flex_none()
            .whitespace_nowrap()
            .text_as(12., T.muted)
        };
        let nav = |id: &'static str,
                   icon: &'static str,
                   tip: &'static str,
                   action: fn(&mut Self, &mut Window, &mut Context<Self>)| {
            icon_btn(id, icon)
                .w(nav_width)
                .tooltip(tip)
                .on_click(this.listen(action))
        };
        let copy_menu = self.render_copy_menu(nav_width, cx);
        let flip_tip = match self.model.read(cx).config.shortcuts.get("flip_board") {
            Some(key) => format!("Flip board ({})", key.to_uppercase()),
            None => ui::i18n::t("Flip board").to_owned(),
        };
        v_flex()
            .flex_none()
            .w(board_size)
            .min_h_full()
            .pb(px(16.))
            .child(
                h_flex()
                    .h(px(39.))
                    .mt(px(12.))
                    .justify_between()
                    .child(
                        h_flex()
                            .gap(px(12.))
                            .items_baseline()
                            .child(
                                div()
                                    .text_size(px(20.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(status),
                            )
                            .children(after.map(|text| div().text_as(14., T.muted).child(text))),
                    )
                    .child(
                        mono(eval_text)
                            .text_size(px(18.))
                            .font_weight(FontWeight::SEMIBOLD),
                    ),
            )
            .child(top_strip)
            .child({
                let (evals, _) = self.line_evals(&[self.current], cx);
                let share = evals[0].map_or(0.5, |cp| {
                    ((engine::quality::winning_chances(Score::Cp(cp)) + 1.) / 2.) as f32
                });
                div()
                    .relative()
                    .size(board_size)
                    .flex_none()
                    .child(
                        div().absolute().left(px(-14.)).top(px(0.)).child(
                            ui::eval_bar::EvalBar::new("analysis-eval", share, board_size)
                                .flipped(self.board.read(cx).is_flipped()),
                        ),
                    )
                    .child(self.board.clone())
            })
            .child(bottom_strip)
            .child(
                h_flex()
                    .mt(px(6.))
                    .justify_between()
                    .child(
                        h_flex()
                            .gap(px(6.))
                            .child(nav(
                                "first",
                                "icons/skip-back.svg",
                                ui::i18n::t("First move"),
                                Self::go_first,
                            ))
                            .child(nav(
                                "prev",
                                "icons/chevron-left.svg",
                                ui::i18n::t("Previous move"),
                                |s, w, c| s.step(-1, w, c),
                            ))
                            .child(nav(
                                "next",
                                "icons/chevron-right.svg",
                                ui::i18n::t("Next move"),
                                |s, w, c| s.step(1, w, c),
                            ))
                            .child(nav(
                                "last",
                                "icons/skip-forward.svg",
                                ui::i18n::t("Last move"),
                                Self::go_last,
                            ))
                            .when(!compact, |row| row.child(move_counter.ml(px(6.)))),
                    )
                    .child(
                        h_flex()
                            .gap(px(6.))
                            .child(
                                icon_btn("flip", "icons/arrow-down-up.svg")
                                    .w(nav_width)
                                    .selected(flipped)
                                    .tooltip(flip_tip)
                                    .on_click(this.listen(move |screen, _, cx| screen.flip(cx))),
                            )
                            .child(
                                icon_btn("edit-position", "icons/pencil.svg")
                                    .w(nav_width)
                                    .selected(self.editor.is_some())
                                    .tooltip(ui::i18n::t("Edit position"))
                                    .on_click(
                                        this.listen(move |screen, _, cx| screen.toggle_editor(cx)),
                                    ),
                            )
                            .child(copy_menu)
                            .child(
                                icon_btn("snapshot", "icons/camera.svg")
                                    .w(nav_width)
                                    .tooltip(ui::i18n::t("Copy the board as an image and save it"))
                                    .on_click(
                                        this.listen(move |screen, _, cx| screen.snapshot_board(cx)),
                                    ),
                            ),
                    ),
            )
            .child(graph)
    }

    /// The evaluation beside the status line: the main engine's score, else the stored
    /// evaluation, or the result once the game is over.
    fn eval_text(&self, end: Option<chess_core::GameEnd>, cx: &mut Context<Self>) -> String {
        let live = self.live.read(cx);
        let eval_text = live
            .primary()
            .and_then(|engine| engine.lines.get(&1))
            .and_then(|line| line.score)
            .map(Score::display)
            .or_else(|| {
                self.evals
                    .get(&self.current)
                    .filter(|_| self.pv_view.is_none())
                    .map(|cp| Score::Cp(*cp).display())
            })
            .filter(|_| self.editor.is_none())
            .unwrap_or_else(|| "—".to_owned());
        // A finished game shows its result, not an evaluation.
        match end.filter(|_| self.editor.is_none()) {
            Some(end) => end.result().replace("1/2", "½"),
            None => eval_text,
        }
    }

    /// One player's name and rating above or below the board, with a dot when they are
    /// to move and the pieces they are up on.
    fn render_player_strip(
        &self,
        side: chess_core::Color,
        end: Option<chess_core::GameEnd>,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let shown = self.shown_pos();
        let to_move = shown.turn();
        let balance = chess_core::material_balance(shown.board());
        let set = self.model.read(cx).config.board.prefs().pieces;
        let strip = move |side: chess_core::Color| {
            (!balance.up(side).is_empty()).then(|| {
                ui::board::material_strip(
                    balance.up(side),
                    side.other(),
                    balance.lead(side),
                    set,
                    17.,
                )
            })
        };
        let material = strip(side).filter(|_| self.editor.is_none());
        let (name, rating) = self.player(side);
        h_flex()
            .h(px(26.))
            .gap(px(8.))
            .justify_between()
            .child(
                h_flex()
                    .gap(px(8.))
                    .min_w_0()
                    .child(
                        // The side to move is marked beside its name.
                        div()
                            .flex_none()
                            .size(px(6.))
                            .rounded_full()
                            .when(to_move == side && end.is_none(), |dot| {
                                dot.bg(rgb(T.accent))
                            }),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_size(px(13.))
                            .font_weight(FontWeight::MEDIUM)
                            .child(name),
                    )
                    .child(mono(rating).flex_none().text_as(12., T.muted)),
            )
            .children(material)
    }

    /// Evals along the shown line: the game's own, else what any engine found for that
    /// position, including by another move order.
    pub(super) fn line_evals(
        &self,
        ids: &[NodeId],
        cx: &gpui::App,
    ) -> (Vec<Option<i32>>, Vec<Option<u32>>) {
        let live = self.live.read(cx);
        let engines = live.engine_ids();
        ids.iter()
            .map(|id| {
                let cached = || {
                    live.cache.eval(
                        &engines,
                        position_hash(&self.tree.node(*id).pos),
                        &crate::analysis_store::tree_position_key(&self.tree, *id),
                    )
                };
                match self.evals.get(id) {
                    Some(cp) => (Some(*cp), self.depths.get(id).copied().filter(|d| *d > 0)),
                    None => match cached() {
                        Some((cp, depth)) => (Some(cp), Some(depth)),
                        None => (None, None),
                    },
                }
            })
            .unzip()
    }

    /// The graph under the board, with tabs for eval, time, depth and the bar timeline.
    pub(super) fn render_graph(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let this = cx.entity();
        let tab = self.model.read(cx).config.view.graph;
        let ids = self.timeline_ids();
        let (evals, depths) = self.line_evals(&ids, cx);
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
        let mut tabs = h_flex().gap(px(2.));
        for (index, option) in GraphTab::ALL.into_iter().enumerate() {
            let active = option == tab;
            tabs = tabs.child(
                div()
                    .id(("graph-tab", index))
                    .px(px(8.))
                    .py(px(2.))
                    .rounded(px(4.))
                    .cursor_pointer()
                    .text_size(px(11.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(if active { T.fg } else { T.muted }))
                    .when(active, |el| el.bg(rgb(T.selected)))
                    .when(!active, |el| el.hover(|style| style.bg(rgb(T.hover))))
                    .on_click(this.listen_event(move |screen, _, _, cx| {
                        screen.set_view(cx, |view| view.graph = option);
                    }))
                    .child(option.label()),
            );
        }
        let count = ids.len();
        let body = match tab {
            GraphTab::Timeline => timeline(TimelineProps {
                evals,
                current,
                on_scrub,
            })
            .into_any_element(),
            GraphTab::Eval => ui::graph::graph(ui::graph::GraphProps {
                series: vec![ui::graph::Series {
                    values: evals.iter().map(|cp| cp.map(|cp| cp as f32)).collect(),
                    color: T.fg,
                    fill: true,
                    connect: false,
                }],
                count,
                current,
                scale: ui::graph::Scale::SymLog {
                    max: chess_core::annotation::MATE_CP as f32,
                    unit: 100.0,
                },
                ticks: ui::graph::eval_ticks(),
                height: None,
                on_scrub,
                empty: "No evaluations yet. Turn the engine on or load an annotated game.",
            })
            .into_any_element(),
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
                })
                .into_any_element()
            }
        };
        v_flex()
            .flex_1()
            .child(
                h_flex()
                    .flex_none()
                    .flex_wrap()
                    .gap(px(6.))
                    .mt(px(14.))
                    .mb(px(6.))
                    .justify_between()
                    .child(
                        h_flex()
                            .gap(px(8.))
                            .child(section_label("Game graph"))
                            .child({
                                let running = self.backward.is_some();
                                let nodes = self.model.read(cx).config.backward_nodes;
                                let label = if running {
                                    ui::i18n::t("Stop")
                                } else {
                                    ui::i18n::t("Analyse backwards")
                                };
                                btn("analyse-backward", label)
                                .mini()
                                .selected(running)
                                .tooltip(format!(
                                    "Search every position from the last move to the first, {} each (Settings › Analysis)",
                                    engine::format_nodes(nodes)
                                ))
                                .on_click(this.listen(move |screen, window, cx| {
                                    screen.toggle_backward(window, cx)
                                }))
                            }),
                    )
                    .child(tabs),
            )
            .child(body)
            .into_any_element()
    }
}
