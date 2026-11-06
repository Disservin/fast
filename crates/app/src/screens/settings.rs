//! Settings: General, Board & pieces, Engines, Databases, Keyboard shortcuts.

use std::{collections::HashMap, path::PathBuf, process::Command};

use chess_core::Notation;
use engine::{
    EngineConfig, EngineEvent, EngineHandle, EngineId, EngineStatus, Limits, OptionKind,
    SearchRequest, UciOption,
};
use gpui::{
    AppContext as _, Context, Div, Entity, FocusHandle, FontWeight, IntoElement, KeyDownEvent,
    Render, SharedString, Subscription, Window, div, img, prelude::*, px, rgb, svg,
};
use gpui_component::{
    input::{InputEvent, InputState},
    slider::{Slider, SliderEvent, SliderState},
};
use ui::{
    MONO, T,
    board::theme_swatch,
    widgets::{btn, check, h_flex, key_cap, mono, panel, section_label, segmented, v_flex},
};

use crate::{
    config::{SavedCoordinates, SavedPieces, SavedTheme},
    controls::{TextSelect, new_input, new_select, select_el, text_input},
    model::{AppModel, ModelEvent, pick_path, split_engine_name},
    screens::database::format_count,
    shell,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsPage {
    General,
    Board,
    Engines,
    Databases,
    Shortcuts,
    Accounts,
}

impl SettingsPage {
    pub const ALL: [Self; 6] = [
        Self::General,
        Self::Board,
        Self::Engines,
        Self::Databases,
        Self::Shortcuts,
        Self::Accounts,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Board => "Board & pieces",
            Self::Engines => "Engines",
            Self::Databases => "Databases",
            Self::Shortcuts => "Keyboard shortcuts",
            Self::Accounts => "Accounts",
        }
    }
}

/// Human labels for the rebindable actions, in display order.
const SHORTCUT_ROWS: [(&str, &str); 8] = [
    ("prev_move", "Previous move"),
    ("next_move", "Next move"),
    ("first_move", "Start of game"),
    ("last_move", "End of game"),
    ("toggle_engine", "Engine on / off"),
    ("flip_board", "Flip board"),
    ("search", "Search"),
    ("save_game", "Save game"),
];

/// Friendly hint and display name for well-known UCI options.
fn option_label(name: &str) -> (String, Option<&'static str>) {
    match name {
        "Threads" => ("Threads".to_owned(), Some("CPU cores to use")),
        "Hash" => (
            "Hash".to_owned(),
            Some("Memory for the transposition table"),
        ),
        "MultiPV" => ("MultiPV".to_owned(), Some("Lines shown in Moves from here")),
        "SyzygyPath" => (
            "Syzygy path".to_owned(),
            Some("Endgame tablebases, 3–6 pieces"),
        ),
        "UCI_ShowWDL" => (
            "Show WDL".to_owned(),
            Some("Win / draw / loss odds next to the eval"),
        ),
        "WeightsFile" => ("Weights file".to_owned(), Some("Lc0 network file")),
        other => (other.to_owned(), None),
    }
}

