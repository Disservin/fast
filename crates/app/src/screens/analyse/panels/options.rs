//! The Search options tab and table tools: lines per engine, threads, hash and other numbers.

use super::*;

/// An engine's value, minimum and maximum for a spin option it declares.
pub fn spin_option(config: &EngineConfig, name: &str) -> Option<(i64, i64, i64)> {
    let option = config
        .info
        .options
        .iter()
        .find(|option| option.name == name)?;
    let OptionKind::Spin { default, min, max } = option.kind else {
        return None;
    };
    let value = config
        .options
        .get(name)
        .and_then(|value| value.parse().ok())
        .unwrap_or(default);
    Some((value, min, max))
}

/// What the Lines buttons ask for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinesTarget {
    Step(bool),
    /// The engine's own default.
    Default,
    /// As many lines as the engine allows, up to [`MAX_LINES`]: every move in every
    /// position, without changing as the game moves on.
    All,
}

/// More lines than any position has legal moves (218 at most).
pub const MAX_LINES: i64 = 256;

/// The MultiPV value for a Lines button, within the engine's range.
pub fn lines_target(target: LinesTarget, current: i64, default: i64, min: i64, max: i64) -> i64 {
    match target {
        LinesTarget::Step(up) => step_value("MultiPV", current, up, min, max),
        LinesTarget::Default => default.clamp(min, max),
        LinesTarget::All => MAX_LINES.clamp(min, max),
    }
}

/// The next stepper value: Hash doubles and halves, everything else moves by one.
pub fn step_value(name: &str, value: i64, up: bool, min: i64, max: i64) -> i64 {
    let next = match (name, up) {
        ("Hash", true) => value.saturating_mul(2),
        ("Hash", false) => value / 2,
        (_, true) => value.saturating_add(1),
        (_, false) => value.saturating_sub(1),
    };
    next.clamp(min, max)
}

impl AnalyseScreen {
    /// Lines each engine searches (MultiPV), for every analysis engine at once:
    /// one fewer, one more, back to each engine's default, or as many as allowed.
    pub(super) fn render_lines_control(&self, cx: &mut Context<Self>) -> Option<Div> {
        let this = cx.entity();
        let small = |id: &'static str, label: &'static str| btn(id, label).compact();
        let lines = self.shown_lines(cx);
        lines.map(|value| {
            let (down_this, up_this) = (this.clone(), this.clone());
            let (reset_this, max_this) = (this.clone(), this.clone());
            h_flex()
                .gap(px(2.))
                .mr(px(4.))
                .child(
                    div()
                        .mr(px(2.))
                        .text_as(13., T.muted)
                        .child(ui::i18n::t("Lines")),
                )
                .child(
                    ui::widgets::stepper("lines", value.to_string())
                        .value_width(24.)
                        .tooltips(
                            "Search one fewer engine line",
                            "MultiPV: lines each engine searches, saved in its options",
                        )
                        .on_down(value > 1, move |_, cx| {
                            down_this.update(cx, |screen, cx| screen.step_lines(false, cx));
                        })
                        .on_up(true, move |_, cx| {
                            up_this.update(cx, |screen, cx| screen.step_lines(true, cx));
                        }),
                )
                .child(
                    small("lines-reset", ui::i18n::t("Reset"))
                        .tooltip(ui::i18n::t("Back to each engine's default number of lines"))
                        .on_click(move |_, cx| {
                            reset_this.update(cx, |screen, cx| {
                                screen.set_lines(LinesTarget::Default, cx)
                            });
                        }),
                )
                .child(
                    small("lines-max", "Max")
                        .tooltip(ui::i18n::t("A line for every legal move in every position (up to 256, as far as the engine allows)"))
                        .on_click(move |_, cx| {
                            max_this
                                .update(cx, |screen, cx| screen.set_lines(LinesTarget::All, cx));
                        }),
                )
        })
    }

