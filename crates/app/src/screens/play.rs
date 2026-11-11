//! Play against a UCI engine (or a scripted opponent in demo mode), or watch two engines
//! play each other.

use std::time::{Duration, Instant};

use chess_core::{Color, GameTree, Move, NodeId, Position, classify, shakmaty::uci::UciMove};
use engine::{EngineConfig, EngineEvent, EngineHandle, Limits, OptionKind, SearchRequest};
use gpui::{
    AppContext as _, Context, Entity, FontWeight, IntoElement, Pixels, Point, Render, ScrollHandle,
    SharedString, Subscription, Task, WeakEntity, Window, anchored, deferred, div, point,
    prelude::*, px, rgb,
};
use gpui_component::{
    scroll::{Scrollbar, ScrollbarMode},
    select::SelectEvent,
    slider::{Slider, SliderEvent, SliderState},
};
use ui::{
    BoardEvent, BoardView, MONO, T,
    widgets::{btn, check, h_flex, mono, panel, section_label, segmented, v_flex},
};

use crate::{
    controls::{TextSelect, field, new_input, new_select, select_el, text_input},
    model::{AppModel, ModelEvent, ToastExt as _},
    shell::Workspace,
};

const TIME_CONTROLS: [(u32, u32); 5] = [(3, 2), (5, 0), (10, 5), (15, 10), (30, 0)];

/// Whether the user plays an engine or watches two engines play each other.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    VsEngine,
    Engines,
}

/// An engine picked for a game, and how strongly it should play.
struct EngineChoice {
    engine: TextSelect,
    strength: Entity<SliderState>,
    elo: u32,
    /// Play as well as it can instead of at `elo`.
    full: bool,
    threads: u32,
    hash: u32,
}

/// Finds one of the screen's engine choices, for handlers that outlive a borrow of it.
type PickChoice = fn(&mut PlayScreen) -> &mut EngineChoice;

/// The engine that plays White when engines play both sides.
struct WhiteEngine {
    name: String,
    rating: Option<u32>,
    engine: Option<EngineHandle>,
    _reader: Option<Task<()>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ColourChoice {
    White,
    Random,
    Black,
}

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

/// A PV move and its resulting position for the hover board.
struct PvMove {
    label: String,
    position: chess_core::Chess,
    last: Option<(chess_core::Square, chess_core::Square)>,
}

/// The latest search line for one side, kept visible while the other side thinks.
#[derive(Default)]
struct GameAnalysis {
    depth: u32,
    score: Option<engine::Score>,
    pv: Vec<PvMove>,
}

/// A played game with independent engines, clocks and live analysis.
struct Game {
    tree: GameTree,
    current: NodeId,
    user: Color,
    /// Set when engines play both sides and the board is read-only. `opponent_name`,
    /// `opponent_rating` and `engine` are then Black's.
    white: Option<WhiteEngine>,
    white_ms: u64,
    black_ms: u64,
    increment_ms: [u64; 2],
    /// A per-move search limit replaces the game clock.
    fixed_limits: [Option<Limits>; 2],
    result: Option<String>,
    opponent_name: String,
    opponent_rating: Option<u32>,
    label: String,
    takebacks: bool,
    /// The player may queue a move during the engine's turn.
    premoves: bool,
    /// The queued move, played as soon as it is the player's turn if still legal.
    premove: Option<(chess_core::Square, chess_core::Square)>,
    last_tick: Instant,
    search_id: u64,
    analysis: [GameAnalysis; 2],
    engine: Option<EngineHandle>,
    _engine_reader: Option<Task<()>>,
    _clock: Option<Task<()>>,
}

impl Game {
    /// Engines play both sides.
    fn self_play(&self) -> bool {
        self.white.is_some()
    }

    /// Every engine process this game runs.
    fn engines(&self) -> impl Iterator<Item = &EngineHandle> {
        self.engine
            .iter()
            .chain(self.white.iter().filter_map(|white| white.engine.as_ref()))
    }
}

impl Drop for Game {
    fn drop(&mut self) {
        for engine in self.engines() {
            engine.quit();
        }
    }
}

/// Keeps one engine's clock and per-move search settings independent.
struct EngineTime {
    time: crate::controls::ClockInput,
    unlimited: bool,
    kind: TextSelect,
    value: Entity<gpui_component::input::InputState>,
}

impl EngineTime {
    /// Restores the last clock values, units and per-move search limit for this side.
    fn new(
        saved: &crate::config::PlayEngineTime,
        window: &mut Window,
        cx: &mut Context<PlayScreen>,
    ) -> Self {
        Self {
            time: crate::controls::ClockInput {
                base: crate::controls::TimeInput::new(
                    &saved.base,
                    saved.base_unit.min(3),
                    window,
                    cx,
                ),
                increment: crate::controls::TimeInput::new(
                    &saved.increment,
                    saved.increment_unit.min(3),
                    window,
                    cx,
                ),
            },
            unlimited: saved.unlimited,
            kind: new_select(
                vec!["Depth".into(), "Nodes".into()],
                saved.limit_kind.min(1),
                window,
                cx,
            ),
            value: new_input(&saved.limit_value, "20", window, cx),
        }
    }

    /// Saves both clock fields exactly as entered, including their chosen units.
    fn saved(&self, cx: &gpui::App) -> crate::config::PlayEngineTime {
        let (base, base_unit) = self.time.base.saved(cx);
        let (increment, increment_unit) = self.time.increment.saved(cx);
        crate::config::PlayEngineTime {
            base,
            base_unit,
            increment,
            increment_unit,
            unlimited: self.unlimited,
            limit_kind: self
                .kind
                .read(cx)
                .selected_index(cx)
                .map_or(0, |index| index.row),
            limit_value: self.value.read(cx).value().to_string(),
        }
    }

