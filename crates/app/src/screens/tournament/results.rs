//! A tournament's page: standings, live games, finished games and the summary of its settings.

use super::*;

use self::summary::locked_summary;

mod games;
mod summary;

/// The results block as tiles, W/D/L and the pentanomial. Scores are from engine A's side.
pub(super) fn results_body(n: &fastchess::Results, a: &str, b: &str) -> Div {
    let games = n.games.max(1) as f64;
    // Equal columns, so every tile has the same width and, row by row, the same height:
    // three across, or two by two once the LLR tile joins them.
    let mut tiles = div()
        .grid()
        .grid_cols(if n.llr.is_some() { 2 } else { 3 })
        .gap(px(12.));
    tiles = tiles.child(
        tile("Elo")
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_x(px(8.))
                    .items_end()
                    .child(big(format!("{:+.2}", n.elo)))
                    .child(small(format!("± {:.2}", n.elo_err))),
            )
            .child(small(format!("nElo {:+.2} ± {:.2}", n.nelo, n.nelo_err))),
    );
    tiles = tiles.child(
        tile("LOS")
            .child(big(format!("{:.1} %", n.los)))
            .child(meter(n.los as f32 / 100., T.engine_a)),
    );
    tiles = tiles.child(
        tile("Score")
            .child(big(format!("{:.1} / {}", n.points, n.games)))
            .child(
                h_flex()
                    .gap(px(12.))
                    .child(small(format!("{:.1}%", n.score_pct)))
                    .child(small(format!(
                        "draws {:.0}%",
                        n.draws as f64 / games * 100.
                    ))),
            ),
    );
    if let Some(llr) = n.llr {
        let span = (llr.upper - llr.lower).max(f64::EPSILON);
        tiles = tiles.child(
            tile("LLR")
                .child(big(format!("{:+.2}", llr.value)))
                .child(small(format!(
                    "bounds {:.2} to {:.2}",
                    llr.lower, llr.upper
                )))
                .child(meter(((llr.value - llr.lower) / span) as f32, T.engine_a)),
        );
    }

    let total = (n.wins + n.draws + n.losses).max(1) as f32;
    let wdl = v_flex()
        .gap(px(6.))
        .child(
            h_flex()
                .justify_between()
                .text_size(px(12.))
                .child(
                    h_flex()
                        .gap(px(6.))
                        .items_center()
                        .child(swatch(T.engine_a))
                        .child(SharedString::from(format!("{a} wins {}", n.wins))),
                )
                .child(small(format!("Draws {}", n.draws)))
                .child(
                    h_flex()
                        .gap(px(6.))
                        .items_center()
                        .child(SharedString::from(format!("{b} wins {}", n.losses)))
                        .child(swatch(T.engine_b)),
                ),
        )
        .child(
            h_flex()
                .h(px(14.))
                .w_full()
                .rounded(px(7.))
                .overflow_hidden()
                .child(
                    div()
                        .h_full()
                        .w(relative(n.wins as f32 / total))
                        .bg(rgb(T.engine_a)),
                )
                .child(
                    div()
                        .h_full()
                        .w(relative(n.draws as f32 / total))
                        .bg(rgb(T.border_strong)),
                )
                .child(
                    div()
                        .h_full()
                        .w(relative(n.losses as f32 / total))
                        .bg(rgb(T.engine_b)),
                ),
        );

    // Losing pairs are orange, the middle grey, winning pairs blue.
    let max = n.ptnml.iter().copied().max().unwrap_or(0).max(1) as f32;
    let bins = ["LL", "LD", "DD/WL", "WD", "WW"];
    let colours = [
        T.engine_b,
        T.engine_b,
        T.border_strong,
        T.engine_a,
        T.engine_a,
    ];
    let chart = v_flex()
        .flex_1()
        .min_w(px(240.))
        .gap(px(6.))
        .child(heading("Pentanomial"))
        .child(
            h_flex()
                .items_end()
                .gap(px(10.))
                .h(px(110.))
                .children((0..5).map(|i| {
                    v_flex()
                        .flex_1()
                        .h_full()
                        .justify_end()
                        .items_center()
                        .gap(px(4.))
                        .child(small(n.ptnml[i].to_string()))
                        .child(
                            div()
                                .w_full()
                                .h(px(72. * n.ptnml[i] as f32 / max))
                                .rounded(px(4.))
                                .bg(rgb(colours[i])),
                        )
                        .child(small(bins[i]))
                })),
        );
    let ratios = v_flex()
        .w(px(170.))
        .gap(px(10.))
        .child(ratio("Pairs ratio", format!("{:.2}", n.pairs_ratio)))
        .child(ratio("WL/DD ratio", format!("{:.2}", n.wl_dd_ratio)))
        .child(ratio("Draw ratio", format!("{:.1} %", n.draw_ratio)));

    v_flex().gap(px(16.)).child(tiles).child(wdl).child(
        h_flex()
            .items_end()
            .flex_wrap()
            .gap(px(20.))
            .child(chart)
            .child(ratios),
    )
}

