//! Play against a UCI engine (or a scripted opponent in demo mode), or watch two engines
//! play each other.

use std::time::{Duration, Instant};

use chess_core::{Color, GameTree, Move, NodeId, Position, classify, shakmaty::uci::UciMove};
use engine::{
    EngineConfig, EngineEvent, EngineHandle, EngineLog, Limits, LogLine, OptionKind, SearchRequest,
};
use gpui::{
    AppContext as _, Context, Entity, FontWeight, IntoElement, ListHorizontalSizingBehavior,
    Pixels, Point, Render, ScrollHandle, ScrollStrategy, SharedString, Subscription, Task,
    UniformListScrollHandle, WeakEntity, Window, anchored, deferred, div, point, prelude::*, px,
    rgb, uniform_list,
};
use gpui_component::{
    scroll::{ScrollableElement as _, Scrollbar, ScrollbarMode},
    select::SelectEvent,
    slider::{Slider, SliderEvent, SliderState},
};
use ui::{
    BoardEvent, BoardView, MONO, T,
    widgets::{btn, check, h_flex, header_tab, mono, panel, section_label, segmented, v_flex},
};

use crate::{
    controls::{TextSelect, field, new_input, new_select, select_el, text_input},
    model::{AppModel, ConfigExt as _, ModelEvent, ToastExt as _},
    shell::Workspace,
};

pub use self::side::format_clock;
use self::{
    pgn::{
        engine_game_name, match_move_comment, match_move_values, opponent_names, set_game_headers,
    },
    setup::supports_limit,
};

mod game;
mod pgn;
mod setup;
mod side;

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

/// A PV move and its resulting position for the hover board.
struct PvMove {
    label: String,
    position: chess_core::Chess,
    last: Option<(chess_core::Square, chess_core::Square)>,
}

/// The latest search line for one side, kept visible while the other side thinks.
#[derive(Default)]
struct GameAnalysis {
    nodes: Option<u64>,
    search_id: Option<u64>,
    depth: u32,
    seldepth: Option<u32>,
    /// Transposition table use, in permille.
    hashfull: Option<u32>,
    /// How long the engine thought about its last move.
    think_ms: Option<u64>,
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
    elapsed_ms: u64,
    search_id: u64,
    analysis: [GameAnalysis; 2],
    engine: Option<EngineHandle>,
    /// White's and Black's UCI traffic, kept after the engines quit.
    logs: [Option<EngineLog>; 2],
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
    graph_metric: usize,
    hide_eval: bool,
    allow_takebacks: bool,
    allow_premoves: bool,
    game: Option<Game>,
    /// The FEN of the start position shown on the board while a new game is set up.
    preview: Option<String>,
    /// Replies scheduled from the demo opponent.
    demo_task: Option<Task<()>>,
    /// The engine match's Log panel is open.
    log_open: bool,
    /// Whose log the panel shows: 0 for White, 1 for Black.
    log_side: usize,
    log_follow: bool,
    log_lines: Vec<LogLine>,
    /// What `log_lines` was taken from: side and log revision.
    log_key: Option<(usize, u64)>,
    log_scroll: UniformListScrollHandle,
    _log_poll: Task<()>,
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
            cx.subscribe(
                &start_fen,
                |_, _, _: &gpui_component::input::InputEvent, cx| cx.notify(),
            ),
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
            graph_metric: 0,
            hide_eval: true,
            allow_takebacks: false,
            allow_premoves: true,
            game: None,
            preview: None,
            demo_task: None,
            log_open: false,
            log_side: 0,
            log_follow: true,
            log_lines: Vec::new(),
            log_key: None,
            log_scroll: UniformListScrollHandle::new(),
            _log_poll: cx.spawn(async move |this: WeakEntity<Self>, cx| {
                loop {
                    cx.background_executor()
                        .timer(Duration::from_millis(200))
                        .await;
                    let alive = this.update(cx, |this, cx| {
                        if this.log_open && this.log_key != this.current_log_key() {
                            cx.notify();
                        }
                    });
                    if alive.is_err() {
                        return;
                    }
                }
            }),
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

    /// Shows the chosen start position, such as a Chess960 setup, while a new game is
    /// being set up, and the game again once setup is left.
    fn refresh_preview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let previewing = match &self.game {
            None => true,
            Some(game) => self.mode == Mode::Engines && self.show_setup && game.result.is_some(),
        };
        if !previewing {
            if self.preview.take().is_some() {
                self.sync_board(false, cx);
            }
            return;
        }
        // An invalid FEN or Chess960 number keeps the last valid preview.
        let Some(tree) = self.start_tree(window, cx) else {
            return;
        };
        let last = tree
            .line_from(GameTree::ROOT)
            .last()
            .copied()
            .unwrap_or(GameTree::ROOT);
        let fen = tree.fen(last);
        if self.preview.as_ref() == Some(&fen) {
            return;
        }
        self.preview = Some(fen);
        let pos = tree.node(last).pos.clone();
        self.board.update(cx, |board, cx| {
            board.set_position(pos, None, false, cx);
            board.set_interactive(false, cx);
            board.set_premove(None, cx);
        });
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
        self.refresh_preview(window, cx);
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
            let side = self.render_side(false, cx);
            let in_game = self.game.as_ref().is_some_and(|game| game.result.is_none());
            let log = (self.log_open && !setup_open).then(|| self.render_log(cx));
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
                            .child(
                                h_flex()
                                    .gap(px(6.))
                                    .child(btn("new-engine-game", "New game").compact().on_click(
                                        move |_, cx| {
                                            this.update(cx, |screen, cx| {
                                                screen.new_engine_game(cx)
                                            });
                                        },
                                    ))
                                    .child({
                                        let this = cx.entity();
                                        btn("stop-self-play", "Stop")
                                            .compact()
                                            .disabled(!in_game)
                                            .on_click(move |_, cx| {
                                                this.update(cx, |screen, cx| screen.resign(cx));
                                            })
                                    })
                                    .child({
                                        let this = cx.entity();
                                        btn("analyse-game", "Analyse this game")
                                            .compact()
                                            .disabled(self.game.is_none())
                                            .on_click(move |window, cx| {
                                                this.update(cx, |screen, cx| {
                                                    screen.analyse_game(window, cx)
                                                });
                                            })
                                    }),
                            )
                            .child(self.render_graph(cx))
                            .child(self.render_analysis(cx))
                            .children(log)
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
        let side = self.render_side(true, cx);
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
}