    /// Validates this side's clock or positive per-move search limit.
    fn settings(&self, cx: &gpui::App) -> Option<(u64, u64, Option<Limits>)> {
        if !self.unlimited {
            let (base_ms, increment_ms) = self.time.milliseconds(cx)?;
            return Some((base_ms, increment_ms, None));
        }
        let value = self
            .value
            .read(cx)
            .value()
            .trim()
            .parse::<u64>()
            .ok()
            .filter(|v| *v > 0)?;
        let nodes = self
            .kind
            .read(cx)
            .selected_index(cx)
            .is_some_and(|i| i.row == 1);
        if !nodes && value > u64::from(u32::MAX) {
            return None;
        }
        Some((
            0,
            0,
            Some(Limits {
                depth: (!nodes).then_some(value as u32),
                nodes: nodes.then_some(value),
                ..Default::default()
            }),
        ))
    }
}

pub struct PlayScreen {
    recovery: Option<crate::recovery::Recovery>,
    recovery_path: std::path::PathBuf,
    suspended: bool,
    model: Entity<AppModel>,
    workspace: WeakEntity<Workspace>,
    board: Entity<BoardView>,
    pv_board: Entity<BoardView>,
    /// Follows each played move while allowing the move list to scroll independently.
    moves_scroll: ScrollHandle,
    /// An earlier move being reviewed while the game continues at its live position.
    viewed: Option<NodeId>,
    pv_hover: Option<(usize, usize)>,
    pv_at: Option<Point<Pixels>>,
    /// The engine the user plays against.
    opponent: EngineChoice,
    /// The two sides of an engine game.
    white: EngineChoice,
    black: EngineChoice,
    opponents: Vec<EngineConfig>,
    mode: Mode,
    show_setup: bool,
    time_index: Option<usize>,
    custom_time: crate::controls::ClockInput,
    engine_times: [EngineTime; 2],
    book_path: Entity<gpui_component::input::InputState>,
    book_tree: Option<GameTree>,
    book_task: Option<Task<()>>,
    colour: ColourChoice,
    start_from: TextSelect,
    /// The Chess960 starting position number.
    chess960_number: Entity<gpui_component::input::InputState>,
    /// The position typed or pasted when the game starts from a FEN.
    start_fen: Entity<gpui_component::input::InputState>,
    hide_eval: bool,
    allow_takebacks: bool,
    allow_premoves: bool,
    game: Option<Game>,
    /// Replies scheduled from the demo opponent.
    demo_task: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl PlayScreen {
    /// The engine is thinking about its move in an unfinished game.
    pub fn engine_thinking(&self) -> bool {
        self.game.as_ref().is_some_and(|game| {
            game.result.is_none()
                && game.engine.is_some()
                && (game.self_play() || game.tree.node(game.current).pos.turn() != game.user)
        })
    }

    pub fn new(
        model: Entity<AppModel>,
        workspace: WeakEntity<Workspace>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let path = model.read(cx).paths.data_dir.join("play-recovery.json");
        Self::new_with_recovery(model, workspace, window, cx, path)
    }

    pub fn new_with_recovery(
        model: Entity<AppModel>,
        workspace: WeakEntity<Workspace>,
        window: &mut Window,
        cx: &mut Context<Self>,
        recovery_path: std::path::PathBuf,
    ) -> Self {
        let board = cx.new(|_| BoardView::new("play-board"));
        let prefs = model.read(cx).config.board.prefs();
        board.update(cx, |board, cx| board.set_prefs(prefs, cx));
        let pv_board = cx.new(|_| BoardView::new("play-pv-board").read_only());
        let opponents = model.read(cx).play_engines();
        let names = opponent_names(&opponents);
        // An engine game starts as a match between the first two engines.
        let other = usize::from(opponents.len() > 1);
        let mut subscriptions = Vec::new();
        let mut choice = |index: usize, full: bool, pick: PickChoice, cx: &mut Context<Self>| {
            let strength = cx.new(|_| {
                SliderState::new()
                    .min(1320.)
                    .max(3190.)
                    .step(10.)
                    .default_value(1800.)
            });
            subscriptions.push(
                cx.subscribe(&strength, move |this, _, event: &SliderEvent, cx| {
                    if let SliderEvent::Change(value) = event {
                        pick(this).elo = value.start().round() as u32;
                        cx.notify();
                    }
                }),
            );
            let engine = new_select(names.clone(), index, window, cx);
            subscriptions.push(cx.subscribe(&engine, |_, _, _: &SelectEvent<_>, cx| cx.notify()));
            EngineChoice {
                engine,
                strength,
                elo: 1800,
                full,
                threads: 1,
                hash: 128,
            }
        };
        let opponent = choice(0, false, |screen| &mut screen.opponent, cx);
        let white = choice(0, true, |screen| &mut screen.white, cx);
        let black = choice(other, true, |screen| &mut screen.black, cx);
        let start_from = new_select(
            [
                "Starting position",
                "Current Analyse position",
                "Repertoire line",
                "FEN",
                "Chess960",
            ]
            .map(SharedString::from)
            .to_vec(),
            0,
            window,
            cx,
        );
        let saved_times = model.read(cx).config.play_engine_times.clone();
        let engine_times = saved_times
            .each_ref()
            .map(|saved| EngineTime::new(saved, window, cx));
        let book_path = new_input("", "Opening book (.pgn or .epd)", window, cx);
        let custom_time = crate::controls::ClockInput::new("5", 2, "3", window, cx);
        let chess960_number = new_input(
            model.read(cx).config.chess960_number.min(959).to_string(),
            "0–959",
            window,
            cx,
        );
        let start_fen = new_input("", "Paste a FEN", window, cx);
        subscriptions.extend([
            cx.subscribe(
                &chess960_number,
                |_, _, _: &gpui_component::input::InputEvent, cx| cx.notify(),
            ),
            cx.subscribe_in(&board, window, |this, _, event, _, cx| match event {
                BoardEvent::Moved(mv) => this.user_move(*mv, cx),
                BoardEvent::Premove(premove) => {
                    if let Some(game) = &mut this.game
                        && game.premoves
                        && game.result.is_none()
                    {
                        game.premove = *premove;
                    }
                }
                BoardEvent::Square { .. } => {}
            }),
            cx.subscribe(&start_from, |_, _, _: &SelectEvent<_>, cx| cx.notify()),
            cx.subscribe_in(
                &model,
                window,
                |this, model, event, window, cx| match event {
                    ModelEvent::BoardPrefs => {
                        let prefs = model.read(cx).config.board.prefs();
                        this.board
                            .update(cx, |board, cx| board.set_prefs(prefs, cx));
                    }
                    ModelEvent::Engines => {
                        this.opponents = model.read(cx).play_engines();
                        let names = opponent_names(&this.opponents);
                        let other = usize::from(this.opponents.len() > 1);
                        for (pick, index) in [
                            ((|screen| &mut screen.opponent) as PickChoice, 0),
                            (|screen| &mut screen.white, 0),
                            (|screen| &mut screen.black, other),
                        ] {
                            let engine = new_select(names.clone(), index, window, cx);
                            this._subscriptions.push(
                                cx.subscribe(&engine, |_, _, _: &SelectEvent<_>, cx| cx.notify()),
                            );
                            pick(this).engine = engine;
                        }
                        cx.notify();
                    }
                    _ => {}
                },
            ),
        ]);
        let demo = model.read(cx).demo;
        let mut this = Self {
            recovery: crate::recovery::Recovery::load(&recovery_path),
            recovery_path,
            suspended: false,
            model,
            workspace,
            board,
            pv_board,
            moves_scroll: ScrollHandle::new(),
            viewed: None,
            pv_hover: None,
            pv_at: None,
            opponent,
            white,
            black,
            opponents,
            mode: Mode::VsEngine,
            show_setup: true,
            time_index: Some(2),
            custom_time,
            engine_times,
            book_path,
            book_tree: None,
            book_task: None,
            colour: ColourChoice::White,
            start_from,
            chess960_number,
            start_fen,
            hide_eval: true,
            allow_takebacks: false,
            allow_premoves: true,
            game: None,
            demo_task: None,
            _subscriptions: subscriptions,
        };
        if demo {
            this.start_demo_game(cx);
        }
        this
    }

    pub fn suspend(&mut self, cx: &mut Context<Self>) {
        self.tick(cx);
        self.suspended = true;
        if let Some(game) = &mut self.game {
            game._clock = None;
            for engine in game.engines() {
                engine.stop();
            }
        }
    }

    pub fn resume(&mut self, cx: &mut Context<Self>) {
        if !self.suspended {
            return;
        }
        self.suspended = false;
        if self.game.as_ref().is_some_and(|game| game.result.is_none()) {
            self.start_clock(cx);
            self.maybe_request_engine_move(cx);
        }
    }

    /// The file this tab keeps its game in, so the tab can be reopened on the next start.
    pub fn recovery_path(&self) -> &std::path::Path {
        &self.recovery_path
    }

    pub fn breadcrumb(&self) -> SharedString {
        match &self.game {
            Some(Game {
                white: Some(white),
                opponent_name,
                label,
                ..
            }) => format!("{} vs {opponent_name}, {label}", white.name).into(),
            Some(game) => format!("vs {}, {}", game.opponent_name, game.label).into(),
            None => ui::i18n::t("New game").into(),
        }
    }

    pub fn flip(&mut self, cx: &mut Context<Self>) {
        self.board.update(cx, |board, cx| {
            let flipped = board.is_flipped();
            board.set_flipped(!flipped, cx);
        });
    }

    fn start_demo_game(&mut self, cx: &mut Context<Self>) {
        let mut tree = GameTree::new();
        let mut node = GameTree::ROOT;
        for san in crate::demo::PLAY_LINE.split(' ') {
            match tree.add_san(node, san) {
                Ok(next) => node = next,
                Err(err) => {
                    log::error!("demo line: {err}");
                    return;
                }
            }
        }
        self.moves_scroll.scroll_to_bottom();
        self.viewed = None;
        self.game = Some(Game {
            tree,
            current: node,
            user: Color::White,
            white: None,
            white_ms: 552_000,
            black_ms: 521_000,
            increment_ms: [5_000; 2],
            fixed_limits: [None; 2],
            result: None,
            opponent_name: "Stockfish".to_owned(),
            opponent_rating: Some(self.opponent.elo),
            label: "10+5".to_owned(),
            takebacks: false,
            premoves: true,
            premove: None,
            last_tick: Instant::now(),
            search_id: 0,
            analysis: Default::default(),
            engine: None,
            _engine_reader: None,
            _clock: None,
        });
        self.sync_board(false, cx);
        self.start_clock(cx);
    }

    fn time_control(&self, cx: &gpui::App) -> Option<(u64, u64)> {
        match self.time_index {
            Some(index) => TIME_CONTROLS
                .get(index)
                .map(|&(m, i)| (u64::from(m) * 60_000, u64::from(i) * 1_000)),
            None => self.custom_time.milliseconds(cx),
        }
    }

    /// The engine picked in `choice`, or `None` when no engine is set up for Play.
    fn chosen_engine(&self, choice: &EngineChoice, cx: &gpui::App) -> Option<EngineConfig> {
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

    /// Starts a fresh process of `config` for one game, limited to `elo` if given.
    fn spawn_engine(
        &self,
        config: &EngineConfig,
        elo: Option<u32>,
        cx: &mut Context<Self>,
    ) -> (EngineHandle, Task<()>) {
        // Analysis-only settings would just weaken or slow the search.
        let mut options: Vec<(String, String)> = config
            .options
            .iter()
            .filter(|(name, _)| !matches!(name.as_str(), "MultiPV" | "UCI_ShowWDL"))
            .map(|(name, value)| (name.clone(), value.clone()))
            .collect();
        if let Some(elo) = elo {
            options.push(("UCI_LimitStrength".to_owned(), "true".to_owned()));
            options.push(("UCI_Elo".to_owned(), elo.to_string()));
        }
        // Its first search sends `ucinewgame`: each game gets its own process.
        let handle = EngineHandle::spawn(config.path.clone(), Vec::new(), options);
        let reader = self.spawn_reader(&handle, cx);
        (handle, reader)
    }

    /// The position the new game starts from, or `None` when the pasted FEN is not valid.
    fn start_tree(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<GameTree> {
        let choice = self
            .start_from
            .read(cx)
            .selected_index(cx)
            .map_or(0, |index| index.row);
        let _ = window;
        Some(match choice {
            1 => self
                .workspace
                .upgrade()
                .map(|workspace| {
                    let analyse = workspace.read(cx).analyse.read(cx);
                    GameTree::from_position(analyse.tree.node(analyse.current).pos.clone())
                })
                .unwrap_or_default(),
            2 => {
                // The repertoire's main line.
                let mut tree = GameTree::new();
                let mut node = GameTree::ROOT;
                let model = self.model.read(cx);
                let mut level = &model.repertoire.roots;
                while let Some(first) = level.first() {
                    match tree.add_san(node, &first.san) {
                        Ok(next) => node = next,
                        Err(_) => break,
                    }
                    level = &first.children;
                }
                tree
            }
            4 => GameTree::chess960(
                self.chess960_number
                    .read(cx)
                    .value()
                    .trim()
                    .parse::<u16>()
                    .ok()
                    .filter(|n| *n < 960)?,
            ),
            3 => GameTree::from_fen(self.start_fen.read(cx).value().trim()).ok()?,
            _ => GameTree::new(),
        })
    }

    /// Loads a local opening off the UI thread before starting the engines.
    fn start_with_book(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.book_task.is_some() {
            return;
        }
        let path = self.book_path.read(cx).value().trim().to_owned();
        self.book_tree = None;
        if self.mode != Mode::Engines || path.is_empty() {
            self.start_game(window, cx);
            return;
        }
        let load = cx.background_spawn(async move {
            let text = std::fs::read_to_string(&path).map_err(|err| err.to_string())?;
            opening_from_book(&path, &text)
        });
        self.book_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = load.await;
            let _ = this.update_in(cx, |screen, window, cx| {
                screen.book_task = None;
                match result {
                    Ok(tree) => {
                        screen.book_tree = Some(tree);
                        screen.start_game(window, cx);
                    }
                    Err(err) => screen
                        .model
                        .toast(format!("Could not load opening book: {err}"), cx),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn start_game(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let settings = if self.mode == Mode::Engines {
            self.engine_times[0]
                .settings(cx)
                .zip(self.engine_times[1].settings(cx))
        } else {
            self.time_control(cx)
                .map(|(m, i)| ((m, i, None), (m, i, None)))
        };
        let Some((
            (base_ms, increment_ms, white_limit),
            (black_base_ms, black_increment_ms, black_limit),
        )) = settings
        else {
            self.model.toast(
                "Enter a positive time and a nonnegative increment, or a positive search limit.",
                cx,
            );
            return;
        };
        let fixed_limits = [white_limit, black_limit];
        let demo = self.model.read(cx).demo;
        let self_play = self.mode == Mode::Engines;
        let (white_choice, choice) = if self_play {
            (Some(&self.white), &self.black)
        } else {
            (None, &self.opponent)
        };
        let pick = |choice: &EngineChoice| {
            let config = self.chosen_engine(choice, cx)?;
            let elo = (supports_limit(&config) && !choice.full).then_some(choice.elo);
            Some((config, elo))
        };
        // `white` is `Some(None)` when an engine game has no engine to put on that side.
        let white = white_choice.map(pick);
        let (Some((config, elo)), false) = (pick(choice), white == Some(None)) else {
            self.model
                .toast("Add an engine for Play in Settings › Engines first.", cx);
            return;
        };
        let Some(tree) = self
            .book_tree
            .take()
            .or_else(|| self.start_tree(window, cx))
        else {
            self.model.toast("Enter a valid starting position.", cx);
            return;
        };
        if tree.variant == chess_core::Variant::Chess960 {
            let supported = |config: &EngineConfig| {
                config
                    .info
                    .options
                    .iter()
                    .any(|option| option.name == "UCI_Chess960")
            };
            if !demo
                && !(supported(&config)
                    && white.iter().flatten().all(|(white, _)| supported(white)))
            {
                self.model.toast("No Chess960 support", cx);
                return;
            }
            if let Some(number) = chess_core::chess960_number(&tree.node(GameTree::ROOT).pos) {
                self.model.update(cx, |model, cx| {
                    model.config.chess960_number = number;
                    model.save_config(cx);
                });
            }
        }
        let current = tree
            .line_from(GameTree::ROOT)
            .last()
            .copied()
            .unwrap_or(GameTree::ROOT);
        let user = match self.colour {
            _ if self_play => Color::White,
            ColourChoice::White => Color::White,
            ColourChoice::Black => Color::Black,
            ColourChoice::Random => {
                if std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |elapsed| elapsed.subsec_nanos())
                    .is_multiple_of(2)
                {
                    Color::White
                } else {
                    Color::Black
                }
            }
        };
        let ms = base_ms;
        let white = white.flatten().map(|(config, elo)| {
            let (engine, reader) = if demo {
                (None, None)
            } else {
                let (engine, reader) = self.spawn_engine(&config, elo, cx);
                (Some(engine), Some(reader))
            };
            WhiteEngine {
                name: config.name,
                rating: elo,
                engine,
                _reader: reader,
            }
        });
        let mut game = Game {
            tree,
            current,
            user,
            white,
            white_ms: ms,
            black_ms: black_base_ms,
            increment_ms: [increment_ms, black_increment_ms],
            fixed_limits,
            result: None,
            opponent_name: config.name.clone(),
            opponent_rating: elo,
            label: if self_play {
                format!(
                    "{} / {}",
                    if white_limit.is_some() {
                        "∞".to_owned()
                    } else {
                        format!(
                            "{}s+{}s",
                            crate::controls::time_seconds(base_ms),
                            crate::controls::time_seconds(increment_ms)
                        )
                    },
                    if black_limit.is_some() {
                        "∞".to_owned()
                    } else {
                        format!(
                            "{}s+{}s",
                            crate::controls::time_seconds(black_base_ms),
                            crate::controls::time_seconds(black_increment_ms)
                        )
                    }
                )
            } else {
                format!(
                    "{}s+{}s",
                    crate::controls::time_seconds(base_ms),
                    crate::controls::time_seconds(increment_ms)
                )
            },
            takebacks: self.allow_takebacks && !self_play,
            premoves: self.allow_premoves && !self_play,
            premove: None,
            last_tick: Instant::now(),
            search_id: 0,
            analysis: Default::default(),
            engine: None,
            _engine_reader: None,
            _clock: None,
        };
        if !demo {
            let (engine, reader) = self.spawn_engine(&config, elo, cx);
            game._engine_reader = Some(reader);
            game.engine = Some(engine);
        }
        self.pv_hover = None;
        self.pv_at = None;
        self.moves_scroll.scroll_to_bottom();
        self.viewed = None;
        self.game = Some(game);
        self.show_setup = false;
        if self_play {
            let saved = self.engine_times.each_ref().map(|time| time.saved(cx));
            self.model.update(cx, |model, cx| {
                model.config.play_engine_times = saved;
                model.save_config(cx);
            });
        }
        self.board
            .update(cx, |board, cx| board.set_flipped(user == Color::Black, cx));
        self.sync_board(false, cx);
        // A pasted position may already be mate, stalemate or a dead draw.
        if let Some(game) = &self.game
            && let Some(end) = game.tree.game_end(game.current)
        {
            self.finish(end.describe().to_owned(), cx);
            return;
        }
        self.start_clock(cx);
        self.maybe_request_engine_move(cx);
        cx.notify();
    }

    /// Receives moves and live search lines without blocking the board.
    fn spawn_reader(&self, handle: &EngineHandle, cx: &mut Context<Self>) -> Task<()> {
        let events = handle.events();
        cx.spawn(async move |this, cx| {
            while let Ok(event) = events.recv().await {
                let alive = this.update(cx, |this, cx| match event {
                    EngineEvent::Analysis { search_id, update } => {
                        this.update_analysis(search_id, update, cx);
                    }
                    EngineEvent::BestMove { search_id, best } => {
                        this.engine_move(search_id, best, cx);
                    }
                    EngineEvent::Crashed(reason) => {
                        this.model
                            .toast(format!("The engine stopped: {reason}"), cx);
                        this.finish("Engine stopped".to_owned(), cx);
                    }
                    _ => {}
                });
                if alive.is_err() {
                    return;
                }
            }
        })
    }

    fn start_clock(&mut self, cx: &mut Context<Self>) {
        let Some(game) = &mut self.game else { return };
        if game.fixed_limits.iter().all(Option::is_some) {
            return;
        }
        game.last_tick = Instant::now();
        game._clock = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(200))
                    .await;
                let keep_going = this.update(cx, |this, cx| this.tick(cx)).unwrap_or(false);
                if !keep_going {
                    return;
                }
            }
        }));
    }

    fn tick(&mut self, cx: &mut Context<Self>) -> bool {
        if self.suspended {
            return false;
        }
        let Some(game) = &mut self.game else {
            return false;
        };
        if game.result.is_some() {
            return false;
        }
        let now = Instant::now();
        let elapsed = now.duration_since(game.last_tick).as_millis() as u64;
        game.last_tick = now;
        let turn = game.tree.node(game.current).pos.turn();
        if game.fixed_limits[usize::from(turn == Color::Black)].is_some() {
            return true;
        }
        let clock = if turn == Color::White {
            &mut game.white_ms
        } else {
            &mut game.black_ms
        };
        *clock = clock.saturating_sub(elapsed);
        let flagged = *clock == 0;
        if flagged {
            let text = if turn == Color::White {
                "Black wins on time"
            } else {
                "White wins on time"
            };
            self.finish(text.to_owned(), cx);
            return false;
        }
        cx.notify();
        true
    }

    fn finish(&mut self, text: String, cx: &mut Context<Self>) {
        let Some(game) = &mut self.game else { return };
        if game.result.is_some() {
            return;
        }
        let result_tag = if text.contains("White wins") {
            "1-0"
        } else if text.contains("Black wins") {
            "0-1"
        } else if text.to_lowercase().contains("draw") || text.contains("Stalemate") {
            "1/2-1/2"
        } else {
            "*"
        };
        game.tree.set_header("Result", result_tag);
        game.result = Some(text);
        game._clock = None;
        let white = game.white.as_mut().and_then(|white| white.engine.take());
        for engine in game.engine.take().into_iter().chain(white) {
            engine.stop();
            engine.quit();
        }
        self.demo_task = None;
        self.board
            .update(cx, |board, cx| board.set_interactive(false, cx));
        cx.notify();
    }

    fn sync_board(&mut self, animate: bool, cx: &mut Context<Self>) {
        let Some(game) = &self.game else { return };
        let node = game.tree.node(self.viewed.unwrap_or(game.current));
        let pos = node.pos.clone();
        let last = node.mv.as_ref().and_then(|mv| Some((mv.from()?, mv.to())));
        let user = game.user;
        let active = game.result.is_none();
        let interactive = active && !game.self_play() && self.viewed.is_none();
        let premoves = game.premoves && interactive;
        let premove = game.premove.filter(|_| premoves);
        self.board.update(cx, |board, cx| {
            board.set_position(pos, last, animate, cx);
            board.set_player(Some(user), cx);
            board.set_interactive(interactive, cx);
            board.set_premoves(premoves, cx);
            board.set_premove(premove, cx);
        });
    }

    /// Reviews the previous or next played move without changing the live game.
    pub fn step(&mut self, delta: i32, cx: &mut Context<Self>) {
        let Some(game) = &self.game else { return };
        let current = self.viewed.unwrap_or(game.current);
        let target = review_step(&game.tree, game.current, current, delta);
        if let Some(target) = target {
            self.review(target, cx);
        }
    }

    /// Shows the starting position without interrupting the running engines.
    pub fn go_first(&mut self, cx: &mut Context<Self>) {
        if self.game.is_some() {
            self.review(GameTree::ROOT, cx);
        }
    }

    /// Returns to the live position and resumes following new moves.
    pub fn go_last(&mut self, cx: &mut Context<Self>) {
        if let Some(game) = &self.game {
            self.review(game.current, cx);
        }
    }

    /// Keeps the displayed board, selected move and scroll position together.
    fn review(&mut self, target: NodeId, cx: &mut Context<Self>) {
        let Some(game) = &self.game else { return };
        if target == self.viewed.unwrap_or(game.current) {
            return;
        }
        self.viewed = (target != game.current).then_some(target);
        if self.viewed.is_none() {
            self.moves_scroll.scroll_to_bottom();
        } else {
            let ids = game.tree.path_to(game.current);
            let first_black = ids
                .first()
                .is_some_and(|&id| game.tree.node(id).ply % 2 == 0);
            let row = ids
                .iter()
                .position(|&id| id == target)
                .map_or(0, |index| (index + usize::from(first_black)) / 2);
            self.moves_scroll
                .set_offset(point(px(0.), -px(row as f32 * 27.)));
        }
        let node = game.tree.node(target);
        if let Some(mv) = &node.mv {
            self.model
                .read(cx)
                .config
                .board
                .play_move_sound(mv, &node.pos, None);
        }
        self.sync_board(true, cx);
        cx.notify();
    }

    /// Applies a move from either side: clock, tree, and game-end detection.
    fn apply_move(&mut self, mv: Move, cx: &mut Context<Self>) {
        self.tick(cx);
        let Some(game) = &mut self.game else { return };
        let turn = game.tree.node(game.current).pos.turn();
        if turn == Color::White {
            game.white_ms += game.increment_ms[0];
        } else {
            game.black_ms += game.increment_ms[1];
        }
        game.current = game.tree.add_move(game.current, mv);
        if self.viewed.is_none() {
            self.moves_scroll.scroll_to_bottom();
        }
        let mut recovery_tree = game.tree.clone();
        recovery_tree.set_header("FastWhiteMs", &game.white_ms.to_string());
        recovery_tree.set_header("FastBlackMs", &game.black_ms.to_string());
        let path = self.recovery_path.clone();
        if let Err(err) =
            crate::recovery::Recovery::save(&path, chess_core::write_pgn(&recovery_tree))
        {
            log::error!("Play recovery failed: {err}");
        }
        // Repetition and the move-count rules end the game too, as on Lichess.
        let end = game.tree.game_end(game.current);
        let after = game.tree.node(game.current).pos.clone();
        // The engine just moved: a queued premove is played now if still legal, and
        // dropped otherwise.
        let premove = if after.turn() == game.user {
            game.premove.take()
        } else {
            None
        };
        self.model
            .read(cx)
            .config
            .board
            .play_move_sound(&mv, &after, end);
        self.sync_board(true, cx);
        if let Some(end) = end {
            self.finish(end.describe().to_owned(), cx);
            return;
        }
        if let Some((from, to)) = premove
            && let Some(mv) = ui::board::premove_move(&after, from, to)
        {
            self.user_move(mv, cx);
        }
    }

    fn user_move(&mut self, mv: Move, cx: &mut Context<Self>) {
        let Some(game) = &self.game else { return };
        let turn = game.tree.node(game.current).pos.turn();
        if self.viewed.is_some() || game.result.is_some() || game.self_play() || turn != game.user {
            return;
        }
        self.apply_move(mv, cx);
        self.maybe_request_engine_move(cx);
    }

    fn maybe_request_engine_move(&mut self, cx: &mut Context<Self>) {
        let demo = self.model.read(cx).demo;
        let Some(game) = &mut self.game else { return };
        if game.result.is_some() {
            return;
        }
        let pos = &game.tree.node(game.current).pos;
        let turn = pos.turn();
        if turn == game.user && !game.self_play() {
            return;
        }
        if demo {
            let moves = pos.legal_moves();
            let Some(mv) = moves
                .get(game.tree.node(game.current).ply as usize % moves.len().max(1))
                .cloned()
            else {
                return;
            };
            self.demo_task = Some(cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(Duration::from_millis(900))
                    .await;
                let _ = this.update(cx, |this, cx| {
                    this.apply_move(mv, cx);
                    if this.game.as_ref().is_some_and(Game::self_play) {
                        this.maybe_request_engine_move(cx);
                    }
                    cx.notify();
                });
            }));
            return;
        }
        game.search_id += 1;
        let engine = match &game.white {
            Some(white) if turn == Color::White => &white.engine,
            _ => &game.engine,
        };
        let Some(engine) = engine else { return };
        let moves = game
            .tree
            .path_to(game.current)
            .iter()
            .filter_map(|&id| game.tree.node(id).mv)
            .map(|mv| game.tree.uci(mv))
            .collect();
        engine.search(SearchRequest {
            chess960: game.tree.variant == chess_core::Variant::Chess960,
            search_moves: Vec::new(),
            fen: game.tree.fen(GameTree::ROOT),
            moves,
            limits: Some(
                game.fixed_limits[usize::from(turn == Color::Black)].unwrap_or(Limits {
                    white_ms: Some(game.white_ms),
                    black_ms: Some(game.black_ms),
                    white_inc_ms: game.increment_ms[0],
                    black_inc_ms: game.increment_ms[1],
                    ..Default::default()
                }),
            ),
            search_id: game.search_id,
            game: 0,
        });
    }

