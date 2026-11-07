//! Play against a UCI engine (or a scripted opponent in demo mode).

use std::time::{Duration, Instant};

use chess_core::{
    Chess, Color, GameTree, Move, NodeId, Position, classify, shakmaty::uci::UciMove,
};
use engine::{EngineConfig, EngineEvent, EngineHandle, Limits, OptionKind, SearchRequest};
use gpui::{
    AppContext as _, Context, Entity, FontWeight, IntoElement, Pixels, Render, SharedString,
    Subscription, Task, WeakEntity, Window, div, prelude::*, px, rgb,
};
use gpui_component::{
    select::SelectEvent,
    slider::{Slider, SliderEvent, SliderState},
};
use ui::{
    BoardEvent, BoardView, MONO, T,
    widgets::{btn, check, h_flex, mono, panel, section_label, v_flex},
};

use crate::{
    controls::{TextSelect, field, new_input, new_select, select_el, text_input},
    model::{AppModel, ModelEvent},
    shell::{Mode, Workspace},
};

const TIME_CONTROLS: [(u32, u32); 5] = [(3, 2), (5, 0), (10, 5), (15, 10), (30, 0)];

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

/// Parses "12+3" into base minutes and increment seconds.
pub fn parse_custom_time(text: &str) -> Option<(u32, u32)> {
    let (base, increment) = text.trim().split_once('+')?;
    let base: u32 = base.trim().parse().ok()?;
    let increment: u32 = increment.trim().parse().ok()?;
    (base > 0).then_some((base, increment))
}

struct Game {
    tree: GameTree,
    current: NodeId,
    user: Color,
    white_ms: u64,
    black_ms: u64,
    increment_ms: u64,
    result: Option<String>,
    opponent_name: String,
    opponent_rating: Option<u32>,
    label: String,
    takebacks: bool,
    last_tick: Instant,
    search_id: u64,
    engine: Option<EngineHandle>,
    _engine_reader: Option<Task<()>>,
    _clock: Option<Task<()>>,
}