/// Time left at the average pace of finished games: average game time × games left ÷
/// concurrency.
pub(super) fn eta(run: &Run) -> String {
    let (time, count) = run.game_time;
    if count == 0 || run.concurrency == 0 {
        return "–".into();
    }
    let remaining = run.total.max(run.played()).saturating_sub(run.played());
    let average = time.as_secs_f64() / f64::from(count);
    crate::format::format_duration(
        (average * remaining as f64 / run.concurrency as f64).round() as u64,
    )
}

impl TournamentScreen {
    pub(super) fn render_run(&self, run: &Run, cx: &mut Context<Self>) -> AnyElement {
        let this = cx.entity();
        let live = self.live_games.len() as u64;
        let total = run.total.max(run.played());
        let fraction = if total == 0 {
            0.
        } else {
            run.played() as f32 / total as f32
        };
        let queued = total.saturating_sub(run.played() + live);
        let (label, fg, bg, note) = match &run.status {
            Status::Running => ("Running", T.live_fg, T.live_bg, None),
            Status::Stopping => (
                "Stopping",
                T.warn,
                T.tour_tile,
                Some((
                    "Stopping: fastchess is saving its config. Stop again to kill it.".to_owned(),
                    T.warn,
                )),
            ),
            Status::Finished(text) => {
                let label = if text.starts_with("Stopped") {
                    "Stopped"
                } else {
                    "Finished"
                };
                (label, T.muted, T.tour_tile, Some((text.clone(), T.muted)))
            }
            Status::Failed(text) => (
                "Failed",
                T.danger_fg,
                T.danger_bg,
                Some((text.clone(), T.danger_fg)),
            ),
        };
        let pill = h_flex()
            .gap(px(6.))
            .items_center()
            .px(px(10.))
            .py(px(3.))
            .rounded(px(999.))
            .bg(rgb(bg))
            .child(div().size(px(7.)).rounded_full().bg(rgb(fg)))
            .child(div().text_size(px(12.)).text_color(rgb(fg)).child(label));
        let (a, b) = (
            run.names.first().map(String::as_str).unwrap_or("Engine A"),
            run.names.get(1).map(String::as_str).unwrap_or("Engine B"),
        );
        let title = h_flex()
            .gap(px(12.))
            .items_center()
            .flex_wrap()
            .child(name_chip(a, engine_colour(&run.names, a), 22.))
            .child(
                div()
                    .text_size(px(22.))
                    .text_color(rgb(T.muted))
                    .child(ui::i18n::t("vs")),
            )
            .child(name_chip(b, engine_colour(&run.names, b), 22.));

        let open_last = run.games.front().map(|game| (game.game, game.pgn_index));
        let can_resume = !run.active() && run.config.is_file();
        let folder = run.dir.clone();
        let (dir, cwd, config) = (run.dir.clone(), run.cwd.clone(), run.config.clone());
        let more = this.clone();
        let menu =
            self.menu_open.then(|| {
                let (copy, copy_config) = (this.clone(), this.clone());
                // Deferred so it paints above the cards that follow the header, and occluding
                // so they neither show through nor take its clicks.
                deferred(
                    v_flex()
                        .id("tournament-more-menu")
                        .absolute()
                        .top(px(40.))
                        .right(px(0.))
                        .w(px(190.))
                        .p(px(6.))
                        .gap(px(4.))
                        .rounded(px(10.))
                        .bg(rgb(T.surface))
                        .border_1()
                        .border_color(rgb(T.border_strong))
                        .shadow_lg()
                        .occlude()
                        .child(btn("tournament-copy", "Copy command").full().on_click(
                            move |_, cx| {
                                copy.update(cx, |screen, cx| {
                                    screen.menu_open = false;
                                    screen.copy_command(cx)
                                })
                            },
                        ))
                        .child(
                            btn("tournament-copy-config", "Copy config")
                                .full()
                                .on_click(move |_, cx| {
                                    copy_config.update(cx, |screen, cx| {
                                        screen.menu_open = false;
                                        screen.copy_config(cx)
                                    })
                                }),
                        ),
                )
                .with_priority(1)
            });
        let overflow = div()
            .relative()
            .child(
                btn("tournament-more", "⋯")
                    .tooltip(ui::i18n::t("More actions"))
                    .on_click(move |_, cx| {
                        more.update(cx, |screen, cx| {
                            screen.menu_open = !screen.menu_open;
                            cx.notify();
                        })
                    }),
            )
            .children(menu);

        let actions = h_flex()
            .gap(px(8.))
            .items_center()
            .flex_wrap()
            .child(
                btn("tournament-open-folder", "Open folder")
                    .tooltip(folder.display().to_string())
                    .on_click(move |_, _| crate::storage::reveal_folder(&folder)),
            )
            .when_some(open_last, |row, (number, index)| {
                let open = this.clone();
                row.child(btn("tournament-open-last", "Open last game").on_click(
                    move |window, cx| {
                        open.update(cx, |screen, cx| screen.open_game(number, index, window, cx))
                    },
                ))
            })
            .when(run.live.is_some() && run.active(), |row| {
                let watch = this.clone();
                row.child(
                    btn("tournament-watch-all", "Watch all live")
                        .primary()
                        .on_click(move |_, cx| watch.update(cx, |screen, cx| screen.watch_all(cx))),
                )
            })
            .when(!run.active(), |row| {
                let new = this.clone();
                row.child(
                    btn("tournament-new", ui::i18n::t("New tournament"))
                        .on_click(move |_, cx| new.update(cx, |screen, cx| screen.open_setup(cx))),
                )
            })
            .when(can_resume, |row| {
                let resume = this.clone();
                row.child(
                    btn("tournament-resume", ui::i18n::t("Continue"))
                        .primary()
                        .on_click(move |_, cx| {
                            let (dir, cwd, config) = (dir.clone(), cwd.clone(), config.clone());
                            resume.update(cx, |screen, cx| screen.resume(dir, cwd, config, cx))
                        }),
                )
            })
            .when(run.active(), |row| {
                let stop = this.clone();
                row.child(
                    div()
                        .id("tournament-stop")
                        .flex()
                        .items_center()
                        .h(px(32.))
                        .px(px(14.))
                        .rounded(px(8.))
                        .border_1()
                        .border_color(rgb(T.danger_border))
                        .bg(rgb(T.danger_bg))
                        .text_size(px(13.))
                        .text_color(rgb(T.danger_fg))
                        .cursor_pointer()
                        .child(ui::i18n::t("Stop"))
                        .on_click(move |_, _, cx| stop.update(cx, |screen, cx| screen.stop(cx))),
                )
            })
            .child(overflow);

        let header = card()
            .child(
                h_flex()
                    .justify_between()
                    .items_start()
                    .flex_wrap()
                    .gap(px(16.))
                    .child(
                        v_flex()
                            .gap(px(8.))
                            .min_w_0()
                            .child(
                                h_flex()
                                    .gap(px(14.))
                                    .items_center()
                                    .flex_wrap()
                                    .child(title)
                                    .child(pill),
                            )
                            .children(note.map(|(text, colour)| {
                                div()
                                    .text_size(px(13.))
                                    .text_color(rgb(colour))
                                    .child(SharedString::from(text))
                            })),
                    )
                    .child(actions),
            )
            .child(
                v_flex()
                    .gap(px(8.))
                    .child(
                        h_flex()
                            .justify_between()
                            .text_size(px(13.))
                            .text_color(rgb(T.muted))
                            .child(
                                h_flex()
                                    .gap(px(14.))
                                    .child(SharedString::from(format!(
                                        "{} / {} games",
                                        run.played(),
                                        total
                                    )))
                                    .child(
                                        div().font_family(ui::MONO).text_color(rgb(T.fg_2)).child(
                                            SharedString::from(format!("{:.0}%", fraction * 100.)),
                                        ),
                                    ),
                            )
                            .child(
                                h_flex()
                                    .gap(px(14.))
                                    .child(SharedString::from(format!("{live} live")))
                                    .child(SharedString::from(format!("{queued} queued")))
                                    .child(SharedString::from(format!("ETA {}", eta(run)))),
                            ),
                    )
                    .child(
                        div()
                            .h(px(8.))
                            .w_full()
                            .rounded(px(4.))
                            .bg(rgb(T.tour_tile))
                            .child(
                                div()
                                    .h_full()
                                    .rounded(px(4.))
                                    .bg(rgb(T.engine_a))
                                    .w(relative(fraction.clamp(0., 1.))),
                            ),
                    ),
            );

        let mut log = v_flex().gap(px(2.));
        for line in run.log.iter().rev().take(150) {
            log = log.child(
                div()
                    .font_family(MONO)
                    .text_size(px(12.))
                    .text_color(rgb(T.fg_2))
                    .child(SharedString::from(line.clone())),
            );
        }

        v_flex()
            .gap(px(20.))
            .child(header)
            .child(self.render_results(run))
            .child(self.render_live(run, cx))
            .child(self.render_games(run, cx))
            .child(locked_summary(run))
            .when(!run.active(), |column| {
                column.children(self.render_saved(cx).map(|saved| card().child(saved)))
            })
            .child(card().child(heading("fastchess output")).child(log))
            .into_any_element()
    }