    fn engine_move(&mut self, search_id: u64, best: Option<String>, cx: &mut Context<Self>) {
        if self.suspended {
            return;
        }
        let Some(game) = &self.game else { return };
        if search_id != game.search_id || game.result.is_some() {
            return;
        }
        let pos = game.tree.node(game.current).pos.clone();
        let parsed = best
            .as_deref()
            .and_then(|uci| uci.parse::<UciMove>().ok())
            .and_then(|uci| uci.to_move(&pos).ok());
        match parsed {
            Some(mv) => {
                self.apply_move(mv, cx);
                if self.game.as_ref().is_some_and(Game::self_play) {
                    self.maybe_request_engine_move(cx);
                }
                cx.notify();
            }
            None => {
                self.model.toast("The engine returned no legal move.", cx);
                self.finish("Engine stopped".to_owned(), cx);
            }
        }
    }

    /// Keeps the moving engine's score White-relative and its PV in chess notation.
    fn update_analysis(
        &mut self,
        search_id: u64,
        update: engine::AnalysisUpdate,
        cx: &mut Context<Self>,
    ) {
        let Some(game) = &mut self.game else { return };
        if self.suspended
            || game.result.is_some()
            || search_id != game.search_id
            || update.multipv > 1
        {
            return;
        }
        let node = game.tree.node(game.current);
        let black = node.pos.turn() == Color::Black;
        let line = &mut game.analysis[usize::from(black)];
        if update.depth > 0 {
            line.depth = update.depth;
        }
        if !update.bound {
            if let Some(score) = update.score {
                line.score = Some(if black { score.negate() } else { score });
            }
            if !update.pv.is_empty() {
                let mut tree = GameTree::from_position(node.pos.clone());
                let mut current = GameTree::ROOT;
                line.pv = update
                    .pv
                    .iter()
                    .enumerate()
                    .map_while(|(index, uci)| {
                        let position = &tree.node(current).pos;
                        let mv = uci.parse::<UciMove>().ok()?.to_move(position).ok()?;
                        let last = mv.from().map(|from| (from, mv.to()));
                        current = tree.add_move(current, mv);
                        let next = tree.node(current);
                        Some(PvMove {
                            label: chess_core::format_pv(
                                node.ply + index as u32 + 1,
                                std::slice::from_ref(&next.san),
                            ),
                            position: next.pos.clone(),
                            last,
                        })
                    })
                    .collect();
            }
        }
        cx.notify();
    }