pub struct PlayScreen {
    model: Entity<AppModel>,
    workspace: WeakEntity<Workspace>,
    board: Entity<BoardView>,
    opponent: TextSelect,
    opponents: Vec<EngineConfig>,
    strength: Entity<SliderState>,
    strength_value: u32,
    time_index: Option<usize>,
    custom_time: Entity<gpui_component::input::InputState>,
    colour: ColourChoice,
    start_from: TextSelect,
    hide_eval: bool,
    allow_takebacks: bool,
    game: Option<Game>,
    /// Replies scheduled from the demo opponent.
    demo_task: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl PlayScreen {
    pub fn new(
        model: Entity<AppModel>,
        workspace: WeakEntity<Workspace>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let board = cx.new(|_| BoardView::new("play-board"));
        let prefs = model.read(cx).config.board.prefs();
        board.update(cx, |board, cx| board.set_prefs(prefs, cx));
        let opponents = model.read(cx).play_engines();
        let names = opponent_names(&opponents);
        let opponent = new_select(names, 0, window, cx);
        let start_from = new_select(
            [
                "Starting position",
                "Current Analyse position",
                "Repertoire line",
            ]
            .map(SharedString::from)
            .to_vec(),
            0,
            window,
            cx,
        );
        let strength = cx.new(|_| {
            SliderState::new()
                .min(1320.)
                .max(3190.)
                .step(10.)
                .default_value(1800.)
        });
        let custom_time = new_input("", "12+3", window, cx);
        let subscriptions = vec![
            cx.subscribe_in(&board, window, |this, _, event, _, cx| match event {
                BoardEvent::Moved(mv) => this.user_move(*mv, cx),
            }),
            cx.subscribe(&strength, |this, _, event: &SliderEvent, cx| {
                if let SliderEvent::Change(value) = event {
                    this.strength_value = value.start().round() as u32;
                    cx.notify();
                }
            }),
            cx.subscribe(&opponent, |_, _, _: &SelectEvent<_>, cx| cx.notify()),
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
                        this.opponent = new_select(names, 0, window, cx);
                        cx.notify();
                    }
                    _ => {}
                },
            ),
        ];
        let demo = model.read(cx).demo;
        let mut this = Self {
            model,
            workspace,
            board,
            opponent,
            opponents,
            strength,
            strength_value: 1800,
            time_index: Some(2),
            custom_time,
            colour: ColourChoice::White,
            start_from,
            hide_eval: true,
            allow_takebacks: false,
            game: None,
            demo_task: None,
            _subscriptions: subscriptions,
        };
        if demo {
            this.start_demo_game(cx);
        }
        this
    }

    pub fn breadcrumb(&self) -> SharedString {
        match &self.game {
            Some(game) => format!("vs {}, {}", game.opponent_name, game.label).into(),
            None => "New game".into(),
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
        self.game = Some(Game {
            tree,
            current: node,
            user: Color::White,
            white_ms: 552_000,
            black_ms: 521_000,
            increment_ms: 5_000,
            result: None,
            opponent_name: "Stockfish".to_owned(),
            opponent_rating: Some(self.strength_value),
            label: "10+5".to_owned(),
            takebacks: false,
            last_tick: Instant::now(),
            search_id: 0,
            engine: None,
            _engine_reader: None,
            _clock: None,
        });
        self.sync_board(false, cx);
        self.start_clock(cx);
    }

    fn time_control(&self, cx: &gpui::App) -> Option<(u32, u32)> {
        match self.time_index {
            Some(index) => TIME_CONTROLS.get(index).copied(),
            None => parse_custom_time(self.custom_time.read(cx).value().as_ref()),
        }
    }

    fn opponent_config(&self, cx: &gpui::App) -> Option<EngineConfig> {
        let index = self.opponent.read(cx).selected_index(cx)?.row;
        self.opponents.get(index).cloned()
    }

    fn start_tree(&self, window: &mut Window, cx: &mut Context<Self>) -> GameTree {
        let choice = self
            .start_from
            .read(cx)
            .selected_index(cx)
            .map_or(0, |index| index.row);
        let _ = window;
        match choice {
            1 => self
                .workspace
                .upgrade()
                .and_then(|workspace| {
                    let analyse = workspace.read(cx).analyse.read(cx);
                    GameTree::from_fen(&analyse.tree.fen(analyse.current)).ok()
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
            _ => GameTree::new(),
        }
    }

    fn start_game(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((minutes, increment)) = self.time_control(cx) else {
            self.toast("Enter a custom time such as 12+3.", cx);
            return;
        };
        let demo = self.model.read(cx).demo;
        let config = self.opponent_config(cx);
        let Some(config) = config else {
            self.toast("Add an engine for Play in Settings › Engines first.", cx);
            return;
        };
        let tree = self.start_tree(window, cx);
        let current = tree
            .line_from(GameTree::ROOT)
            .last()
            .copied()
            .unwrap_or(GameTree::ROOT);
        let user = match self.colour {
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
        let ms = u64::from(minutes) * 60_000;
        let limited = supports_limit(&config);
        let mut game = Game {
            tree,
            current,
            user,
            white_ms: ms,
            black_ms: ms,
            increment_ms: u64::from(increment) * 1000,
            result: None,
            opponent_name: config.name.clone(),
            opponent_rating: limited.then_some(self.strength_value),
            label: format!("{minutes}+{increment}"),
            takebacks: self.allow_takebacks,
            last_tick: Instant::now(),
            search_id: 0,
            engine: None,
            _engine_reader: None,
            _clock: None,
        };
        if !demo {
            // Analysis-only settings would just weaken or slow the opponent's search.
            let mut options: Vec<(String, String)> = config
                .options
                .iter()
                .filter(|(name, _)| !matches!(name.as_str(), "MultiPV" | "UCI_ShowWDL"))
                .map(|(name, value)| (name.clone(), value.clone()))
                .collect();
            if limited {
                options.push(("UCI_LimitStrength".to_owned(), "true".to_owned()));
                options.push(("UCI_Elo".to_owned(), self.strength_value.to_string()));
            }
            let handle = EngineHandle::spawn(config.path.clone(), Vec::new(), options);
            handle.new_game();
            game._engine_reader = Some(self.spawn_reader(&handle, cx));
            game.engine = Some(handle);
        }
        if let Some(old) = self.game.replace(game).and_then(|game| game.engine) {
            old.quit();
        }
        self.board
            .update(cx, |board, cx| board.set_flipped(user == Color::Black, cx));
        self.sync_board(false, cx);
        self.start_clock(cx);
        self.maybe_request_engine_move(cx);
        cx.notify();
    }

    fn spawn_reader(&self, handle: &EngineHandle, cx: &mut Context<Self>) -> Task<()> {
        let events = handle.events();
        cx.spawn(async move |this, cx| {
            while let Ok(event) = events.recv().await {
                let alive = this.update(cx, |this, cx| match event {
                    EngineEvent::BestMove { search_id, best } => {
                        this.engine_move(search_id, best, cx);
                    }
                    EngineEvent::Crashed(reason) => {
                        this.toast(format!("The engine stopped: {reason}"), cx);
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
        if let Some(engine) = game.engine.take() {
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
        let node = game.tree.node(game.current);
        let pos = node.pos.clone();
        let last = node.mv.as_ref().and_then(|mv| Some((mv.from()?, mv.to())));
        let user = game.user;
        let active = game.result.is_none();
        self.board.update(cx, |board, cx| {
            board.set_position(pos, last, animate, cx);
            board.set_player(Some(user), cx);
            board.set_interactive(active, cx);
        });
    }

    /// Applies a move from either side: clock, tree, and game-end detection.
    fn apply_move(&mut self, mv: Move, cx: &mut Context<Self>) {
        self.tick(cx);
        let Some(game) = &mut self.game else { return };
        let turn = game.tree.node(game.current).pos.turn();
        if turn == Color::White {
            game.white_ms += game.increment_ms;
        } else {
            game.black_ms += game.increment_ms;
        }
        game.current = game.tree.add_move(game.current, mv);
        let pos: Chess = game.tree.node(game.current).pos.clone();
        self.sync_board(true, cx);
        if pos.is_checkmate() {
            let text = if pos.turn() == Color::White {
                "Black wins by checkmate"
            } else {
                "White wins by checkmate"
            };
            self.finish(text.to_owned(), cx);
        } else if pos.is_stalemate() {
            self.finish("Stalemate: draw".to_owned(), cx);
        } else if pos.is_insufficient_material() {
            self.finish("Draw by insufficient material".to_owned(), cx);
        }
    }

    fn user_move(&mut self, mv: Move, cx: &mut Context<Self>) {
        let Some(game) = &self.game else { return };
        let turn = game.tree.node(game.current).pos.turn();
        if game.result.is_some() || turn != game.user {
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
        if pos.turn() == game.user {
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
                let _ = this.update(cx, |this, cx| this.apply_move(mv, cx));
            }));
            return;
        }
        let Some(engine) = &game.engine else { return };
        game.search_id += 1;
        let moves = game
            .tree
            .path_to(game.current)
            .iter()
            .filter_map(|&id| game.tree.node(id).mv)
            .map(|mv| UciMove::from_standard(mv).to_string())
            .collect();
        engine.search(SearchRequest {
            fen: game.tree.fen(GameTree::ROOT),
            moves,
            limits: Some(Limits {
                white_ms: Some(game.white_ms),
                black_ms: Some(game.black_ms),
                white_inc_ms: game.increment_ms,
                black_inc_ms: game.increment_ms,
                ..Default::default()
            }),
            search_id: game.search_id,
        });
    }

    fn engine_move(&mut self, search_id: u64, best: Option<String>, cx: &mut Context<Self>) {
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
            Some(mv) => self.apply_move(mv, cx),
            None => {
                self.toast("The engine returned no legal move.", cx);
                self.finish("Engine stopped".to_owned(), cx);
            }
        }
    }

    fn resign(&mut self, cx: &mut Context<Self>) {
        let Some(game) = &self.game else { return };
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
            self.toast(format!("{} declines the draw.", game.opponent_name), cx);
        }
    }

    fn takeback(&mut self, cx: &mut Context<Self>) {
        let Some(game) = &mut self.game else { return };
        if !game.takebacks || game.result.is_some() {
            self.toast("Takebacks are off for this game.", cx);
            return;
        }
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
        let (white, black) = if game.user == Color::White {
            (player, game.opponent_name.clone())
        } else {
            (game.opponent_name.clone(), player)
        };
        tree.set_header("White", &white);
        tree.set_header("Black", &black);
        if let Some(workspace) = self.workspace.upgrade() {
            workspace.update(cx, |workspace, cx| {
                workspace.analyse.update(cx, |analyse, cx| {
                    analyse.load_tree(tree, window, cx);
                });
                workspace.set_mode(Mode::Analyse, cx);
            });
        }
    }

    fn toast(&self, text: impl Into<SharedString>, cx: &mut Context<Self>) {
        let text = text.into();
        self.model.update(cx, |model, cx| model.toast(text, cx));
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
        v_flex()
            .w(px(300.))
            .h_full()
            .flex_none()
            .px(px(18.))
            .pt(px(20.))
            .bg(rgb(T.surface))
            .border_r_1()
            .border_color(rgb(T.border))
            .child(section_label("New game vs engine").mb(px(14.)))
            .child(field("Opponent", select_el(&self.opponent)).mb(px(14.)))
            .child(
                v_flex()
                    .mb(px(16.))
                    .gap(px(6.))
                    .child(
                        h_flex()
                            .justify_between()
                            .text_size(px(13.))
                            .text_color(rgb(T.muted))
                            .child("Strength")
                            .child(mono(self.strength_value.to_string()).text_color(rgb(T.fg_2))),
                    )
                    .child(Slider::new(&self.strength).bg(rgb(T.fg))),
            )
            .child(field("Time control", grid).mb(px(14.)))
            .when(self.time_index.is_none(), |column| {
                column.child(div().mb(px(14.)).child(text_input(&self.custom_time)))
            })
            .child(
                field(
                    "Colour",
                    h_flex()
                        .gap(px(6.))
                        .child(colour_button(
                            "white",
                            "White",
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
                            "Black",
                            ColourChoice::Black,
                            self.colour,
                            this.clone(),
                        )),
                )
                .mb(px(14.)),
            )
            .child(field("Start from", select_el(&self.start_from)).mb(px(20.)))
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
            .child(
                btn("start-game", "Start game")
                    .primary()
                    .full()
                    .h(40.)
                    .disabled(!can_start)
                    .on_click(move |window, cx| {
                        start_this.update(cx, |screen, cx| screen.start_game(window, cx));
                    }),
            )
            .when(!can_start, |column| {
                column.child(
                    div()
                        .mt(px(10.))
                        .text_size(px(13.))
                        .text_color(rgb(T.muted))
                        .child("Add an engine in Settings › Engines to play."),
                )
            })
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
            (true, None) => self
                .opponents
                .first()
                .map_or_else(|| "Opponent".into(), |engine| engine.name.clone().into()),
            (false, _) => "You".into(),
        };
        let rating = if opponent {
            game.and_then(|game| game.opponent_rating)
                .map(|rating| rating.to_string())
        } else {
            self.model
                .read(cx)
                .config
                .player_rating
                .map(|rating| rating.to_string())
        };
        let status = (!opponent && active).then_some("your move");
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
                            .border_color(rgb(if active { T.fg } else { T.muted }))
                            .when(active, |dot| dot.bg(rgb(T.fg))),
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
                    })),
            )
            .child(
                div()
                    .font_family(MONO)
                    .text_size(px(24.))
                    .font_weight(FontWeight::MEDIUM)
                    .px(px(9.))
                    .rounded(px(4.))
                    .when(active, |time| time.bg(rgb(T.fg)).text_color(rgb(T.bg)))
                    .when(!active, |time| time.text_color(rgb(T.muted)))
                    .child(if game.is_some() {
                        format_clock(ms)
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
                    let current = id == Some(game.current);
                    let san = id
                        .map(|id| game.tree.node(id).san.clone())
                        .unwrap_or_default();
                    h_flex().flex_1().child(
                        div()
                            .flex_none()
                            .when(current, |pill| {
                                pill.px(px(6.))
                                    .rounded(px(4.))
                                    .bg(rgb(T.fg))
                                    .text_color(rgb(T.bg))
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
        let draw_this = this.clone();
        let takeback_this = this.clone();
        let resign_this = this.clone();
        v_flex()
            .gap(px(16.))
            .child(
                panel()
                    .p(px(16.))
                    .child(section_label("Moves"))
                    .child(list)
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
            .child(
                h_flex()
                    .gap(px(6.))
                    .child(
                        btn("offer-draw", "Offer draw")
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
                        btn("resign", "Resign")
                            .flex_1()
                            .disabled(!in_game)
                            .on_click(move |_, cx| {
                                resign_this.update(cx, |screen, cx| screen.resign(cx));
                            }),
                    ),
            )
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
        return vec!["No engine configured".into()];
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
        let viewport = window.viewport_size();
        let board_size = (viewport.height - px(324.))
            .min(viewport.width - px(720.))
            .max(px(320.));
        let setup = self.render_setup(cx);
        let center = self.render_center(board_size, cx);
        let side = self.render_side(cx);
        h_flex()
            .items_start()
            .size_full()
            .child(setup)
            .child(div().px(px(28.)).h_full().flex_none().child(center))
            .child(
                div()
                    .id("play-side")
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .overflow_y_scroll()
                    .pt(px(24.))
                    .pr(px(24.))
                    .pb(px(16.))
                    .child(side),
            )
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

    #[test]
    fn custom_time_parses() {
        assert_eq!(parse_custom_time("12+3"), Some((12, 3)));
        assert_eq!(parse_custom_time(" 5 + 0 "), Some((5, 0)));
        assert_eq!(parse_custom_time("0+3"), None);
        assert_eq!(parse_custom_time("nonsense"), None);
    }
}
