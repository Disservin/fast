//! The top bar: the logo, mode switch, breadcrumb or path, search, engine button and toasts.

use super::*;

impl Workspace {
    /// Runs an Analyse-only shortcut, ignored on other screens where its effect
    /// would be invisible.
    pub(super) fn in_analyse(
        &mut self,
        cx: &mut Context<Self>,
        action: impl FnOnce(&mut AnalyseScreen, &mut Context<AnalyseScreen>),
    ) {
        if self.mode == Mode::Analyse {
            self.analyse.update(cx, action);
        }
    }

    /// Refreshes local game suggestions without blocking typing or the board.
    pub(super) fn update_search(&mut self, cx: &mut Context<Self>) {
        let query = self.search.read(cx).value().trim().to_owned();
        self.search_task = None;
        self.search_results.clear();
        self.search_selected = None;
        self.search_open = !query.is_empty();
        self.search_pending = self.search_open;
        self.search_failed = false;
        if query.is_empty() {
            cx.notify();
            return;
        }
        let entries = self.model.read(cx).databases.clone();
        self.search_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(200))
                .await;
            let result = cx
                .background_spawn(async move {
                    let mut found = Vec::new();
                    let mut failed = false;
                    for entry in entries {
                        match db::Database::open(&entry.path)
                            .and_then(|db| db.search_games(&query, 8 - found.len()))
                        {
                            Ok(rows) => {
                                found.extend(rows.into_iter().map(|row| (entry.path.clone(), row)))
                            }
                            Err(_) => failed = true,
                        }
                        if found.len() == 8 {
                            break;
                        }
                    }
                    (found, failed)
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.search_results = result.0;
                this.search_failed = result.1;
                this.search_pending = false;
                cx.notify();
            });
        }));
        cx.notify();
    }

    /// Opens a suggested game after reading its moves in the background.
    pub(super) fn open_search_result(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some((path, row)) = self.search_results.get(index) else {
            return;
        };
        let (path, id) = (path.clone(), row.id);
        self.search_open = false;
        let task =
            cx.background_spawn(async move { db::Database::open(path).and_then(|db| db.game(id)) });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |this, window, cx| match result {
                Ok(Some(game)) => this.open_pgn(&game.to_pgn(), 0, window, cx),
                _ => this
                    .model
                    .update(cx, |model, cx| model.toast("Could not open game", cx)),
            });
        })
        .detach();
        cx.notify();
    }

    /// Runs the typed position or player search when Enter is pressed.
    pub(super) fn run_search(&mut self, query: &str, window: &mut Window, cx: &mut Context<Self>) {
        if query.is_empty() {
            return;
        }
        self.search_open = false;
        // A FEN opens in Analyse; anything else searches players in the database.
        if chess_core::GameTree::from_fen(query).is_ok() {
            let fen = query.to_owned();
            self.new_tab(Mode::Analyse, window, cx);
            self.analyse
                .update(cx, |analyse, cx| analyse.load_fen(&fen, window, cx));
        } else {
            let text = query.to_owned();
            self.database
                .update(cx, |database, cx| database.search_player(&text, window, cx));
            self.set_mode(Mode::Database, cx);
        }
        self.focus.focus(window, cx);
    }

    pub(super) fn submit_path(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let multiple = self
            .model
            .read(cx)
            .pending_path
            .as_ref()
            .is_some_and(|pending| pending.multiple);
        let text = if multiple {
            self.paths_input.read(cx).value().to_string()
        } else {
            self.path_input.read(cx).value().to_string()
        };
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        let pending = self.model.update(cx, |model, cx| {
            cx.notify();
            model.pending_path.take()
        });
        self.path_input
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.paths_input
            .update(cx, |input, cx| input.set_value("", window, cx));
        if let Some(pending) = pending {
            let paths = if multiple {
                text.lines()
                    .map(str::trim)
                    .filter(|line| !line.is_empty())
                    .map(std::path::PathBuf::from)
                    .collect()
            } else {
                vec![std::path::PathBuf::from(text)]
            };
            // File callbacks may update this workspace/window; run after its
            // current event handler has released both update guards.
            cx.defer(move |cx| (pending.on_paths)(paths, cx));
        }
    }

    /// Typed-path fallback, for desktops with no file dialog service (such as WSL).
    pub(super) fn path_bar(&self, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        let title = self.model.read(cx).pending_path.as_ref()?.title.clone();
        let multiple = self.model.read(cx).pending_path.as_ref()?.multiple;
        let this = cx.entity();
        let cancel = self.model.clone();
        Some(
            h_flex()
                .flex_none()
                .w_full()
                .gap(px(10.))
                .px(px(21.))
                .py(px(8.))
                .bg(rgb(T.surface_2))
                .border_b_1()
                .border_color(rgb(T.border))
                .child(div().flex_none().text_color(rgb(T.fg_2)).child(format!(
                    "{title}: {}",
                    if multiple {
                        "enter one file path per line"
                    } else {
                        "enter a path"
                    }
                )))
                .child(div().flex_1().child(if multiple {
                    gpui_component::input::Textarea::new(&self.paths_input)
                        .h(px(88.))
                        .into_any_element()
                } else {
                    crate::controls::text_input(&self.path_input).into_any_element()
                }))
                .child(
                    btn("path-ok", ui::i18n::t("Open"))
                        .primary()
                        .on_click(move |window, cx| {
                            this.update(cx, |this, cx| this.submit_path(window, cx));
                        }),
                )
                .child(
                    btn("path-cancel", ui::i18n::t("Cancel")).on_click(move |_, cx| {
                        cancel.update(cx, |model, cx| {
                            model.pending_path = None;
                            cx.notify();
                        });
                    }),
                ),
        )
    }

    pub(super) fn breadcrumb(&self, cx: &App) -> SharedString {
        match self.mode {
            Mode::Analyse => self.analyse.read(cx).breadcrumb(),
            Mode::Database => self.database.read(cx).breadcrumb(),
            Mode::Train => self.model.read(cx).repertoire.name.clone().into(),
            Mode::Play => self.play.read(cx).breadcrumb(),
            Mode::Settings => {
                format!("Settings › {}", self.settings.read(cx).page().title()).into()
            }
            Mode::Tournament => self.tournament.as_ref().map_or_else(
                || ui::i18n::t("Tournament").into(),
                |screen| screen.read(cx).breadcrumb(),
            ),
            Mode::TestPositions => self.test_positions.as_ref().map_or_else(
                || ui::i18n::t("Test positions").into(),
                |screen| screen.read(cx).breadcrumb(),
            ),
        }
    }

    pub(super) fn mode_button(
        &self,
        mode: Mode,
        label: &'static str,
        cx: &mut Context<Self>,
    ) -> Btn {
        let active = self.mode == mode;
        btn(("mode", mode as u32), label)
            .h(31.)
            .pad(14.)
            .text(14.)
            .kind(if active {
                BtnKind::Active
            } else {
                BtnKind::Ghost
            })
            .on_click({
                let this = cx.entity();
                move |_, cx| this.update(cx, |this, cx| this.set_mode(mode, cx))
            })
    }

    pub(super) fn logo() -> impl IntoElement {
        let square = |filled: bool| {
            div()
                .size(px(9.5))
                .when(filled, |sq| sq.bg(rgb(T.fg)))
                .when(!filled, |sq| sq.border_1().border_color(rgb(T.fg)))
        };
        v_flex()
            .gap(px(1.))
            .child(
                h_flex()
                    .gap(px(1.))
                    .child(square(true))
                    .child(square(false)),
            )
            .child(
                h_flex()
                    .gap(px(1.))
                    .child(square(false))
                    .child(square(true)),
            )
    }

    /// The engine button's text, when the current mode shows one.
    pub(super) fn engine_button_label(&self, cx: &App) -> Option<String> {
        (self.mode == Mode::Analyse).then(|| {
            let names: Vec<String> = self
                .model
                .read(cx)
                .analysis_engines()
                .iter()
                .map(|engine| engine.name.clone())
                .collect();
            if names.is_empty() {
                "No engine".to_owned()
            } else {
                names.join(" + ")
            }
        })
    }

    pub(super) fn engine_button(&self, cx: &mut Context<Self>) -> Option<Btn> {
        let model = self.model.read(cx);
        let engines = model.analysis_engines();
        let live_on = self.analyse.read(cx).live_on(cx);
        let names: Vec<String> = engines.iter().map(|engine| engine.name.clone()).collect();
        let ok = !engines.is_empty()
            && engines
                .iter()
                .all(|engine| model.engine_status.get(&engine.id) != Some(&EngineStatus::Error));
        let label = if names.is_empty() {
            "No engine".to_owned()
        } else {
            names.join(" + ")
        };
        let this = cx.entity();
        Some(
            btn("engines-status", label)
                .pad(14.)
                .text(13.)
                .dot(if ok && live_on { T.good } else { T.faint })
                .tooltip(if ok && live_on {
                    "Engines running. Press Space to pause; click for engine settings."
                } else {
                    "Engines stopped. Press Space to start; click for engine settings."
                })
                .on_click(move |window, cx| {
                    this.update(cx, |this, cx| {
                        this.settings.update(cx, |settings, cx| {
                            settings.set_page(SettingsPage::Engines, window, cx);
                        });
                        this.set_mode(Mode::Settings, cx);
                    });
                }),
        )
    }

    pub(super) fn right_controls(
        &self,
        viewport_w: f32,
        controls: bool,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let this = cx.entity();
        let mut row = h_flex().gap(px(10.));
        // The search box takes what is left beside the centered mode switch.
        let engine_w = self
            .engine_button_label(cx)
            .map_or(0., |label| 60. + label.len() as f32 * 7.);
        let others = match self.mode {
            Mode::Analyse => engine_w + 10.,
            _ => 0.,
        } + 48.
            + 10.
            + if controls {
                WINDOW_CONTROLS_W + 10.
            } else {
                0.
            };
        let free = viewport_w / 2. - MODE_SWITCH_HALF_W - 21. - others;
        let search_w = match self.mode {
            Mode::Analyse | Mode::Database if free >= 120. => Some(free.min(260.)),
            _ => None,
        };
        if let Some(width) = search_w {
            let shortcut = self.model.read(cx).config.shortcuts.get("search").cloned();
            let mut field = crate::controls::text_input(&self.search)
                .height(34.)
                .icon("icons/search.svg");
            // The shortcut only shows when the box has room for it beside the placeholder.
            if let Some(key) = shortcut.filter(|key| !key.is_empty() && width >= 220.) {
                field = field.hint(crate::screens::settings::shortcut_hint(&key));
            }
            let mut search_box =
                div()
                    .relative()
                    .w(px(width))
                    .child(field)
                    .capture_key_down(cx.listener(
                        |this, event: &gpui::KeyDownEvent, window, cx| {
                            if !this.search_open {
                                return;
                            }
                            match event.keystroke.key.as_str() {
                                "escape" => {
                                    this.search_open = false;
                                    this.focus.focus(window, cx);
                                }
                                "enter" if this.search_selected.is_some() => {
                                    this.open_search_result(this.search_selected.unwrap(), cx);
                                }
                                "down" if !this.search_results.is_empty() => {
                                    this.search_selected =
                                        Some(this.search_selected.map_or(0, |index| {
                                            (index + 1) % this.search_results.len()
                                        }));
                                }
                                "up" if !this.search_results.is_empty() => {
                                    this.search_selected = Some(
                                        this.search_selected
                                            .unwrap_or(0)
                                            .checked_sub(1)
                                            .unwrap_or(this.search_results.len() - 1),
                                    );
                                }
                                _ => return,
                            }
                            cx.stop_propagation();
                            cx.notify();
                        },
                    ));
            if self.search_open {
                let mut results = v_flex()
                    .absolute()
                    .top(px(38.))
                    .right_0()
                    .w(px(440.))
                    .rounded(px(6.))
                    .bg(rgb(T.surface))
                    .border_1()
                    .border_color(rgb(T.border))
                    .text_size(px(13.))
                    .text_color(rgb(T.fg))
                    .occlude()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                        this.search_open = false;
                        cx.notify();
                    }));
                let status = if self.search_pending {
                    "Searching…"
                } else if self.search_failed {
                    "Some databases could not be searched"
                } else if self.search_results.is_empty() {
                    "No matching games"
                } else {
                    "Matching games"
                };
                results = results.child(
                    div()
                        .px(px(16.))
                        .py(px(8.))
                        .text_color(rgb(T.muted))
                        .child(ui::i18n::t(status)),
                );
                for (index, (_, game)) in self.search_results.iter().enumerate() {
                    let target = this.clone();
                    results = results.child(
                        div().border_t_1().border_color(rgb(T.divider)).child(
                            btn(
                                ("search-result", index),
                                format!(
                                    "{} — {}  {}  {}",
                                    game.white, game.black, game.result, game.date
                                ),
                            )
                            .compact()
                            .kind(BtnKind::Ghost)
                            .mono()
                            .full()
                            .selected(self.search_selected == Some(index))
                            .on_click(move |_, cx| {
                                target.update(cx, |this, cx| this.open_search_result(index, cx));
                            }),
                        ),
                    );
                }
                search_box = search_box.child(deferred(results));
            }
            row = row.child(search_box);
        }
        if self.mode == Mode::Analyse
            && let Some(button) = self.engine_button(cx)
        {
            row = row.child(button);
        }
        let settings_active = self.mode == Mode::Settings;
        row.child(
            icon_btn("settings", "icons/settings.svg")
                .w(48.)
                .selected(settings_active)
                .tooltip(ui::i18n::t("Settings"))
                .on_click(move |_, cx| {
                    this.update(cx, |this, cx| {
                        let next = if this.mode == Mode::Settings {
                            Mode::Analyse
                        } else {
                            Mode::Settings
                        };
                        this.set_mode(next, cx);
                    });
                }),
        )
        .when(controls, |row| row.child(window_controls(window)))
        // Clicks here must not start a window drag.
        .id("top-bar-controls")
        .occlude()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
    }

    pub(super) fn top_bar(
        &self,
        viewport_w: f32,
        controls: bool,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let crumb = self.breadcrumb(cx);
        // The drawn menu bar takes the breadcrumb's room on narrow windows.
        let show_crumb = self.menu_bar.is_none() || viewport_w >= 1360.;
        // Interactive parts block the drag strip below them; on Windows that strip is
        // the system caption, which would otherwise swallow their clicks.
        let menu_bar = self.menu_bar.clone().map(|bar| {
            div()
                .id("menu-bar")
                .occlude()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(bar)
        });
        div()
            .relative()
            .h(px(58.))
            .flex_none()
            .w_full()
            .bg(rgb(T.surface))
            .border_b_1()
            .border_color(rgb(T.border))
            .when(controls || MAC_TITLE_BAR, |bar| bar.child(drag_region()))
            .child(
                h_flex()
                    .absolute()
                    .inset_0()
                    .px(px(21.))
                    // The traffic lights are hidden in full screen.
                    .when(MAC_TITLE_BAR && !window.is_fullscreen(), |row| {
                        row.pl(px(TRAFFIC_LIGHTS_AT.0 + TRAFFIC_LIGHTS_W))
                    })
                    .justify_between()
                    .child(
                        h_flex()
                            .gap(px(10.))
                            .child(Self::logo())
                            .child(
                                div()
                                    .font_family(SANS)
                                    .font_weight(FontWeight::BOLD)
                                    .text_size(px(15.))
                                    .text_color(rgb(T.fg))
                                    .child(ui::i18n::t("Fast")),
                            )
                            .children(menu_bar)
                            .when(show_crumb, |row| {
                                row.child(
                                    div()
                                        .text_size(px(13.))
                                        .text_color(rgb(T.muted))
                                        .child(SharedString::from(format!("/ {crumb}"))),
                                )
                            }),
                    )
                    .child(self.right_controls(viewport_w, controls, window, cx)),
            )
            .child(
                h_flex()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left_0()
                    .right_0()
                    .justify_center()
                    .child(
                        h_flex()
                            .id("mode-switch")
                            .occlude()
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .h(px(36.))
                            .px(px(2.))
                            .gap(px(2.))
                            .rounded(px(6.))
                            .bg(rgb(T.bg))
                            .child(self.mode_button(Mode::Analyse, ui::i18n::t("Analyse"), cx))
                            .child(self.mode_button(Mode::Database, ui::i18n::t("Database"), cx))
                            .child(self.mode_button(Mode::Train, ui::i18n::t("Train"), cx))
                            .child(self.mode_button(Mode::Play, ui::i18n::t("Play"), cx)),
                    ),
            )
    }

    pub(super) fn toasts(&self, cx: &App) -> impl IntoElement + use<> {
        let model = self.model.read(cx);
        let mut column = v_flex()
            .absolute()
            .bottom(px(20.))
            .right(px(24.))
            .gap(px(8.));
        for toast in &model.toasts {
            let action = toast.action.as_ref().map(|action| {
                let (run, id, model) = (action.run.clone(), toast.id, self.model.clone());
                btn(("toast-action", toast.id as usize), action.label.clone())
                    .compact()
                    .on_click(move |window, cx| {
                        run(window, cx);
                        model.update(cx, |model, cx| model.dismiss_toast(id, cx));
                    })
            });
            column = column.child(
                h_flex()
                    .max_w(px(420.))
                    .gap(px(10.))
                    .px(px(14.))
                    .py(px(10.))
                    .rounded(px(6.))
                    .bg(rgb(T.selected))
                    .border_1()
                    .border_color(rgb(T.border_strong))
                    .text_size(px(14.))
                    .text_color(rgb(T.fg))
                    .child(div().flex_1().min_w_0().child(toast.text.clone()))
                    .children(action),
            );
        }
        deferred(column)
    }
}
