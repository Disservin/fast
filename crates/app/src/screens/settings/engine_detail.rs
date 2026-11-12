//! One engine's page in Settings › Engines: its name, icon, UCI options and saved profiles.

use super::*;

/// Friendly hint and display name for well-known UCI options.
pub(super) fn option_label(name: &str) -> (String, Option<&'static str>) {
    match name {
        "Threads" => (
            ui::i18n::t("Threads").to_owned(),
            Some(ui::i18n::t("CPU cores to use")),
        ),
        "Hash" => (
            "Hash".to_owned(),
            Some(ui::i18n::t("Memory for the transposition table")),
        ),
        "MultiPV" => (
            "MultiPV".to_owned(),
            Some(ui::i18n::t("Lines shown in Moves from here")),
        ),
        "SyzygyPath" => (
            ui::i18n::t("Syzygy path").to_owned(),
            Some(ui::i18n::t("Endgame tablebases, 3–6 pieces")),
        ),
        "UCI_ShowWDL" => (
            ui::i18n::t("Show WDL").to_owned(),
            Some(ui::i18n::t("Win / draw / loss odds next to the eval")),
        ),
        "WeightsFile" => (
            ui::i18n::t("Weights file").to_owned(),
            Some(ui::i18n::t("Lc0 network file")),
        ),
        other => (other.to_owned(), None),
    }
}

/// Well-known options first, in a stable order.
pub(super) fn option_rank(name: &str) -> usize {
    [
        "Threads",
        "Hash",
        "MultiPV",
        "WeightsFile",
        "SyzygyPath",
        "UCI_ShowWDL",
    ]
    .iter()
    .position(|known| *known == name)
    .unwrap_or(usize::MAX)
}

/// What an Analyse tab's engines do while another tab is shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum LeaveAnalyse {
    Keep,
    Pause,
    Stop,
}

impl LeaveAnalyse {
    pub(super) fn of(config: &crate::config::Config) -> Self {
        if config.analyse_in_background {
            Self::Keep
        } else if config.pause_in_background {
            Self::Pause
        } else {
            Self::Stop
        }
    }
}

/// Options the app itself drives, so they are not offered as settings.
/// `UCI_EngineAbout` is the engine's description and is shown as text instead.
pub(super) fn option_is_hidden(name: &str) -> bool {
    matches!(
        name,
        "UCI_LimitStrength"
            | "UCI_Elo"
            | "UCI_AnalyseMode"
            | "UCI_Chess960"
            | "Ponder"
            | "UCI_EngineAbout"
    )
}

/// Inputs first, then switches, then buttons, as Arena groups them.
pub(super) fn option_group(kind: &OptionKind) -> usize {
    match kind {
        OptionKind::Spin { .. } | OptionKind::Combo { .. } | OptionKind::String { .. } => 0,
        OptionKind::Check { .. } => 1,
        OptionKind::Button => 2,
    }
}

/// Width of one cell in the engine options grid.
pub(super) const OPTION_CELL_W: f32 = 232.;

impl SettingsScreen {
    pub(super) fn add_text_option(
        &mut self,
        name: &str,
        value: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let input = cx.new(|cx| {
            let mut state = InputState::new(window, cx);
            state.set_value(value, window, cx);
            state
        });
        let option = name.to_owned();
        self._option_subscriptions.push(cx.subscribe_in(
            &input,
            window,
            move |this, state, event, _, cx| {
                if matches!(event, InputEvent::Blur | InputEvent::PressEnter { .. }) {
                    let value = state.read(cx).value().trim().to_owned();
                    this.set_engine_option(&option, value, cx);
                }
            },
        ));
        self.option_inputs.insert(name.to_owned(), input);
    }

    /// Persists an option and applies it to the engine if running.
    pub(super) fn set_engine_option(&mut self, name: &str, value: String, cx: &mut Context<Self>) {
        let Some(id) = self.selected_engine else {
            return;
        };
        self.model.update(cx, |model, cx| {
            let Some(config) = model.engine_mut(id) else {
                return;
            };
            if config.option_value(name).as_ref() == Some(&value) {
                return;
            }
            config.options.insert(name.to_owned(), value.clone());
            if let Some(handle) = model.manager.get(id) {
                handle.set_option(name, Some(value));
            }
            model.save_config(cx);
            cx.emit(ModelEvent::Engines);
        });
    }

    pub(super) fn rename_engine(&mut self, name: String, cx: &mut Context<Self>) {
        let Some(id) = self.selected_engine else {
            return;
        };
        self.model.update(cx, |model, cx| {
            let Some(config) = model.engine_mut(id) else {
                return;
            };
            if name.is_empty() || config.name == name {
                return;
            }
            config.name = name;
            model.save_config(cx);
            cx.emit(ModelEvent::Engines);
        });
    }