/// Well-known options first, in a stable order.
fn option_rank(name: &str) -> usize {
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

/// Options the app itself drives, so they are not offered as settings.
fn option_is_hidden(name: &str) -> bool {
    matches!(
        name,
        "UCI_LimitStrength" | "UCI_Elo" | "UCI_AnalyseMode" | "UCI_Chess960" | "Ponder"
    )
}

pub struct SettingsScreen {
    model: Entity<AppModel>,
    page: SettingsPage,
    selected_engine: Option<EngineId>,
    path_input: Entity<InputState>,
    option_inputs: HashMap<String, Entity<InputState>>,
    option_selects: HashMap<String, TextSelect>,
    language: TextSelect,
    animation: Entity<SliderState>,
    player_name: Entity<InputState>,
    capture_focus: FocusHandle,
    capturing: Option<&'static str>,
    benchmark: Option<SharedString>,
    _subscriptions: Vec<Subscription>,
    _option_subscriptions: Vec<Subscription>,
}

impl SettingsScreen {
    pub fn new(model: Entity<AppModel>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let language = new_select(vec!["English".into()], 0, window, cx);
        let current_ms = model.read(cx).config.board.animation_ms;
        let animation = cx.new(|_| {
            SliderState::new()
                .min(0.)
                .max(400.)
                .step(10.)
                .default_value(current_ms as f32)
        });
        let name = model.read(cx).config.player_name.clone();
        let player_name = new_input(name, "Your name in game headers", window, cx);
        let path_input = cx.new(|cx| InputState::new(window, cx));
        let subscriptions = vec![
            cx.subscribe(&animation, |this, _, event: &SliderEvent, cx| {
                if let SliderEvent::Change(value) = event {
                    let ms = value.start().round() as u32;
                    this.update_board(cx, |board| board.animation_ms = ms);
                }
            }),
            cx.subscribe_in(&player_name, window, |this, state, event, _, cx| {
                if matches!(event, InputEvent::Blur | InputEvent::PressEnter { .. }) {
                    let name = state.read(cx).value().trim().to_owned();
                    this.model.update(cx, |model, cx| {
                        if model.config.player_name != name && !name.is_empty() {
                            model.config.player_name = name;
                            model.save_config(cx);
                            model.refresh_leaks();
                            cx.emit(ModelEvent::Databases);
                        }
                    });
                }
            }),
            cx.observe(&model, |_, _, cx| cx.notify()),
        ];
        let selected_engine = model
            .read(cx)
            .config
            .engines
            .first()
            .map(|engine| engine.id);
        let mut this = Self {
            model,
            page: SettingsPage::General,
            selected_engine: None,
            path_input,
            option_inputs: HashMap::new(),
            option_selects: HashMap::new(),
            language,
            animation,
            player_name,
            capture_focus: cx.focus_handle(),
            capturing: None,
            benchmark: None,
            _subscriptions: subscriptions,
            _option_subscriptions: Vec::new(),
        };
        this.select_engine(selected_engine, window, cx);
        this
    }

    pub fn page(&self) -> SettingsPage {
        self.page
    }

    pub fn set_page(&mut self, page: SettingsPage, _window: &mut Window, cx: &mut Context<Self>) {
        self.page = page;
        self.capturing = None;
        cx.notify();
    }

    fn update_board(
        &self,
        cx: &mut Context<Self>,
        change: impl FnOnce(&mut crate::config::BoardConfig),
    ) {
        self.model.update(cx, |model, cx| {
            change(&mut model.config.board);
            model.save_config(cx);
            cx.emit(ModelEvent::BoardPrefs);
        });
    }

    /// Builds the input states for an engine's options from its stored schema.
    fn select_engine(&mut self, id: Option<EngineId>, window: &mut Window, cx: &mut Context<Self>) {
        self.selected_engine = id;
        self.benchmark = None;
        self.option_inputs.clear();
        self.option_selects.clear();
        self._option_subscriptions.clear();
        let Some(config) = id.and_then(|id| self.model.read(cx).engine(id).cloned()) else {
            cx.notify();
            return;
        };
        self.path_input.update(cx, |input, cx| {
            input.set_value(config.path.display().to_string(), window, cx);
        });
        for option in &config.info.options {
            let current = config.options.get(&option.name).cloned();
            match &option.kind {
                OptionKind::Spin { default, .. } => {
                    self.add_text_option(
                        &option.name,
                        current.unwrap_or(default.to_string()),
                        window,
                        cx,
                    );
                }
                OptionKind::String { default } => {
                    self.add_text_option(
                        &option.name,
                        current.unwrap_or(default.clone()),
                        window,
                        cx,
                    );
                }
                OptionKind::Combo { default, choices } => {
                    let value = current.unwrap_or(default.clone());
                    let index = choices
                        .iter()
                        .position(|choice| *choice == value)
                        .unwrap_or(0);
                    let items: Vec<SharedString> =
                        choices.iter().map(|choice| choice.clone().into()).collect();
                    let select = new_select(items, index, window, cx);
                    let name = option.name.clone();
                    let choices = choices.clone();
                    self._option_subscriptions.push(cx.subscribe(
                        &select,
                        move |this, state, _: &gpui_component::select::SelectEvent<_>, cx| {
                            let index = state
                                .read(cx)
                                .selected_index(cx)
                                .map_or(0, |index| index.row);
                            if let Some(value) = choices.get(index) {
                                this.set_engine_option(&name, value.clone(), cx);
                            }
                        },
                    ));
                    self.option_selects.insert(option.name.clone(), select);
                }
                OptionKind::Check { .. } | OptionKind::Button => {}
            }
        }
        cx.notify();
    }

    fn add_text_option(
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
    fn set_engine_option(&mut self, name: &str, value: String, cx: &mut Context<Self>) {
        let Some(id) = self.selected_engine else {
            return;
        };
        self.model.update(cx, |model, cx| {
            let Some(config) = model.engine_mut(id) else {
                return;
            };
            if config.options.get(name) == Some(&value) {
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

    fn set_engine_use(&mut self, change: impl FnOnce(&mut EngineConfig), cx: &mut Context<Self>) {
        let Some(id) = self.selected_engine else {
            return;
        };
        self.model.update(cx, |model, cx| {
            let Some(config) = model.engine_mut(id) else {
                return;
            };
            change(config);
            config.enabled = config.use_analysis || config.use_review || config.use_play;
            let status = if config.enabled {
                EngineStatus::Ready
            } else {
                EngineStatus::Disabled
            };
            model.engine_status.insert(id, status);
            model.save_config(cx);
            cx.emit(ModelEvent::Engines);
        });
    }

    fn add_from_file(&mut self, cx: &mut Context<Self>) {
        let this = cx.entity().downgrade();
        pick_path(
            self.model.clone(),
            "Add engine",
            None,
            cx,
            move |path, cx| {
                let _ = this.update(cx, |this, cx| this.register_engine(path, cx));
            },
        );
    }

    fn register_engine(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let handle = EngineHandle::spawn(path.clone(), Vec::new(), Vec::new());
        let events = handle.events();
        let this = cx.entity().downgrade();
        let model = self.model.clone();
        cx.spawn(async move |_, cx| {
            let result = smol::future::or(
                async {
                    loop {
                        match events.recv().await {
                            Ok(EngineEvent::Ready(info)) => return Ok(info),
                            Ok(EngineEvent::Crashed(reason)) => return Err(reason),
                            Ok(_) => {}
                            Err(_) => return Err("the engine closed unexpectedly".to_owned()),
                        }
                    }
                },
                async {
                    smol::Timer::after(std::time::Duration::from_secs(10)).await;
                    Err("no reply to the uci handshake within 10 seconds".to_owned())
                },
            )
            .await;
            handle.quit();
            let added = model.update(cx, |model, cx| match result {
                Ok(info) => {
                    let id = model.config.next_engine_id;
                    model.config.next_engine_id += 1;
                    let fallback = path
                        .file_stem()
                        .map(|stem| stem.to_string_lossy().into_owned())
                        .unwrap_or_else(|| "Engine".to_owned());
                    let (name, version) = split_engine_name(&info.name, &fallback);
                    let wdl = info
                        .options
                        .iter()
                        .any(|option| option.name == "UCI_ShowWDL");
                    let mut options = std::collections::BTreeMap::new();
                    if wdl {
                        options.insert("UCI_ShowWDL".to_owned(), "true".to_owned());
                    }
                    if info.options.iter().any(|option| option.name == "MultiPV") {
                        options.insert("MultiPV".to_owned(), "3".to_owned());
                    }
                    model.config.engines.push(EngineConfig {
                        id,
                        name,
                        version,
                        path: path.clone(),
                        runs_on_gpu: false,
                        enabled: true,
                        options,
                        use_analysis: true,
                        use_review: true,
                        use_play: true,
                        info,
                    });
                    model.engine_status.insert(id, EngineStatus::Ready);
                    model.save_config(cx);
                    cx.emit(ModelEvent::Engines);
                    Some(id)
                }
                Err(reason) => {
                    model.toast(format!("Could not add {}: {reason}", path.display()), cx);
                    None
                }
            });
            if let (Some(id), Some(this)) = (added, this.upgrade()) {
                cx.update(|cx| {
                    if let Some(window) = cx.active_window() {
                        let _ = window.update(cx, |_, window, cx| {
                            this.update(cx, |this, cx| {
                                this.select_engine(Some(id), window, cx);
                            });
                        });
                    }
                });
            }
        })
        .detach();
    }

    fn remove_engine(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.selected_engine else {
            return;
        };
        self.model.update(cx, |model, cx| {
            model.manager.shutdown(id);
            model.config.engines.retain(|engine| engine.id != id);
            model.engine_status.remove(&id);
            model.save_config(cx);
            cx.emit(ModelEvent::Engines);
        });
        let next = self
            .model
            .read(cx)
            .config
            .engines
            .first()
            .map(|engine| engine.id);
        self.select_engine(next, window, cx);
    }

    fn run_benchmark(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.selected_engine else {
            return;
        };
        let Some(config) = self.model.read(cx).engine(id).cloned() else {
            return;
        };
        if self.model.read(cx).demo {
            self.benchmark =
                Some("Benchmark needs a real engine; demo mode only has scripted lines.".into());
            cx.notify();
            return;
        }
        self.benchmark = Some("Running benchmark…".into());
        cx.notify();
        let task = cx.background_spawn(async move { benchmark(&config) });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.benchmark = Some(result.into());
                cx.notify();
            });
        })
        .detach();
    }

    fn restart_engine(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.selected_engine else {
            return;
        };
        self.model.update(cx, |model, cx| {
            model.manager.shutdown(id);
            model.engine_status.insert(id, EngineStatus::Ready);
            cx.emit(ModelEvent::Engines);
            cx.notify();
        });
    }

    fn on_capture_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let Some(action) = self.capturing else { return };
        let keystroke = &event.keystroke;
        if keystroke.key == "escape" {
            self.capturing = None;
            cx.notify();
            return;
        }
        // Modifiers alone do not make a shortcut.
        if matches!(
            keystroke.key.as_str(),
            "shift" | "control" | "alt" | "platform" | "function"
        ) {
            return;
        }
        let mut text = String::new();
        let modifiers = keystroke.modifiers;
        for (active, name) in [
            (modifiers.control, "ctrl-"),
            (modifiers.alt, "alt-"),
            (modifiers.shift, "shift-"),
            (modifiers.platform, "cmd-"),
        ] {
            if active {
                text.push_str(name);
            }
        }
        text.push_str(&keystroke.key);
        cx.stop_propagation();
        let model = self.model.clone();
        model.update(cx, |model, cx| {
            if let Some(old) = model.config.shortcuts.get(action).cloned() {
                shell::unbind_shortcut(&old, action, cx);
            }
            model
                .config
                .shortcuts
                .insert(action.to_owned(), text.clone());
            let single: std::collections::BTreeMap<String, String> =
                [(action.to_owned(), text)].into_iter().collect();
            shell::bind_shortcuts(&single, cx);
            model.save_config(cx);
        });
        self.capturing = None;
        cx.notify();
    }

    fn render_nav(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let this = cx.entity();
        let mut nav = v_flex().gap(px(2.));
        for (index, page) in SettingsPage::ALL.into_iter().enumerate() {
            let selected = self.page == page;
            let this = this.clone();
            nav = nav.child(
                h_flex()
                    .id(("settings-nav", index))
                    .relative()
                    .h(px(36.))
                    .px(px(12.))
                    .rounded(px(4.))
                    .cursor_pointer()
                    .overflow_hidden()
                    .text_size(px(15.))
                    .text_color(rgb(if selected { T.fg } else { T.fg_2 }))
                    .when(selected, |item| {
                        item.bg(rgb(T.selected))
                            .border_1()
                            .border_color(rgb(T.border_strong))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(
                                div()
                                    .absolute()
                                    .left_0()
                                    .top_0()
                                    .bottom_0()
                                    .w(px(3.))
                                    .bg(rgb(T.fg)),
                            )
                    })
                    .when(!selected, |item| item.hover(|style| style.bg(rgb(T.hover))))
                    .on_click(move |_, window, cx| {
                        this.update(cx, |screen, cx| screen.set_page(page, window, cx));
                    })
                    .child(page.title()),
            );
        }
        v_flex()
            .w(px(260.))
            .h_full()
            .flex_none()
            .px(px(12.))
            .pt(px(20.))
            .bg(rgb(T.surface))
            .border_r_1()
            .border_color(rgb(T.border))
            .child(
                div()
                    .px(px(12.))
                    .mb(px(14.))
                    .child(section_label("Settings")),
            )
            .child(nav)
    }

    fn render_appearance(&mut self) -> Div {
        section("Appearance")
            .child(option_row(
                "Theme",
                None,
                h_flex()
                    .gap(px(6.))
                    .child(btn("theme-dark", "Dark").pad(10.).h(30.).selected(true))
                    .child(
                        btn("theme-light", "Light")
                            .pad(10.)
                            .h(30.)
                            .disabled(true)
                            .tooltip("Light theme is not available yet"),
                    )
                    .child(
                        btn("theme-system", "Match system")
                            .pad(10.)
                            .h(30.)
                            .disabled(true)
                            .tooltip("Light theme is not available yet"),
                    ),
            ))
            .child(option_row(
                "Language",
                None,
                div().w(px(190.)).child(select_el(&self.language)),
            ))
    }

    fn render_board_section(&mut self, cx: &mut Context<Self>) -> Div {
        let this = cx.entity();
        let config = self.model.read(cx).config.board.clone();
        let mut boards = h_flex().gap(px(8.));
        let themes = [
            (SavedTheme::Stone, ui::BoardTheme::Stone),
            (SavedTheme::Walnut, ui::BoardTheme::Walnut),
            (SavedTheme::Moss, ui::BoardTheme::Moss),
            (SavedTheme::Graphite, ui::BoardTheme::Graphite),
        ];
        for (index, (saved, theme)) in themes.into_iter().enumerate() {
            let selected = config.theme == saved;
            let this = this.clone();
            boards = boards.child(
                tile(("board-theme", index), selected)
                    .on_click(move |_, _, cx| {
                        this.update(cx, |screen, cx| {
                            screen.update_board(cx, |board| board.theme = saved);
                        });
                    })
                    .child(theme_swatch(theme, px(56.), cx))
                    .child(tile_label(theme.name())),
            );
        }
        let mut pieces = h_flex().gap(px(8.));
        let sets: [(SavedPieces, &str, gpui::AnyElement); 3] = [
            (
                SavedPieces::Classic,
                "Classic",
                img("pieces/merida/bN.svg").size(px(44.)).into_any_element(),
            ),
            (
                SavedPieces::Letters,
                "Letters",
                div()
                    .text_size(px(32.))
                    .font_weight(FontWeight::BOLD)
                    .text_color(rgb(0x14161A))
                    .child("N")
                    .into_any_element(),
            ),
            (
                SavedPieces::Flat,
                "Flat",
                svg()
                    .path("pieces/merida/bN.svg")
                    .size(px(34.))
                    .text_color(rgb(0x14161A))
                    .into_any_element(),
            ),
        ];
        for (index, (saved, name, preview)) in sets.into_iter().enumerate() {
            let selected = config.pieces == saved;
            let this = this.clone();
            pieces = pieces.child(
                tile(("piece-set", index), selected)
                    .on_click(move |_, _, cx| {
                        this.update(cx, |screen, cx| {
                            screen.update_board(cx, |board| board.pieces = saved);
                        });
                    })
                    .child(
                        div()
                            .size(px(56.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(2.))
                            .bg(rgb(0xDCD8D0))
                            .child(preview),
                    )
                    .child(tile_label(name)),
            );
        }
        let coords_this = this.clone();
        let notation_this = this.clone();
        let check_row = h_flex()
            .gap(px(24.))
            .child(board_check(
                "legal",
                config.show_legal,
                "Show legal moves",
                this.clone(),
                |b, v| b.show_legal = v,
            ))
            .child(board_check(
                "last",
                config.highlight_last,
                "Highlight last move",
                this.clone(),
                |b, v| b.highlight_last = v,
            ))
            .child(board_check(
                "queen",
                config.auto_queen,
                "Auto-promote to queen",
                this.clone(),
                |b, v| b.auto_queen = v,
            ))
            .child(board_check(
                "sounds",
                config.move_sounds,
                "Move sounds",
                this.clone(),
                |b, v| b.move_sounds = v,
            ));
        section("Board & pieces")
            .child(option_row("Board", None, boards))
            .child(option_row("Pieces", None, pieces))
            .child(option_row(
                "Coordinates",
                None,
                segmented(
                    "coords",
                    vec![
                        (SavedCoordinates::OnSquares, "On squares".into(), false),
                        (SavedCoordinates::Outside, "Outside".into(), false),
                        (SavedCoordinates::Off, "Off".into(), false),
                    ],
                    config.coordinates,
                    move |value, _, cx| {
                        coords_this.update(cx, |screen, cx| {
                            screen.update_board(cx, |board| board.coordinates = value);
                        });
                    },
                ),
            ))
            .child(option_row(
                "Move animation",
                None,
                h_flex()
                    .gap(px(10.))
                    .child(
                        div()
                            .w(px(160.))
                            .child(Slider::new(&self.animation).bg(rgb(T.fg))),
                    )
                    .child(
                        mono(format!("{} ms", config.animation_ms))
                            .text_size(px(13.))
                            .text_color(rgb(T.muted)),
                    ),
            ))
            .child(option_row("Notation", None, {
                let mut row = h_flex().gap(px(6.));
                for (index, (value, label)) in [
                    (Notation::San, "Nf3"),
                    (Notation::Figurine, "♘f3"),
                    (Notation::Long, "g1-f3"),
                ]
                .into_iter()
                .enumerate()
                {
                    let this = notation_this.clone();
                    row = row.child(
                        btn(("notation", index), label)
                            .mono()
                            .pad(10.)
                            .selected(config.notation == value)
                            .on_click(move |_, cx| {
                                this.update(cx, |screen, cx| {
                                    screen.update_board(cx, |board| board.notation = value);
                                });
                            }),
                    );
                }
                row
            }))
            .child(option_row("Board behaviour", None, check_row))
    }

    fn render_shortcut_summary(&mut self, cx: &mut Context<Self>) -> Div {
        let this = cx.entity();
        let model = self.model.read(cx);
        let keys = |action: &str| -> Div { keycaps(model.config.shortcuts.get(action)) };
        let row = |left: &'static str, a: Div, right: &'static str, b: Div| {
            h_flex()
                .h(px(32.))
                .border_b_1()
                .border_color(rgb(T.divider))
                .child(
                    div()
                        .w(px(232.))
                        .flex_none()
                        .font_weight(FontWeight::MEDIUM)
                        .child(left),
                )
                .child(div().w(px(352.)).flex_none().child(a))
                .child(
                    div()
                        .w(px(232.))
                        .flex_none()
                        .font_weight(FontWeight::MEDIUM)
                        .child(right),
                )
                .child(b)
        };
        let pair =
            |first: &str, second: &str| h_flex().gap(px(4.)).child(keys(first)).child(keys(second));
        section("Keyboard shortcuts")
            .child(row(
                "Previous / next move",
                pair("prev_move", "next_move"),
                "Start / end of game",
                pair("first_move", "last_move"),
            ))
            .child(row(
                "Engine on / off",
                keys("toggle_engine"),
                "Flip board",
                keys("flip_board"),
            ))
            .child(row(
                "Search",
                keys("search"),
                "Save game",
                keys("save_game"),
            ))
            .child(
                h_flex().mt(px(12.)).child(
                    btn("edit-shortcuts", "Edit shortcuts")
                        .h(30.)
                        .on_click(move |window, cx| {
                            this.update(cx, |screen, cx| {
                                screen.set_page(SettingsPage::Shortcuts, window, cx)
                            });
                        }),
                ),
            )
    }

    fn render_general(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        v_flex()
            .gap(px(16.))
            .child(page_title("General"))
            .child(self.render_appearance())
            .child(self.render_board_section(cx))
            .child(self.render_shortcut_summary(cx))
    }

    fn render_board_page(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        v_flex()
            .gap(px(16.))
            .child(page_title("Board & pieces"))
            .child(self.render_board_section(cx))
    }

    fn render_engines(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let this = cx.entity();
        let add_this = this.clone();
        let model = self.model.read(cx);
        let header = h_flex()
            .justify_between()
            .child(page_title("Engines"))
            .child(
                h_flex()
                    .gap(px(12.))
                    .child(btn("add-engine", "Add from file").on_click(move |_, cx| {
                        add_this.update(cx, |screen, cx| screen.add_from_file(cx));
                    }))
                    .child(
                        btn("download-engine", "Download engine")
                            .primary()
                            .tooltip("Opens stockfishchess.org in your browser")
                            .on_click(|_, cx| cx.open_url("https://stockfishchess.org/download/")),
                    ),
            );
        const COL_ENGINE: f32 = 468.;
        const COL_PROTOCOL: f32 = 102.;
        const COL_RUNS: f32 = 102.;
        const COL_USED: f32 = 339.;
        let mut table = panel().overflow_hidden().child(
            h_flex()
                .h(px(32.))
                .px(px(16.))
                .bg(rgb(T.surface_2))
                .border_b_1()
                .border_color(rgb(T.border))
                .child(div().w(px(COL_ENGINE)).child(section_label("Engine")))
                .child(div().w(px(COL_PROTOCOL)).child(section_label("Protocol")))
                .child(div().w(px(COL_RUNS)).child(section_label("Runs on")))
                .child(div().w(px(COL_USED)).child(section_label("Used for")))
                .child(section_label("Status")),
        );
        for (index, engine) in model.config.engines.iter().enumerate() {
            let selected = self.selected_engine == Some(engine.id);
            let id = engine.id;
            let select_this = this.clone();
            let status = model
                .engine_status
                .get(&id)
                .copied()
                .unwrap_or(EngineStatus::Ready);
            let (status_text, status_color) = match status {
                EngineStatus::Ready => ("Ready", T.good),
                EngineStatus::Disabled => ("Disabled", T.muted),
                EngineStatus::Error => ("Error", T.warn),
            };
            table = table.child(
                h_flex()
                    .id(("engine", index))
                    .relative()
                    .h(px(43.))
                    .px(px(16.))
                    .cursor_pointer()
                    .border_b_1()
                    .border_color(rgb(T.divider))
                    .when(selected, |row| row.bg(rgb(T.selected)))
                    .when(!selected, |row| row.hover(|style| style.bg(rgb(T.hover))))
                    .on_click(move |_, window, cx| {
                        select_this.update(cx, |screen, cx| {
                            screen.select_engine(Some(id), window, cx);
                        });
                    })
                    .when(selected, |row| {
                        row.child(
                            div()
                                .absolute()
                                .left_0()
                                .top_0()
                                .bottom_0()
                                .w(px(3.))
                                .bg(rgb(T.fg)),
                        )
                    })
                    .child(
                        h_flex()
                            .w(px(COL_ENGINE))
                            .gap(px(4.))
                            .child(
                                div()
                                    .font_weight(FontWeight::BOLD)
                                    .child(SharedString::from(engine.name.clone())),
                            )
                            .child(
                                mono(engine.version.clone())
                                    .text_size(px(13.))
                                    .text_color(rgb(T.muted)),
                            ),
                    )
                    .child(
                        mono("UCI")
                            .w(px(COL_PROTOCOL))
                            .text_size(px(13.))
                            .text_color(rgb(T.fg_2)),
                    )
                    .child(
                        div()
                            .w(px(COL_RUNS))
                            .text_color(rgb(T.fg_2))
                            .child(if engine.runs_on_gpu { "GPU" } else { "CPU" }),
                    )
                    .child(
                        div()
                            .w(px(COL_USED))
                            .text_color(rgb(T.fg_2))
                            .child(engine.used_for()),
                    )
                    .child(
                        div()
                            .text_size(px(13.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(status_color))
                            .child(status_text),
                    ),
            );
        }
        if model.config.engines.is_empty() {
            table = table.child(
                div()
                    .p(px(20.))
                    .text_size(px(14.))
                    .text_color(rgb(T.muted))
                    .child("No engines yet. Add a UCI engine executable with Add from file."),
            );
        }
        let detail = self
            .selected_engine
            .and_then(|id| model.engine(id).cloned())
            .map(|config| self.render_engine_detail(&config, window, cx));
        v_flex()
            .gap(px(21.))
            .child(header)
            .child(table)
            .children(detail)
    }

    fn render_engine_detail(
        &mut self,
        config: &EngineConfig,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let this = cx.entity();
        let status = self.model.read(cx).engine_status.get(&config.id).copied();
        let mut options: Vec<UciOption> = config
            .info
            .options
            .iter()
            .filter(|option| !option_is_hidden(&option.name))
            .cloned()
            .collect();
        options.sort_by_key(|option| option_rank(&option.name));
        let mut body = v_flex().child(detail_row(
            "Executable",
            None,
            div().w_full().child(text_input(&self.path_input)),
        ));
        for option in options {
            let (label, hint) = option_label(&option.name);
            let control: gpui::AnyElement = match &option.kind {
                OptionKind::Spin { .. } | OptionKind::String { .. } => {
                    let Some(input) = self.option_inputs.get(&option.name) else {
                        continue;
                    };
                    let suffix = (option.name == "Hash").then_some("MB");
                    div()
                        .w(px(320.))
                        .child(text_input(input).suffix(suffix))
                        .into_any_element()
                }
                OptionKind::Combo { .. } => {
                    let Some(select) = self.option_selects.get(&option.name) else {
                        continue;
                    };
                    div()
                        .w(px(320.))
                        .child(select_el(select))
                        .into_any_element()
                }
                OptionKind::Check { default } => {
                    let current = config
                        .options
                        .get(&option.name)
                        .map_or(*default, |value| value == "true");
                    let name = option.name.clone();
                    let this = this.clone();
                    div()
                        .id(SharedString::from(format!("toggle-{}", option.name)))
                        .w(px(320.))
                        .h(px(36.))
                        .px(px(12.))
                        .flex()
                        .items_center()
                        .cursor_pointer()
                        .rounded(px(4.))
                        .bg(rgb(T.bg))
                        .border_1()
                        .border_color(rgb(T.border_strong))
                        .on_click(move |_, _, cx| {
                            let value = (!current).to_string();
                            this.update(cx, |screen, cx| {
                                screen.set_engine_option(&name, value, cx)
                            });
                        })
                        .child(if current { "On" } else { "Off" })
                        .into_any_element()
                }
                OptionKind::Button => {
                    let name = option.name.clone();
                    let model = self.model.clone();
                    let id = config.id;
                    btn(
                        SharedString::from(format!("button-{}", option.name)),
                        option.name.clone(),
                    )
                    .h(30.)
                    .on_click(move |_, cx| {
                        if let Some(handle) = model.read(cx).manager.get(id) {
                            handle.set_option(name.clone(), None);
                        }
                    })
                    .into_any_element()
                }
            };
            body = body.child(detail_row(&label, hint, control));
        }
        let usage_check = |id: &'static str,
                           value: bool,
                           label: &'static str,
                           this: Entity<Self>,
                           set: fn(&mut EngineConfig, bool)| {
            div().child(check(id, value, label).on_change(move |value, _, cx| {
                this.update(cx, |screen, cx| {
                    screen.set_engine_use(|config| set(config, value), cx);
                });
            }))
        };
        let bench_this = this.clone();
        let update_model = self.model.clone();
        let remove_this = this.clone();
        let restart_this = this.clone();
        let name = config.name.clone();
        panel()
            .p(px(21.))
            .child(
                h_flex()
                    .justify_between()
                    .mb(px(6.))
                    .child(
                        h_flex()
                            .gap(px(10.))
                            .items_baseline()
                            .child(div().text_size(px(20.)).font_weight(FontWeight::SEMIBOLD).child(name.clone()))
                            .child(mono(config.version.clone()).text_size(px(16.)).text_color(rgb(T.muted))),
                    )
                    .child(
                        h_flex()
                            .gap(px(12.))
                            .when(status == Some(EngineStatus::Error), |row| {
                                row.child(btn("restart-engine", "Restart").h(32.).on_click(move |_, cx| {
                                    restart_this.update(cx, |screen, cx| screen.restart_engine(cx));
                                }))
                            })
                            .child(btn("benchmark", "Run benchmark").h(32.).on_click(move |_, cx| {
                                bench_this.update(cx, |screen, cx| screen.run_benchmark(cx));
                            }))
                            .child(btn("check-update", "Check for update").h(32.).on_click(move |_, cx| {
                                let name = name.clone();
                                update_model.update(cx, |model, cx| {
                                    model.toast(
                                        format!("Outpost works offline, so it cannot look up {name} versions. Compare with the engine's website."),
                                        cx,
                                    );
                                });
                            }))
                            .child(btn("remove-engine", "Remove").h(32.).on_click(move |window, cx| {
                                remove_this.update(cx, |screen, cx| screen.remove_engine(window, cx));
                            })),
                    ),
            )
            .children(self.benchmark.clone().map(|text| {
                div().mb(px(4.)).text_size(px(13.)).font_family(MONO).text_color(rgb(T.fg_2)).child(text)
            }))
            .child(body)
            .child(
                h_flex()
                    .mt(px(14.))
                    .child(
                        div()
                            .w(px(236.))
                            .flex_none()
                            .font_weight(FontWeight::MEDIUM)
                            .child("Use this engine for"),
                    )
                    .child(
                        h_flex()
                            .gap(px(28.))
                            .child(usage_check("use-analysis", config.use_analysis, "Live analysis", this.clone(), |c, v| c.use_analysis = v))
                            .child(usage_check("use-review", config.use_review, "Batch game review", this.clone(), |c, v| c.use_review = v))
                            .child(usage_check("use-play", config.use_play, "Play", this.clone(), |c, v| c.use_play = v)),
                    ),
            )
    }

    fn render_databases(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let model = self.model.read(cx);
        let mut rows = v_flex();
        for entry in &model.databases {
            rows = rows.child(
                h_flex()
                    .h(px(43.))
                    .justify_between()
                    .border_b_1()
                    .border_color(rgb(T.divider))
                    .child(
                        div()
                            .font_weight(FontWeight::MEDIUM)
                            .child(entry.name.clone()),
                    )
                    .child(
                        mono(format_count(entry.games))
                            .text_size(px(14.))
                            .text_color(rgb(T.muted)),
                    ),
            );
        }
        if model.databases.is_empty() {
            rows = rows.child(
                div()
                    .py(px(10.))
                    .text_color(rgb(T.muted))
                    .child("No databases yet. Import a PGN from the Database screen."),
            );
        }
        let location = model.paths.databases_dir().display().to_string();
        v_flex()
            .gap(px(16.))
            .child(page_title("Databases"))
            .child(section("You").child(option_row(
                "Player name",
                Some("How you appear in PGN headers. Used for My games and leak detection."),
                div().w(px(320.)).child(text_input(&self.player_name)),
            )))
            .child(
                section("Files").child(rows).child(
                    div()
                        .mt(px(10.))
                        .text_size(px(13.))
                        .text_color(rgb(T.muted))
                        .child(format!("Stored in {location}")),
                ),
            )
    }

    fn render_shortcuts(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let this = cx.entity();
        let model = self.model.read(cx);
        let mut list = v_flex();
        for (index, (action, label)) in SHORTCUT_ROWS.into_iter().enumerate() {
            let capturing = self.capturing == Some(action);
            let this = this.clone();
            let capture_focus = self.capture_focus.clone();
            list = list.child(
                h_flex()
                    .h(px(44.))
                    .justify_between()
                    .border_b_1()
                    .border_color(rgb(T.divider))
                    .child(div().font_weight(FontWeight::MEDIUM).child(label))
                    .child(
                        h_flex()
                            .gap(px(14.))
                            .child(if capturing {
                                div()
                                    .text_size(px(13.))
                                    .text_color(rgb(T.warn))
                                    .child("Press a key… (Esc cancels)")
                            } else {
                                keycaps(model.config.shortcuts.get(action))
                            })
                            .child(btn(("rebind", index), "Change").h(30.).on_click(
                                move |window, cx| {
                                    capture_focus.focus(window, cx);
                                    this.update(cx, |screen, cx| {
                                        screen.capturing = Some(action);
                                        cx.notify();
                                    });
                                },
                            )),
                    ),
            );
        }
        v_flex()
            .gap(px(16.))
            .child(page_title("Keyboard shortcuts"))
            .child(section("Shortcuts").child(list))
    }

    fn render_accounts(&mut self) -> impl IntoElement + use<> {
        v_flex().gap(px(16.)).child(page_title("Accounts")).child(
            section("Offline by design").child(div().text_color(rgb(T.fg_2)).child(
                "Outpost has no accounts or cloud sync: everything stays on this computer.",
            )),
        )
    }
}

fn section(title: &'static str) -> Div {
    panel().flex().flex_col().p(px(16.)).child(
        div()
            .mb(px(10.))
            .text_size(px(15.))
            .font_weight(FontWeight::BOLD)
            .child(title),
    )
}

fn page_title(title: &'static str) -> Div {
    div()
        .text_size(px(24.))
        .font_weight(FontWeight::SEMIBOLD)
        .child(title)
}

/// Two-column option row: label + hint on the left, control on the right.
fn option_row(label: &'static str, hint: Option<&'static str>, control: impl IntoElement) -> Div {
    h_flex()
        .min_h(px(44.))
        .py(px(5.))
        .border_b_1()
        .border_color(rgb(T.divider))
        .child(
            v_flex()
                .w(px(232.))
                .flex_none()
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_size(px(14.))
                        .child(label),
                )
                .children(hint.map(|hint| {
                    div()
                        .text_size(px(13.))
                        .text_color(rgb(T.muted))
                        .pr(px(12.))
                        .child(hint)
                })),
        )
        .child(control)
}

fn detail_row(label: &str, hint: Option<&'static str>, control: impl IntoElement) -> Div {
    h_flex()
        .min_h(px(60.))
        .py(px(5.))
        .border_b_1()
        .border_color(rgb(T.divider))
        .child(
            v_flex()
                .w(px(236.))
                .flex_none()
                .child(
                    div()
                        .font_weight(FontWeight::MEDIUM)
                        .text_size(px(15.))
                        .child(SharedString::from(label.to_owned())),
                )
                .children(hint.map(|hint| {
                    div()
                        .text_size(px(13.))
                        .text_color(rgb(T.muted))
                        .child(hint)
                })),
        )
        .child(div().flex_1().min_w_0().child(control))
}

fn tile(id: impl Into<gpui::ElementId>, selected: bool) -> gpui::Stateful<Div> {
    div()
        .id(id)
        .flex()
        .flex_col()
        .gap(px(6.))
        .p(px(8.))
        .w(px(74.))
        .rounded(px(6.))
        .cursor_pointer()
        .border_1()
        .border_color(rgb(if selected { T.fg } else { T.border_strong }))
        .hover(|style| style.bg(rgb(T.hover)))
}

fn tile_label(text: &'static str) -> Div {
    div().text_size(px(13.)).text_color(rgb(T.fg_2)).child(text)
}

fn board_check(
    id: &'static str,
    value: bool,
    label: &'static str,
    this: Entity<SettingsScreen>,
    set: fn(&mut crate::config::BoardConfig, bool),
) -> impl IntoElement {
    div().child(check(id, value, label).on_change(move |value, _, cx| {
        this.update(cx, |screen, cx| {
            screen.update_board(cx, |board| set(board, value));
        });
    }))
}

/// Renders "ctrl-k" as separate caps, with friendly names.
fn keycaps(keystroke: Option<&String>) -> Div {
    let mut row = h_flex().gap(px(4.));
    let Some(keystroke) = keystroke else {
        return row;
    };
    for part in keystroke.split('-') {
        let text = match part {
            "left" => "←".to_owned(),
            "right" => "→".to_owned(),
            "up" => "↑".to_owned(),
            "down" => "↓".to_owned(),
            "ctrl" => "Ctrl".to_owned(),
            "cmd" => "⌘".to_owned(),
            "alt" => "Alt".to_owned(),
            "shift" => "Shift".to_owned(),
            other => {
                let mut chars = other.chars();
                match chars.next() {
                    Some(first) => first.to_uppercase().chain(chars).collect(),
                    None => String::new(),
                }
            }
        };
        row = row.child(key_cap(text));
    }
    row
}

/// `bench` if the engine supports it, otherwise a fixed-time search on a small suite.
fn benchmark(config: &EngineConfig) -> String {
    let output = Command::new(&config.path).arg("bench").output();
    if let Ok(output) = output {
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        if let Some(result) = parse_bench(&text) {
            return result;
        }
    }
    const SUITE: [&str; 3] = [
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
        "r1bq1rk1/2p1bppp/p1np1n2/1p2p3/4P3/1BP2N1P/PP1P1PP1/RNBQR1K1 b - - 0 9",
        "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
    ];
    let options = config
        .options
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let handle = EngineHandle::spawn(config.path.clone(), Vec::new(), options);
    let events = handle.events();
    let mut speeds = Vec::new();
    for (index, fen) in SUITE.iter().enumerate() {
        let search_id = index as u64 + 1;
        handle.search(SearchRequest {
            fen: (*fen).to_owned(),
            moves: Vec::new(),
            limits: Some(Limits {
                movetime_ms: Some(2000),
                ..Default::default()
            }),
            search_id,
        });
        let mut speed = None;
        loop {
            match smol::block_on(events.recv()) {
                Ok(EngineEvent::Analysis { update, .. }) => speed = update.nps.or(speed),
                Ok(EngineEvent::BestMove {
                    search_id: done, ..
                }) if done == search_id => break,
                Ok(EngineEvent::Crashed(reason)) => return format!("Benchmark failed: {reason}"),
                Ok(_) => {}
                Err(_) => return "Benchmark failed: engine closed".to_owned(),
            }
        }
        speeds.extend(speed);
    }
    handle.quit();
    if speeds.is_empty() {
        return "Benchmark finished, but the engine reported no speed.".to_owned();
    }
    let average = speeds.iter().sum::<u64>() / speeds.len() as u64;
    format!("Fixed-time suite: {} n/s", format_count(average))
}

fn parse_bench(text: &str) -> Option<String> {
    let line = text
        .lines()
        .rev()
        .find(|line| line.contains("Nodes/second"))?;
    let value: u64 = line.split(':').nth(1)?.trim().parse().ok()?;
    Some(format!("bench: {} n/s", format_count(value)))
}

impl Render for SettingsScreen {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let nav = self.render_nav(cx);
        let content: gpui::AnyElement = match self.page {
            SettingsPage::General => self.render_general(cx).into_any_element(),
            SettingsPage::Board => self.render_board_page(cx).into_any_element(),
            SettingsPage::Engines => self.render_engines(window, cx).into_any_element(),
            SettingsPage::Databases => self.render_databases(cx).into_any_element(),
            SettingsPage::Shortcuts => self.render_shortcuts(cx).into_any_element(),
            SettingsPage::Accounts => self.render_accounts().into_any_element(),
        };
        h_flex().items_start().size_full().child(nav).child(
            div()
                .id("settings-content")
                .track_focus(&self.capture_focus)
                .on_key_down(cx.listener(|this, event, _, cx| this.on_capture_key(event, cx)))
                .flex_1()
                .min_w_0()
                .h_full()
                .overflow_y_scroll()
                .px(px(28.))
                .pt(px(24.))
                .pb(px(24.))
                .child(div().max_w(px(1180.)).child(content)),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bench_output_is_parsed() {
        let text =
            "stuff\nTotal time (ms) : 100\nNodes searched  : 1000\nNodes/second    : 123456\n";
        assert_eq!(parse_bench(text).as_deref(), Some("bench: 123,456 n/s"));
        assert_eq!(parse_bench("no numbers"), None);
    }

    #[test]
    fn known_options_sort_first() {
        assert!(option_rank("Threads") < option_rank("Hash"));
        assert!(option_rank("UCI_ShowWDL") < option_rank("Contempt"));
        assert!(option_is_hidden("UCI_Elo"));
        assert_eq!(option_label("SyzygyPath").0, "Syzygy path");
    }
}
