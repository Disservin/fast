//! The New tournament form: engines, time control, openings and the other settings.

use super::*;

impl TournamentScreen {
    pub(super) fn toggle_engine(&mut self, id: EngineId, on: bool, cx: &mut Context<Self>) {
        self.selected.retain(|selected| *selected != id);
        if on {
            self.selected.push(id);
        }
        cx.notify();
    }

    pub(super) fn add_executable(&mut self, cx: &mut Context<Self>) {
        let this = cx.entity().downgrade();
        pick_path(
            self.model.clone(),
            "Engine executable",
            None,
            cx,
            move |path, cx| {
                let _ = this.update(cx, |screen, cx| {
                    let name = path
                        .file_stem()
                        .map(|stem| stem.to_string_lossy().into_owned())
                        .unwrap_or_else(|| "engine".into());
                    screen.extra_engines.push(Participant {
                        name,
                        cmd: path,
                        options: Default::default(),
                    });
                    cx.notify();
                });
            },
        );
    }

    pub(super) fn browse_openings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let this = cx.entity().downgrade();
        let window_handle = window.window_handle();
        pick_path(
            self.model.clone(),
            "Opening book (.epd or .pgn)",
            None,
            cx,
            move |path, cx| {
                let _ = window_handle.update(cx, |_, window, cx| {
                    let _ = this.update(cx, |screen, cx| {
                        screen.openings.update(cx, |input, cx| {
                            input.set_value(path.display().to_string(), window, cx)
                        });
                        screen.book.update(cx, |book, cx| {
                            book.set_selected_index(
                                Some(gpui_component::IndexPath::new(
                                    crate::opening_books::BOOKS.len() + 1,
                                )),
                                window,
                                cx,
                            );
                        });
                    });
                });
            },
        );
    }

    pub(super) fn load_config_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let this = cx.entity().downgrade();
        let window_handle = window.window_handle();
        pick_path(
            self.model.clone(),
            "fastchess config.json",
            None,
            cx,
            move |path, cx| {
                let _ = window_handle.update(cx, |_, window, cx| {
                    let _ = this.update(cx, |screen, cx| match std::fs::read_to_string(&path) {
                        Ok(text) => screen
                            .paste
                            .update(cx, |input, cx| input.set_value(text, window, cx)),
                        Err(err) => screen.fail(format!("{}: {err}", path.display()), cx),
                    });
                });
            },
        );
    }

    pub(super) fn render_engines(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let engines = self.model.read(cx).config.engines.clone();
        let mut list = v_flex().gap(px(6.));
        if engines.is_empty() && self.extra_engines.is_empty() {
            list = list.child(div().text_size(px(13.)).text_color(rgb(T.muted)).child(
                ui::i18n::t("No engines yet. Add them in Settings › Engines, or add an executable here."),
            ));
        }
        for engine in engines {
            let id = engine.id;
            let order = self.selected.iter().position(|selected| *selected == id);
            let this = cx.entity();
            let seed = matches!(order, Some(0)) && self.gauntlet;
            let label = engine.name.clone();
            let selfplay = cx.entity();
            let copy = Participant {
                name: engine.name.clone(),
                cmd: engine.path.clone(),
                options: engine.options.clone(),
            };
            list = list.child(
                h_flex()
                    .justify_between()
                    .gap(px(8.))
                    .child(
                        h_flex()
                            .flex_1()
                            .min_w_0()
                            .gap(px(8.))
                            .child(
                                check(("tournament-engine", id as usize), order.is_some(), label)
                                    .text(13.)
                                    .on_change(move |on, _, cx| {
                                        this.update(cx, |screen, cx| {
                                            screen.toggle_engine(id, on, cx)
                                        })
                                    }),
                            )
                            .when(seed, |row| row.child(badge(ui::i18n::t("Seed")))),
                    )
                    .child(
                        btn(("selfplay-copy", id as usize), "Add selfplay copy")
                            .compact()
                            .tooltip(ui::i18n::t(
                                "Run another process of this engine as an opponent",
                            ))
                            .on_click(move |_, cx| {
                                selfplay.update(cx, |screen, cx| {
                                    if !screen.selected.contains(&id) {
                                        screen.selected.push(id);
                                    }
                                    screen.extra_engines.push(copy.clone());
                                    cx.notify();
                                })
                            }),
                    ),
            );
        }
        for (index, engine) in self.extra_engines.iter().enumerate() {
            let this = cx.entity();
            list = list.child(
                h_flex()
                    .gap(px(6.))
                    .child(
                        h_flex()
                            .flex_1()
                            .min_w_0()
                            .gap(px(10.))
                            .text_size(px(13.))
                            .child(
                                div()
                                    .flex_none()
                                    .child(SharedString::from(engine.name.clone())),
                            )
                            .child(
                                div()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .whitespace_nowrap()
                                    .text_size(px(12.))
                                    .text_color(rgb(T.muted))
                                    .child(SharedString::from(engine.cmd.display().to_string())),
                            ),
                    )
                    .child(
                        btn(("tournament-extra-remove", index), "×")
                            .h(24.)
                            .w(24.)
                            .pad(3.)
                            .tooltip(ui::i18n::t("Remove"))
                            .on_click(move |_, cx| {
                                this.update(cx, |screen, cx| {
                                    screen.extra_engines.remove(index);
                                    cx.notify();
                                })
                            }),
                    ),
            );
        }
        let add = cx.entity();
        v_flex()
            .gap(px(8.))
            .child(section_label(ui::i18n::t("Engines")))
            .child(list)
            .child(
                h_flex().child(
                    btn("tournament-add-exe", "Add executable…").on_click(move |_, cx| {
                        add.update(cx, |screen, cx| screen.add_executable(cx))
                    }),
                ),
            )
    }

    pub(super) fn toggle<F>(
        &self,
        id: &'static str,
        on: bool,
        label: &'static str,
        cx: &mut Context<Self>,
        set: F,
    ) -> impl IntoElement + use<F>
    where
        F: Fn(&mut Self, bool) + 'static,
    {
        let this = cx.entity();
        check(id, on, label).text(13.).on_change(move |on, _, cx| {
            this.update(cx, |screen, cx| {
                set(screen, on);
                cx.notify();
            })
        })
    }

    /// A segmented control whose choice is stored with `set`.
    pub(super) fn choice<V, F>(
        &self,
        id: &'static str,
        options: &[(V, &'static str)],
        selected: V,
        cx: &mut Context<Self>,
        set: F,
    ) -> gpui::Div
    where
        V: Copy + PartialEq + 'static,
        F: Fn(&mut Self, V) + 'static,
    {
        let this = cx.entity();
        segmented(
            id,
            options
                .iter()
                .map(|(value, label)| (*value, ui::i18n::t(label).into(), false))
                .collect(),
            selected,
            move |value, _, cx| {
                this.update(cx, |screen, cx| {
                    set(screen, value);
                    cx.notify();
                })
            },
        )
    }

    /// The New tournament form: the basic settings, then the advanced ones when open.
    pub(super) fn render_form(
        &self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let advanced = self.advanced_open.then(|| {
            v_flex()
                .gap(px(14.))
                .pl(px(12.))
                .border_l_2()
                .border_color(rgb(T.border))
                .child(self.render_pairing_settings(cx))
                .child(self.render_adjudication_settings(cx))
                .child(self.render_saved_game_settings(cx))
                .child(self.render_other_settings(cx))
        });
        v_flex()
            .gap(px(16.))
            .child(self.render_basic_settings(cx))
            .child(self.render_advanced_disclosure(cx))
            .children(advanced)
    }

    /// What most tournaments need: who plays, how fast, how many games, which book.
    fn render_basic_settings(&self, cx: &mut Context<Self>) -> Div {
        let limit_this = cx.entity();
        let browse = cx.entity();
        v_flex()
            .gap(px(14.))
            .child(self.render_engines(cx))
            .child(section_label(ui::i18n::t("Time")))
            .child(segmented(
                "tournament-limit",
                Limit::ALL
                    .iter()
                    .map(|limit| (*limit, SharedString::from(limit.label()), false))
                    .collect(),
                self.limit,
                move |limit, _, cx| {
                    limit_this.update(cx, |screen, cx| {
                        screen.limit = limit;
                        cx.notify();
                    })
                },
            ))
            .child(match self.limit {
                Limit::TimeControl => self.clock_input.render(),
                Limit::MoveTime => self.move_time.render("Time per move"),
                _ => field(self.limit.hint(), text_input(&self.limit_value).mono()),
            })
            .child(section_label(ui::i18n::t("Games")))
            .child(pair(
                field("Rounds", text_input(&self.rounds)),
                field("Concurrency", text_input(&self.concurrency)),
            ))
            .child(field(
                "Variant",
                self.choice(
                    "tournament-variant",
                    &[(false, "Standard"), (true, "Chess960")],
                    self.chess960,
                    cx,
                    |screen, on| screen.chess960 = on,
                ),
            ))
            .when(self.chess960, |column| {
                column.child(div().text_size(px(12.)).text_color(rgb(T.muted)).child(
                    ui::i18n::t("Chess960 needs a book of Chess960 start positions."),
                ))
            })
            .child(field("Opening book", select_el(&self.book)))
            .child(div().text_size(px(12.)).text_color(rgb(T.muted)).child(
                ui::i18n::t("Recommended: UHO_Lichess_4852_v1.epd. Stockfish books download on first use. Choose Custom file to use a local book.")
            ))
            .child(field(
                "Custom book (.epd or .pgn)",
                h_flex()
                    .gap(px(6.))
                    .child(div().flex_1().child(text_input(&self.openings)))
                    .child(
                        btn("tournament-book", ui::i18n::t("Browse…")).on_click(move |window, cx| {
                            browse.update(cx, |screen, cx| screen.browse_openings(window, cx))
                        }),
                    ),
            ))
            .child(self.toggle(
                "tournament-sprt",
                self.sprt,
                "Stop by SPRT",
                cx,
                |screen, on| screen.sprt = on,
            ))
            .when(self.sprt, |column| {
                column
                    .child(pair(
                        field("elo0", text_input(&self.elo0)),
                        field("elo1", text_input(&self.elo1)),
                    ))
                    .child(pair(
                        field("alpha", text_input(&self.alpha)),
                        field("beta", text_input(&self.beta)),
                    ))
                    .child(field(
                        "Elo model",
                        self.choice(
                            "tournament-sprt-model",
                            &[
                                ("normalized", "Normalized"),
                                ("logistic", "Logistic"),
                                ("bayesian", "BayesElo"),
                            ],
                            self.sprt_model,
                            cx,
                            |screen, model| screen.sprt_model = model,
                        ),
                    ))
            })
    }

    /// The row that opens and closes the advanced settings, listing what they hold.
    fn render_advanced_disclosure(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let open = self.advanced_open;
        let expand = cx.entity();
        h_flex()
            .id("tournament-advanced")
            .gap(px(8.))
            .items_center()
            .py(px(10.))
            .px(px(12.))
            .rounded(px(8.))
            .border_1()
            .border_color(rgb(T.border))
            .cursor_pointer()
            .hover(|row| row.bg(rgb(T.hover)))
            .child(div().w(px(12.)).text_color(rgb(T.muted)).child(if open {
                "▾"
            } else {
                "▸"
            }))
            .child(
                div()
                    .text_size(px(13.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(ui::i18n::t("Advanced settings")),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .text_size(px(12.))
                    .text_color(rgb(T.muted))
                    .child(ui::i18n::t(
                        "Hash and threads, pairing, adjudication, tablebases, saved games, extra options",
                    )),
            )
            .on_click(move |_, _, cx| {
                expand.update(cx, |screen, cx| {
                    screen.advanced_open = !screen.advanced_open;
                    cx.notify();
                })
            })
    }

    /// Engine hash, threads and arguments, and how the engines are paired.
    fn render_pairing_settings(&self, cx: &mut Context<Self>) -> Div {
        let type_this = cx.entity();
        let games_this = cx.entity();
        let order_this = cx.entity();
        v_flex()
            .gap(px(14.))
            .child(section_label(ui::i18n::t("Engines")))
            .child(pair(
                field("Hash (MB)", text_input(&self.hash)),
                field("Threads", text_input(&self.threads)),
            ))
            .child(field("Engine arguments", text_input(&self.engine_args)))
            .child(section_label("Pairing"))
            .child(segmented(
                "tournament-type",
                vec![
                    (false, "Round robin".into(), false),
                    (true, "Gauntlet".into(), false),
                ],
                self.gauntlet,
                move |gauntlet, _, cx| {
                    type_this.update(cx, |screen, cx| {
                        screen.gauntlet = gauntlet;
                        cx.notify();
                    })
                },
            ))
            .child(field(
                "Games per round",
                segmented(
                    "tournament-games",
                    vec![
                        (1, "1".into(), false),
                        (2, "2, colours swapped".into(), false),
                    ],
                    self.games_per_round,
                    move |games, _, cx| {
                        games_this.update(cx, |screen, cx| {
                            screen.games_per_round = games;
                            cx.notify();
                        })
                    },
                ),
            ))
            .child(
                h_flex()
                    .gap(px(16.))
                    .child(self.toggle(
                        "tournament-noswap",
                        self.noswap,
                        "Never swap colours",
                        cx,
                        |screen, on| screen.noswap = on,
                    ))
                    .child(self.toggle(
                        "tournament-reverse",
                        self.reverse,
                        "Reverse colours",
                        cx,
                        |screen, on| screen.reverse = on,
                    )),
            )
            .child(pair(
                field(
                    "Book order",
                    segmented(
                        "tournament-order",
                        vec![
                            (true, ui::i18n::t("Random").into(), false),
                            (false, "Sequential".into(), false),
                        ],
                        self.random_order,
                        move |random, _, cx| {
                            order_this.update(cx, |screen, cx| {
                                screen.random_order = random;
                                cx.notify();
                            })
                        },
                    ),
                ),
                field("Plies from book", text_input(&self.plies)),
            ))
    }

    /// When games end early: draw, resign, maximum length and tablebase adjudication.
    fn render_adjudication_settings(&self, cx: &mut Context<Self>) -> Div {
        v_flex()
            .gap(px(14.))
            .child(section_label("Adjudication"))
            .child(self.toggle(
                "tournament-draw",
                self.draw_adjudication,
                "Draw adjudication",
                cx,
                |screen, on| screen.draw_adjudication = on,
            ))
            .when(self.draw_adjudication, |column| {
                column.child(triple(
                    field("From move", text_input(&self.draw_movenumber)),
                    field("For moves in a row", text_input(&self.draw_movecount)),
                    field("Within ± cp", text_input(&self.draw_score)),
                ))
            })
            .child(self.toggle(
                "tournament-resign",
                self.resign_adjudication,
                "Resign adjudication",
                cx,
                |screen, on| screen.resign_adjudication = on,
            ))
            .when(self.resign_adjudication, |column| {
                column
                    .child(pair(
                        field("For moves in a row", text_input(&self.resign_movecount)),
                        field("Beyond cp", text_input(&self.resign_score)),
                    ))
                    .child(self.toggle(
                        "tournament-resign-twosided",
                        self.resign_twosided,
                        "Both engines must agree on the score",
                        cx,
                        |screen, on| screen.resign_twosided = on,
                    ))
            })
            .child(field("Maximum moves", text_input(&self.maxmoves)))
            .child(self.toggle(
                "tournament-tb",
                self.tb_adjudication,
                "Tablebase adjudication",
                cx,
                |screen, on| screen.tb_adjudication = on,
            ))
            .when(self.tb_adjudication, |column| {
                column
                    .child(pair(
                        field(
                            "Syzygy folders",
                            h_flex()
                                .gap(px(6.))
                                .child(div().flex_1().min_w_0().child(text_input(&self.tb_paths)))
                                .child(crate::controls::browse_path(
                                    "tournament-syzygy-browse",
                                    &self.model,
                                    &self.tb_paths,
                                    "Syzygy folders",
                                    true,
                                )),
                        ),
                        field("Up to pieces", text_input(&self.tb_pieces)),
                    ))
                    .child(self.choice(
                        "tournament-tb-adjudicate",
                        &[
                            ("BOTH", "Wins and draws"),
                            ("WIN_LOSS", "Wins only"),
                            ("DRAW", "Draws only"),
                        ],
                        self.tb_adjudicate,
                        cx,
                        |screen, mode| screen.tb_adjudicate = mode,
                    ))
                    .child(self.toggle(
                        "tournament-tb-ignore50",
                        self.tb_ignore50,
                        "Ignore the 50-move rule",
                        cx,
                        |screen, on| screen.tb_ignore50 = on,
                    ))
            })
    }

    /// What the saved PGN records with each move.
    fn render_saved_game_settings(&self, cx: &mut Context<Self>) -> Div {
        v_flex()
            .gap(px(14.))
            .child(section_label("Saved games"))
            .child(field(
                "Notation",
                self.choice(
                    "tournament-pgn-notation",
                    &[("san", "SAN"), ("lan", "LAN"), ("uci", "UCI")],
                    self.pgn.notation,
                    cx,
                    |screen, notation| screen.pgn.notation = notation,
                ),
            ))
            .when(self.pgn.notation != "san", |column| {
                column.child(
                    div()
                        .text_size(px(12.))
                        .text_color(rgb(T.warn))
                        .child(ui::i18n::t("Games in LAN or UCI may not open in Analyse.")),
                )
            })
            .child(field("Record with each move", {
                let mut grid = div().grid().grid_cols(3).gap(px(8.));
                let flags: [PgnFlag; 8] = [
                    (
                        "tournament-pgn-nodes",
                        "Nodes",
                        self.pgn.nodes,
                        |pgn, on| pgn.nodes = on,
                    ),
                    (
                        "tournament-pgn-seldepth",
                        "Selective depth",
                        self.pgn.seldepth,
                        |pgn, on| pgn.seldepth = on,
                    ),
                    ("tournament-pgn-nps", "Speed", self.pgn.nps, |pgn, on| {
                        pgn.nps = on
                    }),
                    (
                        "tournament-pgn-hashfull",
                        "Hash usage",
                        self.pgn.hashfull,
                        |pgn, on| pgn.hashfull = on,
                    ),
                    (
                        "tournament-pgn-tbhits",
                        "Tablebase hits",
                        self.pgn.tbhits,
                        |pgn, on| pgn.tbhits = on,
                    ),
                    ("tournament-pgn-pv", "Full line", self.pgn.pv, |pgn, on| {
                        pgn.pv = on
                    }),
                    (
                        "tournament-pgn-timeleft",
                        "Time left",
                        self.pgn.timeleft,
                        |pgn, on| pgn.timeleft = on,
                    ),
                    (
                        "tournament-pgn-latency",
                        "Latency",
                        self.pgn.latency,
                        |pgn, on| pgn.latency = on,
                    ),
                ];
                for (id, label, on, set) in flags {
                    grid = grid.child(self.toggle(id, on, label, cx, move |screen, on| {
                        set(&mut screen.pgn, on)
                    }));
                }
                grid
            }))
            .child(self.toggle(
                "tournament-pgn-min",
                self.pgn.min,
                "Minimal PGN",
                cx,
                |screen, on| screen.pgn.min = on,
            ))
            .child(field(
                "Copy engine output matching",
                text_input(&self.pgn_match_line),
            ))
    }

    /// Crash recovery, seeds, delays, margins and other fastchess options.
    fn render_other_settings(&self, cx: &mut Context<Self>) -> Div {
        v_flex()
            .gap(px(14.))
            .child(section_label("Other"))
            .child(self.toggle(
                "tournament-recover",
                self.recover,
                "Recover crashed engines",
                cx,
                |screen, on| screen.recover = on,
            ))
            .child(pair(
                field("Random seed", text_input(&self.seed)),
                field("Delay between games (ms)", text_input(&self.wait)),
            ))
            .child(field("Time margin (ms)", text_input(&self.time_margin)))
            .child(self.toggle(
                "tournament-restart",
                self.restart,
                "Restart engines between games",
                cx,
                |screen, on| screen.restart = on,
            ))
            .child(self.toggle(
                "tournament-affinity",
                self.affinity,
                "Bind engines to CPU cores",
                cx,
                |screen, on| screen.affinity = on,
            ))
            .child(field("Extra fastchess options", text_input(&self.extra)))
    }
}

/// Two form fields side by side, sharing the width.
fn pair(a: Div, b: Div) -> Div {
    h_flex()
        .gap(px(8.))
        .items_start()
        .child(a.flex_1())
        .child(b.flex_1())
}

/// Three form fields side by side, sharing the width.
fn triple(a: Div, b: Div, c: Div) -> Div {
    pair(a, b).child(c.flex_1())
}