    /// A second entry for the same binary with its own options, such as Lc0 with
    /// another network.
    pub(super) fn duplicate_engine(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.selected_engine else {
            return;
        };
        let new_id = self.model.update(cx, |model, cx| {
            let source = model.engine(id)?.clone();
            let new_id = model.config.next_engine_id;
            model.config.next_engine_id += 1;
            let mut copy = source.duplicate(new_id, &model.config.engines);
            // Only one copy analyses at first, so the table does not double up.
            copy.use_analysis = false;
            copy.sync_enabled();
            let status = copy.idle_status();
            model.config.engines.push(copy);
            model.engine_status.insert(new_id, status);
            model.save_config(cx);
            cx.emit(ModelEvent::Engines);
            Some(new_id)
        });
        if new_id.is_some() {
            self.select_engine(new_id, window, cx);
        }
    }

    pub(super) fn choose_icon(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.selected_engine else {
            return;
        };
        let model = self.model.clone();
        pick_path(
            self.model.clone(),
            "Engine icon (PNG, SVG or JPEG)",
            None,
            cx,
            move |path, cx| {
                model.update(cx, |model, cx| {
                    if let Some(config) = model.engine_mut(id) {
                        config.icon = Some(path);
                        model.save_config(cx);
                    }
                });
            },
        );
    }

    pub(super) fn reset_icon(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.selected_engine else {
            return;
        };
        self.model.update(cx, |model, cx| {
            if let Some(config) = model.engine_mut(id) {
                config.icon = None;
                model.save_config(cx);
            }
        });
    }

    pub(super) fn save_profile(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.selected_engine else {
            return;
        };
        let name = self.profile_input.read(cx).value().trim().to_owned();
        if name.is_empty() {
            self.model.toast("Name the profile first.", cx);
            return;
        }
        self.model.update(cx, |model, cx| {
            if let Some(config) = model.engine_mut(id) {
                config.save_profile(&name);
                model.save_config(cx);
                model.toast(format!("Saved profile {name}."), cx);
            }
        });
        self.profile_input
            .update(cx, |input, cx| input.set_value("", window, cx));
    }

    /// Switches the engine to a saved set of options, applied to a running engine in place.
    pub(super) fn apply_profile(
        &mut self,
        name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(id) = self.selected_engine else {
            return;
        };
        self.model.update(cx, |model, cx| {
            let Some(config) = model.engine_mut(id) else {
                return;
            };
            let changed = config.apply_profile(name);
            if changed.is_empty() {
                return;
            }
            if let Some(handle) = model.manager.get(id) {
                for (option, value) in changed {
                    handle.set_option(option, value);
                }
            }
            model.save_config(cx);
            cx.emit(ModelEvent::Engines);
        });
        self.select_engine(Some(id), window, cx);
    }

    pub(super) fn delete_profile(&mut self, name: &str, cx: &mut Context<Self>) {
        let Some(id) = self.selected_engine else {
            return;
        };
        self.model.update(cx, |model, cx| {
            if let Some(config) = model.engine_mut(id) {
                config.delete_profile(name);
                model.save_config(cx);
            }
        });
    }