    fn resign(&mut self, cx: &mut Context<Self>) {
        let Some(game) = &self.game else { return };
        if game.self_play() {
            self.finish("Game stopped".to_owned(), cx);
            return;
        }
        let text = if game.user == Color::White {
            "Black wins by resignation"
        } else {
            "White wins by resignation"
        };
        self.finish(text.to_owned(), cx);
    }

    fn offer_draw(&mut self, cx: &mut Context<Self>) {
        // UCI has no draw protocol. Long, material-light games are accepted.
        const LATE_PLY: u32 = 80;
        const LOW_PIECES: usize = 10;
        let Some(game) = &self.game else { return };
        let node = game.tree.node(game.current);
        let accepted = node.ply >= LATE_PLY || node.pos.board().occupied().count() <= LOW_PIECES;
        if accepted {
            self.finish("Draw agreed".to_owned(), cx);
        } else {
            self.model
                .toast(format!("{} declines the draw.", game.opponent_name), cx);
        }
    }

    fn takeback(&mut self, cx: &mut Context<Self>) {
        let Some(game) = &mut self.game else { return };
        if !game.takebacks || game.result.is_some() {
            self.model.toast("Takebacks are off for this game.", cx);
            return;
        }
        // A premove was meant for the position being taken back.
        game.premove = None;
        // Back to the user's previous turn: undo the reply and the user's move.
        let mut node = game.current;
        for _ in 0..2 {
            if let Some(parent) = game.tree.node(node).parent {
                node = parent;
            }
        }
        if game.tree.node(node).pos.turn() != game.user {
            return;
        }
        game.search_id += 1;
        if let Some(engine) = &game.engine {
            engine.stop();
        }
        game.current = node;
        self.viewed = None;
        self.moves_scroll.scroll_to_bottom();
        let keep = game.tree.node(node).children.first().copied();
        if let Some(keep) = keep {
            game.tree.delete_from(keep);
        }
        self.sync_board(false, cx);
        cx.notify();
    }

