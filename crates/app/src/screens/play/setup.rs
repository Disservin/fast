//! The new game form: mode, colour, time control and each engine's settings.

use super::*;

pub(super) fn supports_limit(config: &EngineConfig) -> bool {
    config
        .info
        .options
        .iter()
        .any(|option| option.name == "UCI_Elo")
        || config.name.eq_ignore_ascii_case("stockfish")
}

impl PlayScreen {
    pub(super) fn time_control(&self, cx: &gpui::App) -> Option<(u64, u64)> {
        match self.time_index {
            Some(index) => TIME_CONTROLS
                .get(index)
                .map(|&(m, i)| (u64::from(m) * 60_000, u64::from(i) * 1_000)),
            None => self.custom_time.milliseconds(cx),
        }
    }

    /// The engine picked in `choice`, or `None` when no engine is set up for Play.
    pub(super) fn chosen_engine(
        &self,
        choice: &EngineChoice,
        cx: &gpui::App,
    ) -> Option<EngineConfig> {
        let index = choice.engine.read(cx).selected_index(cx)?.row;
        let mut config = self.opponents.get(index)?.clone();
        if self.mode == Mode::Engines {
            for (name, value) in [("Threads", choice.threads), ("Hash", choice.hash)] {
                if let Some(option) = config
                    .info
                    .options
                    .iter()
                    .find(|option| option.name == name)
                    && let OptionKind::Spin { min, max, .. } = option.kind
                {
                    config.options.insert(
                        name.to_owned(),
                        i64::from(value).clamp(min, max).to_string(),
                    );
                }
            }
        }
        Some(config)
    }

    pub(super) fn render_setup(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let this = cx.entity();
        let grid = self.render_time_grid(cx);
        let start_this = this.clone();
        let can_start = !self.opponents.is_empty();
        let self_play = self.mode == Mode::Engines;
        let from_fen = self
            .start_from
            .read(cx)
            .selected_index(cx)
            .is_some_and(|index| index.row == 3);
        let from_960 = self
            .start_from
            .read(cx)
            .selected_index(cx)
            .is_some_and(|index| index.row == 4);
        let mode = choice(
            "play-mode",
            [
                (Mode::VsEngine, "Vs engine"),
                (Mode::Engines, "Engine vs engine"),
            ],
            self.mode,
            cx,
            |screen, mode| screen.mode = mode,
        );
        let engines = if self_play {
            let mut row = h_flex().items_start().gap(px(12.));
            for (index, label, choice, pick) in [
                (0, "White", &self.white, (|s| &mut s.white) as PickChoice),
                (1, "Black", &self.black, (|s| &mut s.black) as PickChoice),
            ] {
                row = row.child(
                    panel()
                        .flex_1()
                        .min_w_0()
                        .p(px(12.))
                        .child(section_label(label).mb(px(12.)))
                        .child(self.render_choice(label, choice, pick, cx))
                        .child(self.render_engine_time(index, cx)),
                );
            }
            v_flex().child(row).mb(px(16.))
        } else {
            self.render_choice(
                "Opponent",
                &self.opponent,
                |screen| &mut screen.opponent,
                cx,
            )
        };
        v_flex()
            .id("play-setup")
            .w(px(if self_play { 580. } else { 300. }))
            .h_full()
            .flex_none()
            .overflow_y_scroll()
            .px(px(18.))
            .pt(px(20.))
            .pb(px(16.))
            .bg(rgb(T.surface))
            .border_r_as(T.border)
            .child(section_label("New game").mb(px(10.)))
            .when(self_play && self.game.is_some(), |column| {
                column.child(
                    btn("back-to-engine-game", "Back to game")
                        .compact()
                        .on_click(this.listen(move |screen, _, cx| {
                            screen.show_setup = false;
                            cx.notify();
                        })),
                )
            })
            .child(mode.mb(px(14.)))
            .child(engines)
            .when(!self_play, |column| {
                column.child(field("Time control", grid).mb(px(14.)))
            })
            .when(self_play, |column| {
                column
                    .child(
                        field(
                            "Opening book (.pgn or .epd)",
                            h_flex()
                                .gap(px(6.))
                                .child(div().flex_1().min_w_0().child(text_input(&self.book_path)))
                                .child(crate::controls::browse_path(
                                    "play-book-browse",
                                    &self.model,
                                    &self.book_path,
                                    "Opening book (.pgn or .epd)",
                                    false,
                                )),
                        )
                        .mb(px(6.)),
                    )
                    .child(div().text_as(12., T.muted).mb(px(14.)).child(ui::i18n::t(
                        "Optional local file. Uses the first game or position.",
                    )))
            })
            .when(self.time_index.is_none() && !self_play, |column| {
                column.child(div().mb(px(14.)).child(self.custom_time.render()))
            })
            .when(!self_play, |column| {
                column.child(self.render_colour_choice(cx))
            })
            .child(
                field("Start from", select_el(&self.start_from)).mb(px(if from_fen {
                    6.
                } else {
                    20.
                })),
            )
            .when(from_fen, |column| {
                column.child(div().mb(px(20.)).child(text_input(&self.start_fen)))
            })
            .when(from_960, |column| {
                column.child(self.render_chess960_picker(cx))
            })
            .when(!self_play, |column| {
                column.child(self.render_game_options(cx))
            })
            .child(
                btn("start-game", "Start game")
                    .primary()
                    .full()
                    .h(40.)
                    .disabled(!can_start || self.book_task.is_some())
                    .on_click(
                        start_this
                            .listen(move |screen, window, cx| screen.start_with_book(window, cx)),
                    ),
            )
            .when(!can_start, |column| {
                column.child(
                    div()
                        .mt(px(10.))
                        .text_as(13., T.muted)
                        .child(ui::i18n::t("Add an engine in Settings › Engines to play.")),
                )
            })
    }