    pub(super) fn render_results(&self, run: &Run) -> Div {
        let numbers = fastchess::results(&run.results);
        let view = numbers
            .as_ref()
            .and_then(|n| n.engines.as_ref().map(|(a, _)| a.clone()))
            .or_else(|| run.names.first().cloned())
            .unwrap_or_else(|| "Engine A".into());
        let games = numbers
            .as_ref()
            .map(|n| n.games)
            .unwrap_or_else(|| run.played());
        let body = match (&numbers, run.results.is_empty()) {
            (Some(n), _) => {
                let other = n
                    .engines
                    .as_ref()
                    .map(|(_, b)| b.clone())
                    .or_else(|| run.names.get(1).cloned())
                    .unwrap_or_else(|| "Engine B".into());
                results_body(n, &view, &other).into_any_element()
            }
            (None, false) => {
                let mut block = v_flex();
                for line in &run.results {
                    block = block.child(
                        div()
                            .font_family(MONO)
                            .text_size(px(12.5))
                            .whitespace_nowrap()
                            .child(SharedString::from(line.clone())),
                    );
                }
                div()
                    .id("tournament-results-block")
                    .overflow_x_scroll()
                    .p(px(12.))
                    .rounded(px(10.))
                    .bg(rgb(T.tour_tile))
                    .border_1()
                    .border_color(rgb(T.tour_tile_border))
                    .child(block)
                    .into_any_element()
            }
            (None, true) => {
                small("Results appear after the first finished game.").into_any_element()
            }
        };
        card()
            .child(
                h_flex()
                    .justify_between()
                    .items_center()
                    .flex_wrap()
                    .gap(px(8.))
                    .child(
                        h_flex()
                            .gap(px(10.))
                            .child(heading("Results"))
                            .child(small(format!("{games} games"))),
                    )
                    .child(small(format!("from {view}'s view"))),
            )
            .child(body)
    }