    pub(super) fn render_engine_detail(
        &mut self,
        config: &EngineConfig,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let this = cx.entity();
        let status = self.model.read(cx).engine_status.get(&config.id).copied();
        let body = v_flex()
            .child(detail_row(
                "Name",
                Some(ui::i18n::t("Shown in tables and menus")),
                div().w(px(320.)).child(text_input(&self.name_input)),
            ))
            .child(detail_row(
                "Executable",
                None,
                h_flex()
                    .w_full()
                    .gap(px(8.))
                    .child(div().flex_1().min_w_0().child(text_input(&self.path_input)))
                    .child(btn("browse-executable", ui::i18n::t("Browse…")).on_click(
                        this.listen(move |screen, _, cx| {
                            screen.choose_engine_path(None, false, cx)
                        }),
                    )),
            ))
            .child(self.render_profiles(config, cx))
            .child(detail_row(
                "Arrow colour",
                Some(ui::i18n::t(
                    "Overrides move-quality colours for this engine",
                )),
                h_flex().gap(px(6.)).children(
                    std::iter::once(None)
                        .chain(ui::board::ENGINE_ARROW_COLOURS.map(Some))
                        .enumerate()
                        .map(|(index, color)| {
                            btn(
                                ("engine-arrow-color", index),
                                if color.is_none() { "Auto" } else { "●" },
                            )
                            .selected(config.arrow_color == color)
                            .on_click(this.listen(move |screen, _, cx| {
                                screen.set_engine_use(|engine| engine.arrow_color = color, cx)
                            }))
                            .dot(color.unwrap_or(T.arrow))
                        }),
                ),
            ))
            .child(self.render_option_grid(config, cx));
        let about = config.info.about().map(str::to_owned);
        let debug = config.debug;
        let debug_row = h_flex()
            .gap(px(12.))
            .mb(px(4.))
            .child(
                check("engine-debug", debug, "Debug")
                    .text(13.)
                    .on_change(this.listen_with(move |screen, on, _, cx| screen.set_debug(on, cx))),
            )
            .child(div().text_as(12., T.muted).child(ui::i18n::t(
                "Sends debug on and shows the raw engine output in its own window",
            )))
            .when(debug, |row| {
                row.child(
                    btn("debug-window", "Open output window")
                        .compact()
                        .on_click(this.listen(move |screen, _, cx| screen.open_debug_window(cx))),
                )
            });
        panel()
            .p(px(21.))
            .child(
                h_flex()
                    .justify_between()
                    .mb(px(6.))
                    .child(self.render_engine_title(config, cx))
                    .child(self.render_engine_actions(config, status, cx)),
            )
            .children(about.map(|about| {
                div()
                    .mb(px(8.))
                    .text_size(px(13.))
                    .line_height(px(19.))
                    .text_color(rgb(T.fg_2))
                    .child(SharedString::from(about))
            }))
            .child(debug_row)
            .children(
                self.benchmark
                    .clone()
                    .map(|text| mono(text).mb(px(4.)).text_as(13., T.fg_2)),
            )
            .child(body)
            .child(self.render_engine_usage(config, cx))
    }