    /// The time control presets and Custom.
    fn render_time_grid(&self, cx: &mut Context<Self>) -> gpui::Div {
        let this = cx.entity();
        let mut grid = div().grid().grid_cols(3).gap(px(6.));
        for (index, (base, increment)) in TIME_CONTROLS.iter().enumerate() {
            grid = grid.child(
                btn(("time", index), format!("{base}+{increment}"))
                    .mono()
                    .full()
                    .h(36.)
                    .selected(self.time_index == Some(index))
                    .on_click(this.listen(move |screen, _, cx| {
                        screen.time_index = Some(index);
                        cx.notify();
                    })),
            );
        }
        grid = grid.child(
            btn("time-custom", "Custom")
                .full()
                .selected(self.time_index.is_none())
                .on_click(this.listen(move |screen, _, cx| {
                    screen.time_index = None;
                    cx.notify();
                })),
        );
        grid
    }

    /// White, Random or Black for the player.
    fn render_colour_choice(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let this = cx.entity();
        let colour_button =
            |id: &'static str, label: &'static str, choice: ColourChoice, current: ColourChoice| {
                btn(id, label)
                    .flex_1()
                    .selected(choice == current)
                    .on_click(this.listen(move |screen, _, cx| {
                        screen.colour = choice;
                        cx.notify();
                    }))
            };
        field(
            "Colour",
            h_flex()
                .gap(px(6.))
                .child(colour_button(
                    "white",
                    ui::i18n::t("White"),
                    ColourChoice::White,
                    self.colour,
                ))
                .child(colour_button(
                    "random",
                    "Random",
                    ColourChoice::Random,
                    self.colour,
                ))
                .child(colour_button(
                    "black",
                    ui::i18n::t("Black"),
                    ColourChoice::Black,
                    self.colour,
                )),
        )
        .mb(px(14.))
    }

    /// The Chess960 start position: its number, Random, and the back rank it gives.
    fn render_chess960_picker(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let this = cx.entity();
        let number = self
            .chess960_number
            .read(cx)
            .value()
            .trim()
            .parse::<u16>()
            .ok()
            .filter(|n| *n < 960);
        h_flex()
            .flex_wrap()
            .gap(px(8.))
            .mb(px(16.))
            .child(
                div()
                    .w(px(80.))
                    .child(text_input(&self.chess960_number).mono()),
            )
            .child(
                btn("random-chess960", "Random")
                    .icon("icons/dices.svg")
                    .compact()
                    .on_click(this.listen(move |screen, window, cx| {
                        let number = crate::shell::random_chess960_number();
                        screen.chess960_number.update(cx, |input, cx| {
                            input.set_value(number.to_string(), window, cx)
                        });
                        cx.notify();
                    })),
            )
            .child(mono(number.map_or_else(
                || ui::i18n::t("Enter a number from 0 to 959").to_owned(),
                |n| format!("#{n} {}", chess_core::chess960::back_rank(n)),
            )))
    }