    /// Games in progress: a board per game, with its clocks and evaluation.
    pub(super) fn render_live(&self, run: &Run, cx: &mut Context<Self>) -> AnyElement {
        let count_text = if self.live_games.is_empty() {
            String::new()
        } else {
            format!("{} in progress", self.live_games.len())
        };
        let head = |more: Option<AnyElement>| {
            h_flex()
                .justify_between()
                .items_center()
                .gap(px(10.))
                .flex_wrap()
                .child(
                    h_flex()
                        .gap(px(10.))
                        .items_center()
                        .child(heading("Live games"))
                        .child(small(count_text.clone())),
                )
                .children(more)
        };
        if run.live.is_none() {
            return card()
                .child(head(None))
                .child(small(run.live_off.unwrap_or("Live boards are off.")))
                .into_any_element();
        }
        if self.live_games.is_empty() {
            return card()
                .child(head(None))
                .child(small(if run.active() {
                    "Waiting for the first moves…"
                } else {
                    "No games in progress."
                }))
                .into_any_element();
        }
        let this = cx.entity();
        let more = self.live_games.len() > LIVE_LIMIT;
        let toggle = more.then(|| {
            btn(
                "live-show-all",
                if self.live_all {
                    "Show fewer".to_owned()
                } else {
                    format!("Show all {}", self.live_games.len())
                },
            )
            .mini()
            .on_click(move |_, cx| {
                this.update(cx, |screen, cx| {
                    screen.live_all = !screen.live_all;
                    cx.notify();
                })
            })
        });
        let shown = if self.live_all {
            usize::MAX
        } else {
            LIVE_LIMIT
        };
        // As many columns as fit boards of at least LIVE_BOARD, with the boards grown to fill
        // the row so the grid sits flush with both sides of the card.
        let width = f32::from(self.live_width);
        let min_card = LIVE_BOARD + LIVE_CARD_EXTRA;
        let cols = ((width + LIVE_GAP) / (min_card + LIVE_GAP)).floor().max(1.);
        let edge = if width > 0. {
            ((width - LIVE_GAP * (cols - 1.)) / cols - LIVE_CARD_EXTRA)
                .floor()
                .max(LIVE_BOARD / 2.)
        } else {
            LIVE_BOARD
        };
        let mut grid = h_flex().flex_wrap().items_start().gap(px(LIVE_GAP));
        for game in self.live_games.iter().take(shown) {
            let Some((board, _, _)) = self.live_boards.get(&game.slot) else {
                continue;
            };
            let this = cx.entity();
            let slot = game.slot;
            let focused = self.live_focus == Some(slot);
            grid = grid.child(
                div()
                    .id(("live-card", slot))
                    .cursor_pointer()
                    .on_click(move |_, _, cx| {
                        this.update(cx, |screen, cx| screen.focus_live(slot, cx))
                    })
                    .child(game_card(game, board, px(edge), &run.names, focused)),
            );
        }
        let this = cx.entity().downgrade();
        let measure = canvas(
            move |bounds, _, cx| {
                let width = bounds.size.width;
                this.update(cx, |screen, cx| {
                    if (f32::from(screen.live_width) - f32::from(width)).abs() >= 1. {
                        screen.live_width = width;
                        cx.notify();
                    }
                })
                .ok();
            },
            |_, _, _, _| {},
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full();
        let grid = div().relative().w_full().child(measure).child(grid);
        let focus = self
            .live_focus
            .and_then(|slot| self.live_games.iter().find(|game| game.slot == slot))
            .map(|game| self.render_focus(game, &run.names));
        card()
            .child(head(toggle.map(IntoElement::into_any_element)))
            .children(focus)
            .child(grid)
            .into_any_element()
    }

    pub(super) fn render_focus(
        &self,
        game: &LiveGame,
        names: &[String],
    ) -> impl IntoElement + use<> {
        let (pos, _) = game.position();
        let start_ply = (pos.fullmoves().get() - 1) * 2 + u32::from(pos.turn().is_black());
        // The line of whoever searches now, from the current position.
        let search = if game.thinking.is_some() && game.thinking == game.white {
            Some(&game.white_search)
        } else if game.thinking.is_some() && game.thinking == game.black {
            Some(&game.black_search)
        } else {
            None
        };
        let pv = search
            .map(|search| pv_to_san(&pos, &search.pv))
            .filter(|sans| !sans.is_empty())
            .map(|sans| format_pv(start_ply, &sans))
            .unwrap_or_else(|| "–".into());
        let detail = |name: &Option<String>, search: &Search| {
            v_flex()
                .gap(px(2.))
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(SharedString::from(
                            name.clone().unwrap_or_else(|| "?".into()),
                        )),
                )
                .child(search_stats(search))
        };
        panel().p(px(14.)).child(
            h_flex()
                .items_start()
                .gap(px(18.))
                .child(
                    v_flex()
                        .gap(px(6.))
                        .child(player_line(game, false, 13.))
                        .child(div().size(px(360.)).child(self.focus_board.clone()))
                        .child(player_line(game, true, 13.)),
                )
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .gap(px(14.))
                        .child(detail(&game.white, &game.white_search))
                        .child(detail(&game.black, &game.black_search))
                        .child(
                            v_flex().gap(px(4.)).child(section_label("Line")).child(
                                div()
                                    .font_family(MONO)
                                    .text_size(px(13.))
                                    .child(SharedString::from(pv)),
                            ),
                        )
                        .child(eval_graph(game, names, 110.))
                        .child(div().text_size(px(12.)).text_color(rgb(T.muted)).child(
                            SharedString::from(format!(
                                "Game {}, move {}, {} plies played. Click the board's card again to close.",
                                game.number,
                                pos.fullmoves(),
                                game.moves.len()
                            )),
                        )),
                ),
        )
    }

    pub(super) fn render_empty(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let folder = fastchess::tournaments_dir(&self.data_dir(cx));
        let new = cx.entity();
        let saved = self
            .render_saved(cx)
            .map(|saved| card().mt(px(14.)).child(saved));
        v_flex()
            .gap(px(10.))
            .max_w(px(620.))
            .child(
                div()
                    .text_size(px(18.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(ui::i18n::t("Engine against engine")),
            )
            .child(div().text_color(rgb(T.fg_2)).child(
                "Pick two or more engines and a time control, then start. fastchess plays the \
                     games in the background; results, the game list and its output appear here.",
            ))
            .child(div().text_color(rgb(T.muted)).text_size(px(13.)).child(
                "Each tournament gets its own folder with games.pgn and config.json, so it can \
                 be stopped and continued later, here or with fastchess -config file=config.json.",
            ))
            .child(self.render_folder(folder, cx))
            .child(
                div().mt(px(6.)).child(
                    btn("tournament-new-empty", ui::i18n::t("New tournament"))
                        .primary()
                        .h(38.)
                        .on_click(move |_, cx| new.update(cx, |screen, cx| screen.open_setup(cx))),
                ),
            )
            .children(saved)
    }
}