    fn analyse_game(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(game) = &self.game else { return };
        let mut tree = game.tree.clone();
        let player = self.model.read(cx).config.player_name.clone();
        let (white, black) = if let Some(white) = &game.white {
            (white.name.clone(), game.opponent_name.clone())
        } else if game.user == Color::White {
            (player, game.opponent_name.clone())
        } else {
            (game.opponent_name.clone(), player)
        };
        tree.set_header("White", &white);
        tree.set_header("Black", &black);
        // Deferred: opening a tab saves the session, which reads every Play tab,
        // including this one while it is still being updated.
        if let Some(workspace) = self.workspace.upgrade() {
            window.defer(cx, move |window, cx| {
                workspace.update(cx, |workspace, cx| {
                    workspace.open_analysis(tree, window, cx);
                });
            });
        }
    }

    fn render_setup(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let this = cx.entity();
        let mut grid = div().grid().grid_cols(3).gap(px(6.));
        for (index, (base, increment)) in TIME_CONTROLS.iter().enumerate() {
            let select_this = this.clone();
            grid = grid.child(
                btn(("time", index), format!("{base}+{increment}"))
                    .mono()
                    .full()
                    .h(36.)
                    .selected(self.time_index == Some(index))
                    .on_click(move |_, cx| {
                        select_this.update(cx, |screen, cx| {
                            screen.time_index = Some(index);
                            cx.notify();
                        });
                    }),
            );
        }
        let custom_this = this.clone();
        grid = grid.child(
            btn("time-custom", "Custom")
                .full()
                .selected(self.time_index.is_none())
                .on_click(move |_, cx| {
                    custom_this.update(cx, |screen, cx| {
                        screen.time_index = None;
                        cx.notify();
                    });
                }),
        );
        let colour_button = |id: &'static str,
                             label: &'static str,
                             choice: ColourChoice,
                             current: ColourChoice,
                             this: Entity<Self>| {
            btn(id, label)
                .flex_1()
                .selected(choice == current)
                .on_click(move |_, cx| {
                    this.update(cx, |screen, cx| {
                        screen.colour = choice;
                        cx.notify();
                    });
                })
        };
        let hide_this = this.clone();
        let takeback_this = this.clone();
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
        let number = self
            .chess960_number
            .read(cx)
            .value()
            .trim()
            .parse::<u16>()
            .ok()
            .filter(|n| *n < 960);
        let random_this = this.clone();
        let mode_this = this.clone();
        let mode = segmented(
            "play-mode",
            vec![
                (Mode::VsEngine, ui::i18n::t("Vs engine").into(), false),
                (Mode::Engines, ui::i18n::t("Engine vs engine").into(), false),
            ],
            self.mode,
            move |mode, _, cx| {
                mode_this.update(cx, |screen, cx| {
                    screen.mode = mode;
                    cx.notify();
                });
            },
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
            .border_r_1()
            .border_color(rgb(T.border))
            .child(section_label("New game").mb(px(10.)))
            .when(self_play && self.game.is_some(), |column| {
                let back_this = cx.entity();
                column.child(
                    btn("back-to-engine-game", "Back to game")
                        .compact()
                        .on_click(move |_, cx| {
                            back_this.update(cx, |screen, cx| {
                                screen.show_setup = false;
                                cx.notify();
                            });
                        }),
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
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(rgb(T.muted))
                            .mb(px(14.))
                            .child(ui::i18n::t(
                                "Optional local file. Uses the first game or position.",
                            )),
                    )
            })
            .when(self.time_index.is_none() && !self_play, |column| {
                column.child(div().mb(px(14.)).child(self.custom_time.render()))
            })
            .when(!self_play, |column| {
                column.child(
                    field(
                        "Colour",
                        h_flex()
                            .gap(px(6.))
                            .child(colour_button(
                                "white",
                                ui::i18n::t("White"),
                                ColourChoice::White,
                                self.colour,
                                this.clone(),
                            ))
                            .child(colour_button(
                                "random",
                                "Random",
                                ColourChoice::Random,
                                self.colour,
                                this.clone(),
                            ))
                            .child(colour_button(
                                "black",
                                ui::i18n::t("Black"),
                                ColourChoice::Black,
                                self.colour,
                                this.clone(),
                            )),
                    )
                    .mb(px(14.)),
                )
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
                column.child(
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
                                .on_click(move |window, cx| {
                                    random_this.update(cx, |screen, cx| {
                                        let number = crate::shell::random_chess960_number();
                                        screen.chess960_number.update(cx, |input, cx| {
                                            input.set_value(number.to_string(), window, cx)
                                        });
                                        cx.notify();
                                    });
                                }),
                        )
                        .child(mono(number.map_or_else(
                            || ui::i18n::t("Enter a number from 0 to 959").to_owned(),
                            |n| format!("#{n} {}", chess_core::chess960::back_rank(n)),
                        ))),
                )
            })
            .when(!self_play, |column| {
                column
                    .child(
                        div().mb(px(12.)).child(
                            check(
                                "hide-eval",
                                self.hide_eval,
                                "Hide evaluation during the game",
                            )
                            .on_change(move |value, _, cx| {
                                hide_this.update(cx, |screen, cx| {
                                    screen.hide_eval = value;
                                    cx.notify();
                                });
                            }),
                        ),
                    )
                    .child(div().mb(px(12.)).child({
                        let premove_this = cx.entity();
                        check("premoves", self.allow_premoves, "Allow premoves").on_change(
                            move |value, _, cx| {
                                premove_this.update(cx, |screen, cx| {
                                    screen.allow_premoves = value;
                                    cx.notify();
                                });
                            },
                        )
                    }))
                    .child(div().mb(px(20.)).child(
                        check("takebacks", self.allow_takebacks, "Allow takebacks").on_change(
                            move |value, _, cx| {
                                takeback_this.update(cx, |screen, cx| {
                                    screen.allow_takebacks = value;
                                    cx.notify();
                                });
                            },
                        ),
                    ))
            })
            .child(
                btn("start-game", "Start game")
                    .primary()
                    .full()
                    .h(40.)
                    .disabled(!can_start || self.book_task.is_some())
                    .on_click(move |window, cx| {
                        start_this.update(cx, |screen, cx| screen.start_with_book(window, cx));
                    }),
            )
            .when(!can_start, |column| {
                column.child(
                    div()
                        .mt(px(10.))
                        .text_size(px(13.))
                        .text_color(rgb(T.muted))
                        .child(ui::i18n::t("Add an engine in Settings › Engines to play.")),
                )
            })
    }