    /// Hide evaluation, premoves and takebacks for a game against an engine.
    fn render_game_options(&self, cx: &mut Context<Self>) -> gpui::Div {
        let this = cx.entity();
        v_flex()
            .child(
                div().mb(px(12.)).child(
                    check(
                        "hide-eval",
                        self.hide_eval,
                        "Hide evaluation during the game",
                    )
                    .on_change(this.listen_with(
                        move |screen, value, _, cx| {
                            screen.hide_eval = value;
                            cx.notify();
                        },
                    )),
                ),
            )
            .child(div().mb(px(12.)).child({
                check("premoves", self.allow_premoves, "Allow premoves").on_change(
                    this.listen_with(move |screen, value, _, cx| {
                        screen.allow_premoves = value;
                        cx.notify();
                    }),
                )
            }))
            .child(div().mb(px(20.)).child(
                check("takebacks", self.allow_takebacks, "Allow takebacks").on_change(
                    this.listen_with(move |screen, value, _, cx| {
                        screen.allow_takebacks = value;
                        cx.notify();
                    }),
                ),
            ))
    }

    /// One engine of the new game: which engine, and how strongly it plays.
    /// Shows a separate clock and unlimited-search choice for one engine.
    pub(super) fn render_engine_time(&self, index: usize, cx: &mut Context<Self>) -> gpui::Div {
        let settings = &self.engine_times[index];
        let this = cx.entity();
        v_flex()
            .gap(px(8.))
            .child(
                check(
                    SharedString::from(format!("unlimited-{index}")),
                    settings.unlimited,
                    "Unlimited time",
                )
                .on_change(this.listen_with(move |screen, value, _, cx| {
                    screen.engine_times[index].unlimited = value;
                    cx.notify();
                })),
            )
            .when(!settings.unlimited, |column| {
                column.child(settings.time.render())
            })
            .when(settings.unlimited, |column| {
                column.child(field(
                    "Per-move search limit",
                    v_flex()
                        .gap(px(6.))
                        .child(select_el(&settings.kind))
                        .child(text_input(&settings.value).mono()),
                ))
            })
    }

    pub(super) fn render_choice(
        &self,
        label: &'static str,
        choice: &EngineChoice,
        pick: PickChoice,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let this = cx.entity();
        // Without an engine to ask, the slider stays as a preview of the setup.
        let limitable = self
            .chosen_engine(choice, cx)
            .is_none_or(|config| supports_limit(&config));
        let limited = limitable && !choice.full;
        v_flex()
            .child(field(ui::i18n::t(label), select_el(&choice.engine)).mb(px(14.)))
            .when(self.mode == Mode::Engines, |column| {
                let mut column = column;
                for (name, value, step) in [
                    ("Threads", choice.threads, 1),
                    ("Hash (MB)", choice.hash, 64),
                ] {
                    let entity = cx.entity();
                    let step = move |up: bool| {
                        let this = entity.clone();
                        move |_: &mut gpui::Window, cx: &mut gpui::App| {
                            this.update(cx, |screen, cx| {
                                let choice = pick(screen);
                                let value = if name == "Threads" {
                                    &mut choice.threads
                                } else {
                                    &mut choice.hash
                                };
                                *value = if up {
                                    value.saturating_add(step)
                                } else {
                                    value.saturating_sub(step).max(1)
                                };
                                cx.notify();
                            });
                        }
                    };
                    column = column.child(
                        h_flex()
                            .gap(px(6.))
                            .mb(px(8.))
                            .child(div().flex_1().text_size(px(13.)).child(ui::i18n::t(name)))
                            .child(
                                ui::widgets::stepper(
                                    SharedString::from(format!("{label}-{name}")),
                                    value.to_string(),
                                )
                                .on_down(true, step(false))
                                .on_up(true, step(true)),
                            ),
                    );
                }
                column
            })
            .child(
                v_flex()
                    .mb(px(16.))
                    .gap(px(6.))
                    .child(
                        h_flex()
                            .justify_between()
                            .text_as(13., T.muted)
                            .child(ui::i18n::t("Strength"))
                            .when(limited, |row| {
                                row.child(mono(choice.elo.to_string()).text_color(rgb(T.fg_2)))
                            })
                            .when(!limitable, |row| {
                                row.child(
                                    div()
                                        .text_color(rgb(T.fg_2))
                                        .child(ui::i18n::t("Full strength")),
                                )
                            }),
                    )
                    .when(limited, |column| {
                        column.child(Slider::new(&choice.strength).bg(rgb(T.accent)))
                    })
                    .when(limitable, |column| {
                        column.child(
                            check(
                                SharedString::from(format!("full-strength-{label}")),
                                choice.full,
                                "Full strength",
                            )
                            .on_change(this.listen_with(
                                move |screen, value, _, cx| {
                                    pick(screen).full = value;
                                    cx.notify();
                                },
                            )),
                        )
                    }),
            )
    }
}