    /// The engine's icon, name and version; clicking the icon or the link under the name
    /// chooses or resets the icon.
    fn render_engine_title(&self, config: &EngineConfig, cx: &mut Context<Self>) -> Div {
        let this = cx.entity();
        let has_icon = config.icon.is_some();
        let name = h_flex()
            .gap(px(10.))
            .items_baseline()
            .child(
                div()
                    .text_size(px(20.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(config.name.clone()),
            )
            .child(mono(config.version.clone()).text_as(16., T.muted));
        let icon_link = div()
            .id("icon-choose")
            .cursor_pointer()
            .text_color(rgb(T.accent))
            .on_click(this.listen_event(move |screen, _, _, cx| {
                if has_icon {
                    screen.reset_icon(cx);
                } else {
                    screen.choose_icon(cx);
                }
            }))
            .child(if has_icon {
                "Use the default rook"
            } else {
                "Choose an icon"
            });
        h_flex()
            .gap(px(12.))
            .child(
                div()
                    .id("engine-icon")
                    .cursor_pointer()
                    .on_click(this.listen_event(move |screen, _, _, cx| screen.choose_icon(cx)))
                    .child(engine_icon(config, 40.)),
            )
            .child(
                v_flex()
                    .child(name)
                    .child(h_flex().gap(px(10.)).text_size(px(12.)).child(icon_link)),
            )
    }

    /// Restart (after an error), benchmark, update check, duplicate and remove.
    fn render_engine_actions(
        &self,
        config: &EngineConfig,
        status: Option<EngineStatus>,
        cx: &mut Context<Self>,
    ) -> Div {
        let this = cx.entity();
        let model = self.model.clone();
        let name = config.name.clone();
        let duplicate_tip = ui::i18n::t(
            "Another entry for this binary with its own options, e.g. Lc0 with another network",
        );
        h_flex()
            .gap(px(12.))
            .when(status == Some(EngineStatus::Error), |row| {
                row.child(
                    btn("restart-engine", "Restart")
                        .h(32.)
                        .on_click(this.listen(move |screen, _, cx| screen.restart_engine(cx))),
                )
            })
            .child(
                btn("benchmark", "Run benchmark")
                    .h(32.)
                    .on_click(this.listen(move |screen, _, cx| screen.run_benchmark(cx))),
            )
            .child(
                btn("check-update", "Check for update")
                    .h(32.)
                    .on_click(move |_, cx| {
                        model.toast(
                            format!(
                                "Compare {name} with the pinned versions under Download engine. \
                                 Existing installations are kept until you choose a new version."
                            ),
                            cx,
                        );
                    }),
            )
            .child(
                btn("duplicate-engine", "Duplicate")
                    .h(32.)
                    .tooltip(duplicate_tip)
                    .on_click(
                        this.listen(move |screen, window, cx| screen.duplicate_engine(window, cx)),
                    ),
            )
            .child(
                btn("remove-engine", ui::i18n::t("Remove")).h(32.).on_click(
                    this.listen(move |screen, window, cx| screen.remove_engine(window, cx)),
                ),
            )
    }

    /// Which jobs this engine takes: live analysis, batch review and play.
    fn render_engine_usage(&self, config: &EngineConfig, cx: &mut Context<Self>) -> Div {
        let this = cx.entity();
        let usage_check = |id: &'static str,
                           value: bool,
                           label: &'static str,
                           set: fn(&mut EngineConfig, bool)| {
            div().child(check(id, value, label).on_change(this.listen_with(
                move |screen, value, _, cx| {
                    screen.set_engine_use(|config| set(config, value), cx);
                },
            )))
        };
        h_flex()
            .mt(px(14.))
            .child(
                div()
                    .w(px(236.))
                    .flex_none()
                    .font_weight(FontWeight::MEDIUM)
                    .child(ui::i18n::t("Use this engine for")),
            )
            .child(
                h_flex()
                    .gap(px(28.))
                    .child(usage_check(
                        "use-analysis",
                        config.use_analysis,
                        "Live analysis",
                        |c, v| c.use_analysis = v,
                    ))
                    .child(usage_check(
                        "use-review",
                        config.use_review,
                        "Batch game review",
                        |c, v| c.use_review = v,
                    ))
                    .child(usage_check("use-play", config.use_play, "Play", |c, v| {
                        c.use_play = v
                    })),
            )
    }

    /// The engine's UCI options as a grid of compact cells, label above the control,
    /// so every option fits on screen without a long scrolling list.
    fn render_option_grid(&self, config: &EngineConfig, cx: &mut Context<Self>) -> Div {
        let this = cx.entity();
        let mut options: Vec<UciOption> = config
            .info
            .options
            .iter()
            .filter(|option| !option_is_hidden(&option.name))
            .cloned()
            .collect();
        options.sort_by_key(|option| (option_group(&option.kind), option_rank(&option.name)));
        // Arena-style: a grid of compact cells, label above the control, so every
        // option fits on screen without a long scrolling list.
        let mut grid = div()
            .flex()
            .flex_row()
            .flex_wrap()
            .gap_x(px(16.))
            .gap_y(px(12.))
            .py(px(14.));
        let cell = |label: String, detail: Option<String>, control: gpui::AnyElement| {
            v_flex()
                .w(px(OPTION_CELL_W))
                .gap(px(4.))
                .child(
                    h_flex()
                        .gap(px(6.))
                        .h(px(16.))
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .child(
                            div()
                                .text_size(px(12.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(rgb(T.fg_2))
                                .child(SharedString::from(label)),
                        )
                        .children(detail.map(|detail| {
                            div()
                                .text_as(11., T.faint)
                                .child(SharedString::from(detail))
                        })),
                )
                .child(control)
        };
        for option in options {
            let (label, hint) = option_label(&option.name);
            let element: gpui::AnyElement = match &option.kind {
                OptionKind::Spin { min, max, .. } => {
                    let Some(input) = self.option_inputs.get(&option.name) else {
                        continue;
                    };
                    let suffix = (option.name == "Hash")
                        .then(|| crate::controls::hash_suffix(&input.read(cx).value()));
                    let range =
                        (*min > i64::MIN && *max < i64::MAX).then(|| format!("{min}–{max}"));
                    cell(
                        label,
                        hint.map(str::to_owned).or(range),
                        text_input(input)
                            .height(30.)
                            .suffix(suffix)
                            .into_any_element(),
                    )
                    .into_any_element()
                }
                OptionKind::String { .. } => {
                    let Some(input) = self.option_inputs.get(&option.name) else {
                        continue;
                    };
                    cell(label, hint.map(str::to_owned), {
                        let name = option.name.clone();
                        let directory = matches!(
                            name.as_str(),
                            "SyzygyPath" | "NalimovPath" | "GaviotaTbPath" | "TablebasePath"
                        );
                        let can_browse = directory
                            || matches!(
                                name.as_str(),
                                "WeightsFile"
                                    | "EvalFile"
                                    | "EvalFileSmall"
                                    | "BookFile"
                                    | "BookFile2"
                                    | "BookFileName"
                            );
                        h_flex()
                            .gap(px(6.))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .child(text_input(input).height(30.)),
                            )
                            .when(can_browse, |row| {
                                row.child(
                                    btn(SharedString::from(format!("browse-{name}")), "Browse…")
                                        .h(30.)
                                        .pad(6.)
                                        .on_click(this.listen(move |screen, _, cx| {
                                            screen.choose_engine_path(
                                                Some(name.clone()),
                                                directory,
                                                cx,
                                            )
                                        })),
                                )
                            })
                            .into_any_element()
                    })
                    .into_any_element()
                }
                OptionKind::Combo { choices, .. } => {
                    let Some(select) = self.option_selects.get(&option.name) else {
                        continue;
                    };
                    cell(
                        label,
                        Some(format!("{} choices", choices.len())),
                        div().w_full().child(select_el(select)).into_any_element(),
                    )
                    .into_any_element()
                }
                OptionKind::Check { default } => {
                    let current = config
                        .options
                        .get(&option.name)
                        .map_or(*default, |value| value == "true");
                    let name = option.name.clone();
                    let this = this.clone();
                    // Label beside the box, like Arena's check options.
                    div()
                        .w(px(OPTION_CELL_W))
                        .h(px(30.))
                        .flex()
                        .items_center()
                        .child(
                            check(
                                SharedString::from(format!("toggle-{}", option.name)),
                                current,
                                label,
                            )
                            .text(13.)
                            .on_change(move |value, _, cx| {
                                let value = value.to_string();
                                this.update(cx, |screen, cx| {
                                    screen.set_engine_option(&name, value, cx)
                                });
                            }),
                        )
                        .into_any_element()
                }
                OptionKind::Button => {
                    let name = option.name.clone();
                    let model = self.model.clone();
                    let id = config.id;
                    div()
                        .w(px(OPTION_CELL_W))
                        .child(
                            btn(
                                SharedString::from(format!("button-{}", option.name)),
                                option.name.clone(),
                            )
                            .h(30.)
                            .full()
                            .on_click(move |_, cx| {
                                if let Some(handle) = model.read(cx).manager.get(id) {
                                    handle.set_option(name.clone(), None);
                                }
                            }),
                        )
                        .into_any_element()
                }
            };
            grid = grid.child(element);
        }
        grid
    }

    /// Saved option sets: click one to switch to it, or save the current options.
    pub(super) fn render_profiles(&self, config: &EngineConfig, cx: &mut Context<Self>) -> Div {
        let this = cx.entity();
        let matching = config.matching_profile().map(str::to_owned);
        let mut chips = h_flex().gap(px(6.)).flex_wrap();
        for (index, profile) in config.profiles.iter().enumerate() {
            let name = profile.name.clone();
            let active = matching.as_deref() == Some(name.as_str());
            let delete_name = name.clone();
            chips = chips.child(
                h_flex()
                    .gap(px(2.))
                    .child(
                        btn(("profile", index), name.clone())
                            .h(28.)
                            .pad(10.)
                            .text(13.)
                            .selected(active)
                            .tooltip(format!("{} options", profile.options.len()))
                            .on_click(this.listen(move |screen, window, cx| {
                                screen.apply_profile(&name, window, cx);
                            })),
                    )
                    .child(icon_x(("profile-delete", index)).on_click(
                        this.listen(move |screen, _, cx| screen.delete_profile(&delete_name, cx)),
                    )),
            );
        }
        if config.profiles.is_empty() {
            chips = chips.child(div().text_as(13., T.muted).child(ui::i18n::t("None yet")));
        }
        let save_this = this;
        detail_row(
            "Profiles",
            Some(ui::i18n::t("Preset option sets for this engine")),
            h_flex().gap(px(12.)).flex_wrap().child(chips).child(
                h_flex()
                    .gap(px(6.))
                    .child(
                        div()
                            .w(px(180.))
                            .child(text_input(&self.profile_input).height(30.)),
                    )
                    .child(
                        btn("profile-save", "Save current")
                            .h(30.)
                            .pad(10.)
                            .on_click(
                                save_this.listen(move |screen, window, cx| {
                                    screen.save_profile(window, cx)
                                }),
                            ),
                    ),
            ),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_group_inputs_before_switches_and_buttons() {
        assert!(option_is_hidden("UCI_EngineAbout"));
        assert!(
            option_group(&OptionKind::Combo {
                default: String::new(),
                choices: Vec::new()
            }) < option_group(&OptionKind::Check { default: false })
        );
        assert!(
            option_group(&OptionKind::Check { default: true }) < option_group(&OptionKind::Button)
        );
    }

    #[test]
    fn known_options_sort_first() {
        assert!(option_rank("Threads") < option_rank("Hash"));
        assert!(option_rank("UCI_ShowWDL") < option_rank("Contempt"));
        assert!(option_is_hidden("UCI_Elo"));
        assert_eq!(option_label("SyzygyPath").0, "Syzygy path");
    }
}