    /// One engine of the new game: which engine, and how strongly it plays.
    /// Shows a separate clock and unlimited-search choice for one engine.
    fn render_engine_time(&self, index: usize, cx: &mut Context<Self>) -> gpui::Div {
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
                .on_change(move |value, _, cx| {
                    this.update(cx, |screen, cx| {
                        screen.engine_times[index].unlimited = value;
                        cx.notify();
                    });
                }),
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

    fn render_choice(
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
                    let name_key = name;
                    column = column.child(
                        h_flex()
                            .gap(px(6.))
                            .mb(px(8.))
                            .child(div().flex_1().text_size(px(13.)).child(ui::i18n::t(name)))
                            .child({
                                let this = cx.entity();
                                btn(SharedString::from(format!("{label}-{name}-minus")), "−")
                                    .compact()
                                    .on_click(move |_, cx| {
                                        this.update(cx, |screen, cx| {
                                            let choice = pick(screen);
                                            let value = if name_key == "Threads" {
                                                &mut choice.threads
                                            } else {
                                                &mut choice.hash
                                            };
                                            *value = value.saturating_sub(step).max(1);
                                            cx.notify();
                                        });
                                    })
                            })
                            .child(mono(value.to_string()).w(px(44.)))
                            .child({
                                let this = cx.entity();
                                btn(SharedString::from(format!("{label}-{name}-plus")), "+")
                                    .compact()
                                    .on_click(move |_, cx| {
                                        this.update(cx, |screen, cx| {
                                            let choice = pick(screen);
                                            let value = if name_key == "Threads" {
                                                &mut choice.threads
                                            } else {
                                                &mut choice.hash
                                            };
                                            *value = value.saturating_add(step);
                                            cx.notify();
                                        });
                                    })
                            }),
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
                            .text_size(px(13.))
                            .text_color(rgb(T.muted))
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
                            .on_change(move |value, _, cx| {
                                this.update(cx, |screen, cx| {
                                    pick(screen).full = value;
                                    cx.notify();
                                });
                            }),
                        )
                    }),
            )
    }

    fn clock_bar(&self, opponent: bool, cx: &gpui::App) -> impl IntoElement + use<> {
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
                    .children(
                        rating
                            .map(|rating| mono(rating).text_size(px(13.)).text_color(rgb(T.muted))),
                    )
                    .children(status.map(|text| {
                        div()
                            .text_size(px(13.))
                            .text_color(rgb(T.muted))
                            .child(text)
                    }))
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

    fn render_center(
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

    /// Shows both engines' latest evaluations and principal variations beside the board.
    fn render_analysis(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let mut column = panel().p(px(16.)).child(section_label("Engines"));
        if let Some(game) = &self.game {
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
                        .border_b_1()
                        .border_color(rgb(T.divider))
                        .child(
                            h_flex()
                                .gap(px(8.))
                                .child(section_label(if index == 0 { "White" } else { "Black" }))
                                .child(div().text_size(px(13.)).child(name.to_owned())),
                        )
                        .child(
                            h_flex()
                                .gap(px(16.))
                                .child(mono(
                                    line.score
                                        .map_or_else(|| "—".to_owned(), |score| score.display()),
                                ))
                                .child(section_label("Depth"))
                                .child(mono(if line.depth == 0 {
                                    "—".to_owned()
                                } else {
                                    line.depth.to_string()
                                })),
                        )
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

    fn render_side(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let this = cx.entity();
        let game = self.game.as_ref();
        let mut list = v_flex().mt(px(8.)).font_family(MONO).text_size(px(14.));
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
            for (index, pair) in cells.chunks(2).enumerate() {
                let number = first_ply.div_ceil(2) as usize + index;
                let cell = |id: Option<NodeId>| {
                    let current = id == Some(self.viewed.unwrap_or(game.current));
                    let san = id
                        .map(|id| game.tree.node(id).san.clone())
                        .unwrap_or_default();
                    h_flex().flex_1().child(
                        div()
                            .flex_none()
                            .when(current, |pill| {
                                pill.px(px(6.))
                                    .rounded(px(4.))
                                    .bg(rgb(T.accent))
                                    .text_color(rgb(T.accent_fg))
                            })
                            .child(san),
                    )
                };
                list = list.child(
                    h_flex()
                        .h(px(27.))
                        .border_b_1()
                        .border_color(rgb(T.divider))
                        .child(
                            div()
                                .w(px(36.))
                                .flex_none()
                                .text_color(rgb(T.faint))
                                .child(format!("{number}.")),
                        )
                        .child(cell(pair[0]))
                        .child(cell(pair.get(1).copied().flatten())),
                );
            }
        }
        let result = game.and_then(|game| game.result.clone());
        let in_game = game.is_some_and(|game| game.result.is_none());
        let self_play = game.is_some_and(|game| game.self_play());
        let draw_this = this.clone();
        let takeback_this = this.clone();
        let resign_this = this.clone();
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
                    .child(
                        div()
                            .relative()
                            .child(
                                div()
                                    .id("play-moves-scroll")
                                    .max_h(px(220.))
                                    .overflow_y_scroll()
                                    .track_scroll(&self.moves_scroll)
                                    .pr(px(12.))
                                    .child(list),
                            )
                            .child(
                                Scrollbar::vertical(&self.moves_scroll).mode(ScrollbarMode::Always),
                            ),
                    )
                    .child(
                        div()
                            .mt(px(10.))
                            .text_size(px(13.))
                            .text_color(rgb(T.muted))
                            .child(opening_text),
                    )
                    .children(result.map(|text| {
                        div()
                            .mt(px(8.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(text)
                    })),
            )
            .child(if self_play {
                h_flex().child(
                    btn("stop-self-play", "Stop")
                        .flex_1()
                        .disabled(!in_game)
                        .on_click(move |_, cx| {
                            resign_this.update(cx, |screen, cx| screen.resign(cx));
                        }),
                )
            } else {
                h_flex()
                    .gap(px(6.))
                    .child(
                        btn("offer-draw", ui::i18n::t("Offer draw"))
                            .flex_1()
                            .disabled(!in_game)
                            .on_click(move |_, cx| {
                                draw_this.update(cx, |screen, cx| screen.offer_draw(cx));
                            }),
                    )
                    .child(
                        btn("takeback", "Takeback")
                            .flex_1()
                            .disabled(!in_game)
                            .on_click(move |_, cx| {
                                takeback_this.update(cx, |screen, cx| screen.takeback(cx));
                            }),
                    )
                    .child(
                        btn("resign", ui::i18n::t("Resign"))
                            .flex_1()
                            .disabled(!in_game)
                            .on_click(move |_, cx| {
                                resign_this.update(cx, |screen, cx| screen.resign(cx));
                            }),
                    )
            })
            .child(
                btn("analyse-game", "Analyse this game")
                    .full()
                    .h(40.)
                    .disabled(game.is_none())
                    .on_click(move |window, cx| {
                        this.update(cx, |screen, cx| screen.analyse_game(window, cx));
                    }),
            )
    }
}

/// Steps along the played game rather than a side variation or an engine PV.
fn review_step(tree: &GameTree, live: NodeId, viewed: NodeId, delta: i32) -> Option<NodeId> {
    if delta < 0 {
        tree.node(viewed).parent
    } else {
        tree.path_to(live)
            .into_iter()
            .find(|&id| tree.node(id).parent == Some(viewed))
    }
}

/// Starts a match at the first PGN line or EPD position in a local book.
fn opening_from_book(path: &str, text: &str) -> Result<GameTree, String> {
    if path.to_ascii_lowercase().ends_with(".pgn") {
        chess_core::parse_pgn(text)
            .map_err(|err| err.to_string())?
            .into_iter()
            .next()
            .ok_or_else(|| "The opening book is empty.".to_owned())
    } else if path.to_ascii_lowercase().ends_with(".epd") {
        let record = chess_core::parse_epd(text)
            .into_iter()
            .next()
            .ok_or_else(|| "The opening book is empty.".to_owned())?
            .parsed?;
        GameTree::from_fen(&record.fen).map_err(|err| err.to_string())
    } else {
        Err("Choose a PGN or EPD opening book.".to_owned())
    }
}

fn supports_limit(config: &EngineConfig) -> bool {
    config
        .info
        .options
        .iter()
        .any(|option| option.name == "UCI_Elo")
        || config.name.eq_ignore_ascii_case("stockfish")
}

fn opponent_names(engines: &[EngineConfig]) -> Vec<SharedString> {
    if engines.is_empty() {
        return vec![ui::i18n::t("No engine configured").into()];
    }
    engines
        .iter()
        .map(|engine| {
            let limited = engine.info.options.iter().any(|option| {
                option.name == "UCI_Elo" && matches!(option.kind, OptionKind::Spin { .. })
            }) || engine.name.eq_ignore_ascii_case("stockfish");
            let version = if engine.version.is_empty() {
                String::new()
            } else {
                format!(" {}", engine.version)
            };
            let suffix = if limited { ", limited strength" } else { "" };
            format!("{}{version}{suffix}", engine.name).into()
        })
        .collect()
}

impl Render for PlayScreen {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // An engine game left unfinished last time opens in Analyse, which keeps it from
        // then on; the engine is not restarted.
        if let Some(recovery) = self.recovery.take() {
            match self.workspace.upgrade() {
                Some(workspace) => {
                    let pgn = recovery.pgn;
                    let path = self.recovery_path.clone();
                    window.defer(cx, move |window, cx| {
                        workspace.update(cx, |workspace, cx| {
                            workspace.open_pgn(&pgn, 0, window, cx);
                        });
                        if let Err(err) = crate::recovery::Recovery::discard(&path) {
                            log::error!("could not remove the Play autosave: {err}");
                        }
                    });
                }
                None => self.recovery = Some(recovery),
            }
        }
        if self.mode == Mode::Engines {
            let viewport = window.viewport_size();
            let setup_open = self.show_setup || self.game.is_none();
            let board_size = ui::board::snap_board_size(
                (viewport.width - px(if setup_open { 636. } else { 440. }))
                    .min(viewport.height - px(300.))
                    .max(px(320.)),
                window.scale_factor(),
            );
            let center = self.render_center(board_size, cx);
            let side = self.render_side(cx);
            let this = cx.entity();
            let mut layout = h_flex().size_full().items_start();
            if setup_open {
                layout = layout.child(self.render_setup(cx));
            }
            return layout
                .child(div().px(px(24.)).flex_none().child(center))
                .when(!setup_open, |row| {
                    row.child(
                        v_flex()
                            .id("engine-match-side")
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .overflow_y_scroll()
                            .p(px(16.))
                            .gap(px(16.))
                            .child(btn("new-engine-game", "New game").compact().on_click(
                                move |_, cx| {
                                    this.update(cx, |screen, cx| {
                                        screen.show_setup = true;
                                        cx.notify();
                                    });
                                },
                            ))
                            .child(self.render_analysis(cx))
                            .child(side),
                    )
                })
                .children(self.pv_at.map(|at| {
                    deferred(
                        anchored()
                            .position(at + point(px(16.), px(16.)))
                            .snap_to_window_with_margin(px(8.))
                            .child(
                                div()
                                    .size(ui::board::snap_board_size(
                                        px(256.),
                                        window.scale_factor(),
                                    ))
                                    .border_1()
                                    .border_color(rgb(T.border))
                                    .child(self.pv_board.clone()),
                            ),
                    )
                    .with_priority(1)
                }))
                .into_any_element();
        }
        let board_size = super::side_columns_board_size(window);
        let setup = self.render_setup(cx);
        let center = self.render_center(board_size, cx);
        let side = self.render_side(cx);
        super::three_columns(setup, "play-center", center, "play-side", side)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Browsing follows the played line even while new moves and side lines are added.
    #[test]
    fn review_tracks_the_live_line_without_changing_it() {
        let mut tree = GameTree::new();
        let e4 = tree.add_san(GameTree::ROOT, "e4").unwrap();
        let c5 = tree.add_san(e4, "c5").unwrap();
        let e5 = tree.add_san(e4, "e5").unwrap();
        assert_eq!(review_step(&tree, e5, e4, 1), Some(e5));
        assert_eq!(review_step(&tree, c5, e4, 1), Some(c5));
        assert_eq!(review_step(&tree, e5, e5, -1), Some(e4));
        assert_eq!(review_step(&tree, e5, GameTree::ROOT, -1), None);
        assert_eq!(review_step(&tree, e5, e5, 1), None);
        let nf3 = tree.add_san(e5, "Nf3").unwrap();
        assert_eq!(review_step(&tree, nf3, e4, 1), Some(e5));
        assert_eq!(review_step(&tree, nf3, e5, 1), Some(nf3));
        assert_eq!(tree.node(nf3).san, "Nf3");
    }

    /// Books seed the played line and reject empty or unsupported files.
    #[test]
    fn opening_books_seed_matches() {
        let tree = opening_from_book("opening.pgn", "1. e4 e5 2. Nf3 *").unwrap();
        assert_eq!(tree.line_from(GameTree::ROOT).len(), 3);
        let tree =
            opening_from_book("opening.epd", "8/8/8/8/8/4k3/8/4K3 w - - id \"test\";").unwrap();
        assert_eq!(tree.line_from(GameTree::ROOT).len(), 0);
        assert!(opening_from_book("empty.epd", "").is_err());
        assert!(opening_from_book("book.bin", "anything").is_err());
    }

    #[test]
    fn clocks_format_and_round_up() {
        assert_eq!(format_clock(552_000), "09:12");
        assert_eq!(format_clock(551_001), "09:12");
        assert_eq!(format_clock(0), "00:00");
        assert_eq!(format_clock(3_661_000), "1:01:01");
    }
}