    /// The Moves header's controls: the view settings on the left, and the pause
    /// button and panel toggles on the right.
    pub(crate) fn render_table_tools(&self, cx: &mut Context<Self>) -> (AnyElement, AnyElement) {
        let this = cx.entity();
        let live = self.live.read(cx);
        let (on, paused) = (live.on, live.paused);
        let view = self.model.read(cx).config.view.clone();
        let all_this = this.clone();
        let arrows_this = this.clone();
        let mine_this = this.clone();
        let small = |id: &'static str, label: &'static str| btn(id, label).compact();
        let view_tools = h_flex()
            .flex_wrap()
            .gap(px(4.))
            .mr(px(6.))
            .child(
                small("all-moves", ui::i18n::t("All moves"))
                    .selected(view.all_moves)
                    .tooltip(ui::i18n::t("List every legal move, analysed or not"))
                    .on_click(move |_, cx| {
                        all_this.update(cx, |screen, cx| {
                            screen.set_view(cx, |view| view.all_moves = !view.all_moves);
                        });
                    }),
            )
            .child(
                small("quality-arrows", "Arrows")
                    .selected(view.quality_arrows)
                    .tooltip(ui::i18n::t(
                        "An arrow per engine line, coloured from best (blue) to blunder (red)",
                    ))
                    .on_click(move |_, cx| {
                        arrows_this.update(cx, |screen, cx| {
                            screen.set_view(cx, |view| view.quality_arrows = !view.quality_arrows);
                        });
                    }),
            )
            .child(
                small("my-moves", "My moves")
                    .selected(view.my_moves)
                    .tooltip(ui::i18n::t(
                        "Show what you played and your repertoire for each move",
                    ))
                    .on_click(move |_, cx| {
                        mine_this.update(cx, |screen, cx| {
                            screen.set_view(cx, |view| view.my_moves = !view.my_moves);
                        });
                    }),
            );
        let pause_this = this.clone();
        let log_this = this.clone();
        let online_this = this;
        let settings = h_flex()
            .flex_wrap()
            .gap(px(8.))
            .child(view_tools)
            .into_any_element();
        let toggles = h_flex()
            .flex_wrap()
            .gap(px(6.))
            .child(
                icon_btn(
                    "engine-pause",
                    if on && !paused {
                        "icons/pause.svg"
                    } else {
                        "icons/play.svg"
                    },
                )
                .w(28.)
                .compact()
                .selected(on && paused)
                .tooltip(if !on {
                    "Start analysis"
                } else if paused {
                    "Resume search (P)"
                } else {
                    "Pause search and keep the current lines (P)"
                })
                .on_click(move |_, cx| {
                    // With the engines off, the same button starts them again.
                    pause_this.update(cx, |screen, cx| {
                        if on {
                            screen.toggle_pause(cx);
                        } else {
                            screen.toggle_engine(cx);
                        }
                    });
                }),
            )
            .child(
                btn("log-panel", "Log")
                    .compact()
                    .selected(self.log_panel_open)
                    .tooltip(ui::i18n::t("Show or hide engine output"))
                    .on_click(move |_, cx| {
                        log_this.update(cx, |screen, cx| {
                            screen.log_panel_open = !screen.log_panel_open;
                            cx.notify();
                        });
                    }),
            )
            .child(
                btn("online-panel", "Online")
                    .compact()
                    .selected(self.online.open)
                    .tooltip(
                        "Add the ChessTB Explorer tablebase and chessdb.cn to Moves from here. \
                         Sends each position you visit while on.",
                    )
                    .on_click(move |_, cx| {
                        online_this.update(cx, |screen, cx| screen.toggle_online(cx));
                    }),
            )
            .into_any_element();
        (toggles, settings)
    }

    /// Lines shown by the analysis engines: the first one's MultiPV.
    pub(crate) fn shown_lines(&self, cx: &gpui::App) -> Option<i64> {
        self.live
            .read(cx)
            .engines
            .iter()
            .find_map(|engine| spin_option(&engine.config, "MultiPV"))
            .map(|(value, ..)| value)
    }

    /// Steps MultiPV on every analysis engine that has it, saved in each engine's
    /// options, without restarting them.
    pub(crate) fn step_lines(&mut self, up: bool, cx: &mut Context<Self>) {
        self.set_lines(LinesTarget::Step(up), cx);
    }

    /// Sets MultiPV on every analysis engine that has it, each within its own range,
    /// saved in its options and applied without restarting it.
    pub(crate) fn set_lines(&mut self, target: LinesTarget, cx: &mut Context<Self>) {
        let Some(current) = self.shown_lines(cx) else {
            return;
        };
        let changes: Vec<(EngineId, i64)> = self
            .live
            .read(cx)
            .engines
            .iter()
            .filter_map(|engine| {
                let (value, min, max) = spin_option(&engine.config, "MultiPV")?;
                let default = match engine
                    .config
                    .info
                    .options
                    .iter()
                    .find(|option| option.name == "MultiPV")
                    .map(|option| &option.kind)
                {
                    Some(OptionKind::Spin { default, .. }) => *default,
                    _ => 1,
                };
                let next = lines_target(target, current, default, min, max);
                (next != value).then_some((engine.config.id, next))
            })
            .collect();
        for (id, value) in changes {
            self.live.update(cx, |live, cx| {
                live.set_option(id, "MultiPV", value.to_string(), cx);
            });
        }
        self.rebuild_rows(cx);
    }

    pub(super) fn set_spin(
        &mut self,
        id: EngineId,
        name: &str,
        value: i64,
        cx: &mut Context<Self>,
    ) {
        let name = name.to_owned();
        self.live.update(cx, |live, cx| {
            live.set_option(id, &name, value.to_string(), cx);
        });
        self.rebuild_rows(cx);
    }

    /// Turns an engine on or off for live analysis.
    pub(super) fn set_engine_analysis(&mut self, id: EngineId, on: bool, cx: &mut Context<Self>) {
        self.model.update(cx, |model, cx| {
            model.set_engine_use(id, |config| config.use_analysis = on, cx);
        });
    }

