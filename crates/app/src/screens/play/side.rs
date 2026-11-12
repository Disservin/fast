//! The panels beside the board: clocks, the evaluation graph, engine analysis and the engine log.

use super::*;

/// Formats a clock as mm:ss, or h:mm:ss for long games.
pub fn format_clock(ms: u64) -> String {
    let seconds = ms.div_ceil(1000);
    let (hours, minutes, secs) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{secs:02}")
    } else {
        format!("{minutes:02}:{secs:02}")
    }
}

/// The Move, Eval and Time headings over both columns of an engine match's move list.
fn move_columns_header() -> gpui::Div {
    h_flex()
        .h(px(28.))
        .flex_none()
        .bg(rgb(T.surface_2))
        .mt(px(8.))
        .mr(px(16.))
        .pl(px(48.))
        .gap(px(12.))
        .children((0..2).map(|_| {
            h_flex()
                .flex_1()
                .min_w_0()
                .gap(px(6.))
                .child(section_label("Move").flex_1().min_w_0())
                .child(section_label("Eval").w(px(52.)).text_right())
                .child(section_label("Time").w(px(56.)).text_right())
        }))
}

impl PlayScreen {
    pub(super) fn clock_bar(&self, opponent: bool, cx: &gpui::App) -> impl IntoElement + use<> {
        let game = self.game.as_ref();
        let turn = game.map(|game| game.tree.node(game.current).pos.turn());
        let finished = game.is_some_and(|game| game.result.is_some());
        let side = game.map(|game| {
            if opponent {
                game.user.other()
            } else {
                game.user
            }
        });
        let active = !finished && game.is_some() && turn == side;
        let ms = game.map_or(0, |game| match side {
            Some(Color::White) => game.white_ms,
            _ => game.black_ms,
        });
        let name: SharedString = match (opponent, game) {
            (true, Some(game)) => game.opponent_name.clone().into(),
            (true, None) => self.opponents.first().map_or_else(
                || ui::i18n::t("Opponent").into(),
                |engine| engine.name.clone().into(),
            ),
            (false, Some(game)) if game.self_play() => game
                .white
                .as_ref()
                .map_or_else(SharedString::default, |white| white.name.clone().into()),
            (false, _) => ui::i18n::t("You").into(),
        };
        let self_play = game.is_some_and(|game| game.self_play());
        let rating = if opponent {
            game.and_then(|game| game.opponent_rating)
                .map(|rating| rating.to_string())
        } else if self_play {
            game.and_then(|game| game.white.as_ref()?.rating)
                .map(|rating| rating.to_string())
        } else {
            self.model
                .read(cx)
                .config
                .player_rating
                .map(|rating| rating.to_string())
        };
        let status = (!opponent && active && !self_play).then_some(ui::i18n::t("your move"));
        // The pieces this side is up on, in the opponent's colour.
        let material = game.zip(side).map(|(game, side)| {
            let balance = chess_core::material_balance(game.tree.node(game.current).pos.board());
            let set = self.model.read(cx).config.board.prefs().pieces;
            ui::board::material_strip(balance.up(side), side.other(), balance.lead(side), set, 18.)
        });
        h_flex()
            .h(px(52.))
            .flex_none()
            .w_full()
            .px(px(16.))
            .justify_between()
            .bg(rgb(T.surface))
            .border_1()
            .border_color(rgb(if active { T.fg } else { T.border }))
            .rounded(px(6.))
            .child(
                h_flex()
                    .gap(px(10.))
                    .child(
                        div()
                            .size(px(10.))
                            .rounded_full()
                            .border_1()
                            .border_color(rgb(if active { T.accent } else { T.muted }))
                            .when(active, |dot| dot.bg(rgb(T.accent))),
                    )
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_size(px(14.))
                            .child(name),
                    )
                    .children(rating.map(|rating| mono(rating).text_as(13., T.muted)))
                    .children(status.map(|text| div().text_as(13., T.muted).child(text)))
                    .children(material),
            )
            .child(
                div()
                    .font_family(MONO)
                    .text_size(px(24.))
                    .font_weight(FontWeight::MEDIUM)
                    .px(px(9.))
                    .rounded(px(4.))
                    .when(active, |time| {
                        time.bg(rgb(T.accent)).text_color(rgb(T.accent_fg))
                    })
                    .when(!active, |time| time.text_color(rgb(T.muted)))
                    .child(if game.is_some() {
                        if game.is_some_and(|game| {
                            game.fixed_limits[usize::from(side == Some(Color::Black))].is_some()
                        }) {
                            "∞".to_owned()
                        } else {
                            format_clock(ms)
                        }
                    } else {
                        "--:--".to_owned()
                    }),
            )
    }

    pub(super) fn render_center(
        &mut self,
        board_size: Pixels,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let top = self.clock_bar(true, cx);
        let bottom = self.clock_bar(false, cx);
        v_flex()
            .flex_none()
            .w(board_size)
            .pt(px(24.))
            .child(top)
            .child(
                div()
                    .size(board_size)
                    .mt(px(12.))
                    .mb(px(12.))
                    .flex_none()
                    .child(self.board.clone()),
            )
            .child(bottom)
    }

    /// Compares both engines' evaluations and search effort throughout the game.
    pub(super) fn render_graph(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let mut header = h_flex().h(px(44.)).border_b_as(T.divider);
        for (index, label) in ["Eval", "Nodes", "Depth", "Time"].into_iter().enumerate() {
            let this = cx.entity();
            header = header.child(header_tab(
                ("match-graph-tab", index),
                label,
                self.graph_metric == index,
                this.listen(move |screen, _, cx| {
                    screen.graph_metric = index;
                    cx.notify();
                }),
            ));
        }
        let column = panel().p(px(16.)).gap(px(8.)).child(header);
        let Some(game) = &self.game else {
            return column;
        };
        let ids = game.tree.path_to(game.current);
        let mut maximum = 1.0_f32;
        let series = [T.engine_a, T.engine_b]
            .into_iter()
            .enumerate()
            .map(|(side, color)| {
                let values = ids
                    .iter()
                    .map(|&id| {
                        let node = game.tree.node(id);
                        if usize::from(node.ply % 2 == 0) != side {
                            return None;
                        }
                        let note = chess_core::annotation::parse_annotation(&node.comment);
                        let value = if self.graph_metric == 0 {
                            note.eval_cp.map(|cp| cp as f32)
                        } else {
                            match self.graph_metric {
                                1 => node
                                    .comment
                                    .split("[%nodes ")
                                    .nth(1)
                                    .and_then(|tail| tail.split(']').next())
                                    .and_then(|value| value.parse::<f32>().ok()),
                                2 => note.depth.map(|depth| depth as f32),
                                _ => note.elapsed.map(|seconds| seconds as f32),
                            }
                        };
                        if let Some(value) = value {
                            maximum = maximum.max(value);
                        }
                        value
                    })
                    .collect();
                ui::graph::Series {
                    values,
                    color,
                    fill: false,
                    connect: true,
                }
            })
            .collect();
        let legend = h_flex().gap(px(14.)).children(
            [
                (
                    game.white.as_ref().map_or("—", |white| white.name.as_str()),
                    T.engine_a,
                ),
                (game.opponent_name.as_str(), T.engine_b),
            ]
            .into_iter()
            .map(|(name, color)| {
                div()
                    .min_w_0()
                    .text_size(px(12.))
                    .text_color(rgb(color))
                    .child(name.to_owned())
            }),
        );
        let current = ids
            .iter()
            .position(|id| *id == self.viewed.unwrap_or(game.current));
        let count = ids.len();
        let this = cx.entity();
        let (scale, ticks) = match self.graph_metric {
            0 => (
                ui::graph::Scale::SymLog {
                    max: chess_core::annotation::MATE_CP as f32,
                    unit: 100.0,
                },
                ui::graph::eval_ticks()
                    .into_iter()
                    .filter(|(cp, _)| cp.abs() != 100.0)
                    .collect(),
            ),
            1 => ui::graph::nodes_axis(maximum),
            _ => (
                ui::graph::Scale::Linear { max: maximum },
                ui::graph::linear_ticks(maximum, if self.graph_metric == 3 { "s" } else { "" }),
            ),
        };
        column
            .child(legend)
            .child(ui::graph::graph(ui::graph::GraphProps {
                series,
                count,
                current,
                scale,
                ticks,
                height: Some(130.0),
                on_scrub: std::rc::Rc::new(move |index, _, cx| {
                    if let Some(&id) = ids.get(index) {
                        this.update(cx, |screen, cx| {
                            screen.viewed = screen
                                .game
                                .as_ref()
                                .and_then(|game| (id != game.current).then_some(id));
                            screen.sync_board(false, cx);
                            cx.notify();
                        });
                    }
                }),
                empty: ui::i18n::t(
                    "No search data yet. Scores and search effort appear after each move.",
                ),
            }))
    }

    /// Shows both engines' latest evaluations and principal variations beside the board.
    pub(super) fn render_analysis(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let this = cx.entity();
        let mut column = panel().p(px(16.)).child(
            h_flex()
                .justify_between()
                .child(section_label("Engines"))
                .child(
                    btn("match-log-toggle", "Log")
                        .mini()
                        .tooltip(ui::i18n::t("Show or hide engine output"))
                        .selected(self.log_open)
                        .on_click(this.listen(move |screen, _, cx| {
                            screen.log_open = !screen.log_open;
                            screen.log_follow = true;
                            cx.notify();
                        })),
                ),
        );
        if let Some(game) = &self.game {
            let moving = usize::from(game.tree.node(game.current).pos.turn() == Color::Black);
            for (index, name) in [
                game.white.as_ref().map_or("—", |white| white.name.as_str()),
                game.opponent_name.as_str(),
            ]
            .into_iter()
            .enumerate()
            {
                let line = &game.analysis[index];
                column = column.child(
                    v_flex()
                        .py(px(10.))
                        .gap(px(6.))
                        .border_b_as(T.divider)
                        .child(
                            h_flex()
                                .gap(px(8.))
                                .child(section_label(if index == 0 { "White" } else { "Black" }))
                                .child(
                                    div()
                                        .text_size(px(13.))
                                        .text_color(rgb(if index == 0 {
                                            T.engine_a
                                        } else {
                                            T.engine_b
                                        }))
                                        .child(name.to_owned()),
                                ),
                        )
                        .child(Self::render_search_stats(
                            line,
                            // The side to move shows its time so far on this move.
                            if game.result.is_none() && index == moving {
                                Some(game.elapsed_ms)
                            } else {
                                line.think_ms
                            },
                        ))
                        .child({
                            let mut flow = div()
                                .flex()
                                .flex_wrap()
                                .gap(px(4.))
                                .font_family(MONO)
                                .text_size(px(13.));
                            if line.pv.is_empty() {
                                flow = flow.child("—");
                            }
                            for (move_index, mv) in line.pv.iter().enumerate() {
                                let position = mv.position.clone();
                                let last = mv.last;
                                flow = flow.child(
                                    div()
                                        .id((
                                            if index == 0 { "white-pv" } else { "black-pv" },
                                            move_index,
                                        ))
                                        .px(px(3.))
                                        .rounded(px(3.))
                                        .text_color(rgb(T.fg_2))
                                        .hover(|item| item.bg(rgb(T.hover_strong)))
                                        .child(mv.label.clone())
                                        .on_hover(cx.listener(
                                            move |screen, hovering: &bool, window, cx| {
                                                if *hovering {
                                                    let mut prefs =
                                                        screen.model.read(cx).config.board.prefs();
                                                    prefs.animation_ms = 0;
                                                    screen.pv_board.update(cx, |board, cx| {
                                                        board.set_prefs(prefs, cx);
                                                        board.set_position(
                                                            position.clone(),
                                                            last,
                                                            false,
                                                            cx,
                                                        );
                                                    });
                                                    screen.pv_hover = Some((index, move_index));
                                                    screen.pv_at = Some(window.mouse_position());
                                                } else if screen.pv_hover
                                                    == Some((index, move_index))
                                                {
                                                    screen.pv_hover = None;
                                                    screen.pv_at = None;
                                                }
                                                cx.notify();
                                            },
                                        )),
                                );
                            }
                            div()
                                .id(if index == 0 {
                                    "white-pv-scroll"
                                } else {
                                    "black-pv-scroll"
                                })
                                .max_h(px(72.))
                                .overflow_y_scroll()
                                .child(flow)
                        }),
                );
            }
        }
        column
    }

    /// One engine's score and search effort in fixed columns, so values stay in place
    /// as they update: depth, nodes, hash use and thinking time.
    pub(super) fn render_search_stats(line: &GameAnalysis, time_ms: Option<u64>) -> gpui::Div {
        let dash = || "—".to_owned();
        let cell = |caption, value: String, width| {
            ui::widgets::figure(caption, value).col_w(width).gap(px(1.))
        };
        let depth = if line.depth == 0 {
            dash()
        } else {
            match line.seldepth {
                Some(sel) => format!("{}/{sel}", line.depth),
                None => line.depth.to_string(),
            }
        };
        h_flex()
            .gap(px(8.))
            .child(
                mono(line.score.map_or_else(dash, |score| score.display()))
                    .col_w(64.)
                    .whitespace_nowrap()
                    .font_weight(FontWeight::SEMIBOLD),
            )
            .child(cell("Depth", depth, 56.))
            .child(cell(
                "Nodes",
                line.nodes.map_or_else(dash, engine::format_nodes),
                72.,
            ))
            .child(cell(
                "Hash",
                line.hashfull
                    .map_or_else(dash, |full| format!("{:.0}%", f64::from(full) / 10.)),
                44.,
            ))
            .child(cell(
                "Time",
                time_ms.map_or_else(dash, crate::screens::analyse::search_time),
                64.,
            ))
    }

    /// The log the Log panel shows, if that side's engine was started.
    pub(super) fn shown_log(&self) -> Option<&EngineLog> {
        self.game.as_ref()?.logs[self.log_side].as_ref()
    }

    pub(super) fn current_log_key(&self) -> Option<(usize, u64)> {
        self.shown_log().map(|log| (self.log_side, log.revision()))
    }

    /// Takes a new snapshot only when the log or the side changed.
    pub(super) fn refresh_log(&mut self) {
        let key = self.current_log_key();
        if key.is_some() && key == self.log_key {
            return;
        }
        let same_view = key.is_some_and(|key| self.log_key.is_some_and(|old| old.0 == key.0));
        self.log_key = key;
        let lines = self
            .shown_log()
            .map(|log| log.snapshot(true))
            .unwrap_or_default();
        // Reading back through the log: new output must not move the text in view.
        if !self.log_follow && same_view {
            crate::debug_window::keep_log_anchor(&self.log_scroll, &self.log_lines, &lines);
        }
        self.log_lines = lines;
    }

    /// The UCI traffic of either engine in the match.
    pub(super) fn render_log(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        self.refresh_log();
        let count = self.log_lines.len();
        if self.log_follow && count > 0 {
            self.log_scroll
                .scroll_to_item(count - 1, ScrollStrategy::Bottom);
        }
        let mut header = h_flex()
            .h(px(44.))
            .px(px(16.))
            .gap(px(10.))
            .border_b_as(T.border)
            .child(section_label("Log"));
        for (side, label) in ["White", "Black"].into_iter().enumerate() {
            let this = cx.entity();
            header = header.child(header_tab(
                ("match-log-side", side),
                label,
                self.log_side == side,
                this.listen(move |screen, _, cx| {
                    screen.log_side = side;
                    screen.log_follow = true;
                    cx.notify();
                }),
            ));
        }
        let (follow_this, window_this) = (cx.entity(), cx.entity());
        header = header
            .child(div().flex_1())
            .child(
                div().flex_none().child(
                    check("match-log-follow", self.log_follow, "Follow")
                        .text(13.)
                        .on_change(follow_this.listen_with(move |screen, value, _, cx| {
                            screen.log_follow = value;
                            cx.notify();
                        })),
                ),
            )
            .child(
                btn("match-log-window", "Window")
                    .compact()
                    .disabled(self.shown_log().is_none())
                    .tooltip(ui::i18n::t(
                        "Follow this engine's raw output in its own window",
                    ))
                    .on_click(move |_, cx| {
                        let screen = window_this.read(cx);
                        let Some(game) = &screen.game else { return };
                        let side = screen.log_side;
                        let name = if side == 0 {
                            game.white
                                .as_ref()
                                .map_or_else(String::new, |white| white.name.clone())
                        } else {
                            game.opponent_name.clone()
                        };
                        if let Some(log) = game.logs[side].clone() {
                            crate::debug_window::DebugWindow::open(&name, log, cx);
                        }
                    }),
            );
        let longest = self
            .log_lines
            .iter()
            .enumerate()
            .max_by_key(|(_, line)| line.text.len())
            .map(|(index, _)| index);
        let body = if count == 0 {
            ui::widgets::note("Nothing logged yet.").into_any_element()
        } else {
            uniform_list(
                "match-log",
                count,
                cx.processor(|this, range: std::ops::Range<usize>, _, _| {
                    range
                        .map(|index| {
                            let Some(line) = this.log_lines.get(index) else {
                                return div().into_any_element();
                            };
                            crate::debug_window::log_row(line.kind)
                                .child(SharedString::from(line.text.clone()))
                                .into_any_element()
                        })
                        .collect::<Vec<_>>()
                }),
            )
            .track_scroll(&self.log_scroll)
            .with_horizontal_sizing_behavior(ListHorizontalSizingBehavior::Unconstrained)
            .with_width_from_item(longest)
            .h(px(crate::debug_window::LOG_ROW_H * 12.))
            .on_scroll(cx.listener(|this, _, _, cx| {
                this.log_follow = false;
                cx.notify();
            }))
            .into_any_element()
        };
        panel().flex_none().overflow_hidden().child(header).child(
            div()
                .relative()
                .py(px(6.))
                .bg(rgb(T.surface_2))
                .child(body)
                .vertical_scrollbar(&self.log_scroll)
                .horizontal_scrollbar(&self.log_scroll),
        )
    }

    /// The moves of the game, and with `actions` the buttons to stop, resign or analyse it.
    pub(super) fn render_side(
        &mut self,
        actions: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let this = cx.entity();
        let game = self.game.as_ref();
        let mut list = v_flex()
            .w_full()
            .min_w_0()
            .font_family(MONO)
            .text_size(px(13.));
        let mut columns = None;
        let mut opening_text = String::from("Starting position");
        if let Some(game) = game {
            let ids = game.tree.path_to(game.current);
            let first_ply = ids.first().map_or(1, |&id| game.tree.node(id).ply);
            if let Some(opening) = classify(ids.iter().map(|&id| game.tree.node(id).san.as_str())) {
                opening_text = format!("{} {}", opening.eco, opening.name);
            }
            let black_first = first_ply % 2 == 0;
            let mut cells: Vec<Option<NodeId>> = Vec::new();
            if black_first {
                cells.push(None);
            }
            cells.extend(ids.iter().map(|&id| Some(id)));
            if game.self_play() {
                columns = Some(move_columns_header());
            }
            for (index, pair) in cells.chunks(2).enumerate() {
                let number = first_ply.div_ceil(2) as usize + index;
                list = list.child(
                    h_flex()
                        .w_full()
                        .h(px(28.))
                        .flex_none()
                        .gap(px(12.))
                        .border_b_as(T.divider)
                        .child(
                            div()
                                .col_w(36.)
                                .text_color(rgb(T.faint))
                                .child(format!("{number}.")),
                        )
                        .child(self.move_cell(game, pair[0]))
                        .child(self.move_cell(game, pair.get(1).copied().flatten())),
                );
            }
        }
        let result = game.and_then(|game| game.result.clone());
        let in_game = game.is_some_and(|game| game.result.is_none());
        let self_play = game.is_some_and(|game| game.self_play());
        v_flex()
            .gap(px(16.))
            .child(
                panel()
                    .p(px(16.))
                    .child(
                        h_flex().gap(px(8.)).child(section_label("Moves")).when(
                            self.game.as_ref().is_some_and(|game| {
                                game.tree.variant == chess_core::Variant::Chess960
                            }),
                            |row| row.child(ui::widgets::badge("Chess960")),
                        ),
                    )
                    .children(columns)
                    .child(
                        div()
                            .relative()
                            .min_w_0()
                            .child(
                                div()
                                    .id("play-moves-scroll")
                                    .w_full()
                                    .max_h(px(220.))
                                    .overflow_y_scroll()
                                    .pr(px(16.))
                                    .track_scroll(&self.moves_scroll)
                                    .child(list),
                            )
                            .child(
                                Scrollbar::vertical(&self.moves_scroll).mode(ScrollbarMode::Always),
                            ),
                    )
                    .child(div().mt(px(10.)).text_as(13., T.muted).child(opening_text))
                    .children(result.map(|text| {
                        div()
                            .mt(px(8.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(text)
                    })),
            )
            .when(actions, |column| {
                column
                    .child(if self_play {
                        h_flex().child(
                            btn("stop-self-play", "Stop")
                                .flex_1()
                                .disabled(!in_game)
                                .on_click(this.listen(move |screen, _, cx| screen.resign(cx))),
                        )
                    } else {
                        h_flex()
                            .gap(px(6.))
                            .child(
                                btn("offer-draw", ui::i18n::t("Offer draw"))
                                    .flex_1()
                                    .disabled(!in_game)
                                    .on_click(
                                        this.listen(move |screen, _, cx| screen.offer_draw(cx)),
                                    ),
                            )
                            .child(
                                btn("takeback", "Takeback")
                                    .flex_1()
                                    .disabled(!in_game)
                                    .on_click(
                                        this.listen(move |screen, _, cx| screen.takeback(cx)),
                                    ),
                            )
                            .child(
                                btn("resign", ui::i18n::t("Resign"))
                                    .flex_1()
                                    .disabled(!in_game)
                                    .on_click(this.listen(move |screen, _, cx| screen.resign(cx))),
                            )
                    })
                    .child(
                        btn("analyse-game", "Analyse this game")
                            .full()
                            .h(40.)
                            .disabled(game.is_none())
                            .on_click(
                                this.listen(move |screen, window, cx| {
                                    screen.analyse_game(window, cx)
                                }),
                            ),
                    )
            })
    }

    /// One move in the move list: the SAN, highlighted when it is the move on the board,
    /// and in engine matches the evaluation and time recorded with it.
    fn move_cell(&self, game: &Game, id: Option<NodeId>) -> gpui::Div {
        let current = id == Some(self.viewed.unwrap_or(game.current));
        let san = id
            .map(|id| game.tree.node(id).san.clone())
            .unwrap_or_default();
        let values = id.map(|id| match_move_values(&game.tree.node(id).comment));
        h_flex()
            .flex_1()
            .min_w_0()
            .gap(px(6.))
            .child(
                h_flex().flex_1().min_w_0().child(
                    div()
                        .flex_none()
                        .px(px(4.))
                        .whitespace_nowrap()
                        .when(current, |pill| {
                            pill.rounded(px(4.))
                                .bg(rgb(T.accent))
                                .text_color(rgb(T.accent_fg))
                        })
                        .child(san),
                ),
            )
            .when(game.self_play() && id.is_some(), |row| {
                let (eval, time) = values.unwrap_or_else(|| ("—".to_owned(), "—".to_owned()));
                row.child(
                    mono(eval)
                        .col_w(52.)
                        .text_size(px(12.))
                        .text_right()
                        .whitespace_nowrap()
                        .text_color(rgb(T.fg_2)),
                )
                .child(
                    mono(time)
                        .col_w(56.)
                        .text_size(px(12.))
                        .text_right()
                        .whitespace_nowrap()
                        .text_color(rgb(T.muted)),
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clocks_format_and_round_up() {
        assert_eq!(format_clock(552_000), "09:12");
        assert_eq!(format_clock(551_001), "09:12");
        assert_eq!(format_clock(0), "00:00");
        assert_eq!(format_clock(3_661_000), "1:01:01");
    }
}