    pub(super) fn stepper(
        &self,
        config: &EngineConfig,
        name: &'static str,
        label: &'static str,
        unit: &'static str,
        cx: &mut Context<Self>,
    ) -> Div {
        let id = config.id;
        let spin = spin_option(config, name);
        let this = cx.entity();
        let step = |up: bool| {
            let this = this.clone();
            let enabled = spin
                .is_some_and(|(value, min, max)| step_value(name, value, up, min, max) != value);
            let handler = move |_: &mut gpui::Window, cx: &mut gpui::App| {
                let Some((value, min, max)) = spin else {
                    return;
                };
                let next = step_value(name, value, up, min, max);
                this.update(cx, |screen, cx| screen.set_spin(id, name, next, cx));
            };
            (enabled, handler)
        };
        let ((down_on, down), (up_on, up)) = (step(false), step(true));
        let edit_this = this.clone();
        let value_element = if let Some((edit_id, edit_name, input, _)) = &self.spin_edit
            && *edit_id == id
            && *edit_name == name
        {
            Input::new(input).w_full().into_any_element()
        } else {
            let text = spin.map_or_else(
                || "—".to_owned(),
                |(value, ..)| {
                    if name == "Hash" {
                        engine::format_hash(value)
                    } else {
                        format!("{value}{unit}")
                    }
                },
            );
            btn(SharedString::from(format!("{name}-value")), text)
                .w(72.)
                .h(24.)
                .pad(0.)
                .disabled(spin.is_none())
                .on_click(move |window, cx| {
                    let Some((value, min, max)) = spin else {
                        return;
                    };
                    edit_this.update(cx, |screen, cx| {
                        let input = cx.new(|cx| {
                            let mut state = InputState::new(window, cx);
                            state.set_value(value.to_string(), window, cx);
                            state
                        });
                        let subscription =
                            cx.subscribe_in(&input, window, move |screen, input, event, _, cx| {
                                if matches!(event, InputEvent::Blur | InputEvent::PressEnter { .. })
                                {
                                    if let Ok(value) = input.read(cx).value().trim().parse::<i64>()
                                    {
                                        screen.set_spin(id, name, value.clamp(min, max), cx);
                                    }
                                    screen.spin_edit = None;
                                    cx.notify();
                                }
                            });
                        use gpui::Focusable;
                        input.focus_handle(cx).focus(window, cx);
                        screen.spin_edit = Some((id, name, input, subscription));
                        cx.notify();
                    });
                })
                .into_any_element()
        };
        h_flex()
            .flex_none()
            .gap(px(4.))
            .child(
                div()
                    .mr(px(4.))
                    .text_as(12., T.muted)
                    .child(ui::i18n::t(label).to_owned()),
            )
            .child(
                ui::widgets::stepper(SharedString::from(format!("{name}-{id:?}")), "")
                    .mini()
                    .value_width(72.)
                    .value_element(value_element)
                    .on_down(down_on, down)
                    .on_up(up_on, up),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine::{EngineInfo, UciOption};

    fn config() -> EngineConfig {
        EngineConfig {
            info: EngineInfo {
                options: vec![UciOption {
                    name: "Hash".to_owned(),
                    kind: OptionKind::Spin {
                        default: 16,
                        min: 1,
                        max: 64,
                    },
                }],
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn spin_values_come_from_config_then_the_engine_default() {
        let mut config = config();
        assert_eq!(spin_option(&config, "Hash"), Some((16, 1, 64)));
        config.options.insert("Hash".to_owned(), "32".to_owned());
        assert_eq!(spin_option(&config, "Hash"), Some((32, 1, 64)));
        assert_eq!(spin_option(&config, "Threads"), None);
    }

    #[test]
    fn lines_buttons_stay_in_the_engine_range() {
        assert_eq!(lines_target(LinesTarget::Step(true), 3, 1, 1, 500), 4);
        assert_eq!(lines_target(LinesTarget::Default, 7, 1, 1, 500), 1);
        // Max stays at the cap, whatever the position, so every move keeps a line.
        assert_eq!(lines_target(LinesTarget::All, 1, 1, 1, 500), MAX_LINES);
        // An engine allowing fewer lines gets its own maximum.
        assert_eq!(lines_target(LinesTarget::All, 1, 1, 1, 8), 8);
        assert_eq!(lines_target(LinesTarget::Default, 4, 0, 1, 8), 1);
    }

    #[test]
    fn steppers_clamp_and_hash_doubles() {
        assert_eq!(step_value("Hash", 32, true, 1, 64), 64);
        assert_eq!(step_value("Hash", 64, true, 1, 64), 64);
        assert_eq!(step_value("Hash", 1, false, 1, 64), 1);
        assert_eq!(step_value("MultiPV", 3, true, 1, 500), 4);
        assert_eq!(step_value("MultiPV", 1, false, 1, 500), 1);
    }
}
