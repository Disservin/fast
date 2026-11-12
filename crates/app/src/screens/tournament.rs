//! Engine tournaments run by fastchess: set one up in a form, paste a command line or a
//! saved config to continue, and follow the results live.

use std::{
    collections::{HashMap, VecDeque},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, atomic::AtomicBool},
    time::{Duration, Instant},
};

use chess_core::{Position as _, format_pv, pv_to_san};

use engine::EngineId;
use gpui::{
    AnyElement, AppContext as _, ClipboardItem, Context, Div, Entity, FontWeight, IntoElement,
    Pixels, Render, SharedString, Subscription, Task, WeakEntity, Window, canvas, deferred, div,
    prelude::*, px, relative, rgb,
};
use gpui_component::{
    input::{InputState, Textarea, TextareaState},
    scroll::ScrollableElement as _,
};
use ui::{
    BoardView, MONO, T,
    widgets::{
        badge, btn, check, divider, h_flex, meter, mono, panel, section_label, segmented, v_flex,
    },
};

use crate::{
    controls::{TextSelect, field, new_input, new_select, select_el, text_input, toggle},
    fastchess::{self, Limit, Line, Participant, Pasted, PgnOut, Setup, StopHandle},
    model::{AppModel, ModelEvent, ToastExt as _, pick_path},
    shell::Workspace,
    tournament_live::{self, LiveGame, LiveGames, Score, Search},
    tournament_wall::{self, LiveWall},
};

pub(crate) use self::live_view::{eval_graph, game_card, search_stats};
use self::{live_view::*, widgets::*};
use crate::fastchess::{engines, flag_value, flag_values, has, kv};

mod fastchess_setup;
mod form;
mod live_view;
mod results;
mod run;
mod setup;
mod widgets;

const MAX_LOG: usize = 400;
/// Live boards shown before "Show all".
const LIVE_LIMIT: usize = 6;
const LIVE_REFRESH: Duration = Duration::from_millis(200);
const MAX_GAMES: usize = 500;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Source {
    Form,
    Paste,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Status {
    Running,
    Stopping,
    Finished(String),
    Failed(String),
}

/// The filter above the games table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GameFilter {
    All,
    Decisive,
    Adjudicated,
    Repetition,
}

impl GameFilter {
    const ALL: [Self; 4] = [
        Self::All,
        Self::Decisive,
        Self::Adjudicated,
        Self::Repetition,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::All => ui::i18n::t("All"),
            Self::Decisive => ui::i18n::t("Decisive"),
            Self::Adjudicated => ui::i18n::t("Adjudicated"),
            Self::Repetition => ui::i18n::t("Repetition"),
        }
    }

    fn matches(self, row: &GameRow) -> bool {
        let reason = row.reason.to_lowercase();
        match self {
            Self::All => true,
            Self::Decisive => row.result != "1/2-1/2",
            Self::Adjudicated => reason.contains("adjudicat"),
            Self::Repetition => reason.contains("repetition"),
        }
    }
}

/// A column the finished games table can be ordered by.
#[derive(Clone, Copy, PartialEq, Eq)]
enum GameSort {
    Number,
    White,
    Black,
    Result,
    Termination,
}

impl GameSort {
    fn compare(self, a: &GameRow, b: &GameRow) -> std::cmp::Ordering {
        // White's score, so 1-0 > 1/2-1/2 > 0-1; anything else (an unfinished `*`) last.
        let score = |row: &GameRow| match row.result.as_str() {
            "1-0" => 2,
            "1/2-1/2" => 1,
            "0-1" => 0,
            _ => -1,
        };
        let text = |a: &str, b: &str| a.to_lowercase().cmp(&b.to_lowercase());
        match self {
            Self::Number => a.game.cmp(&b.game),
            Self::White => text(&a.white, &b.white),
            Self::Black => text(&a.black, &b.black),
            Self::Result => score(a).cmp(&score(b)),
            Self::Termination => text(&a.reason, &b.reason),
        }
        .then(a.game.cmp(&b.game))
    }
}

/// Live boards are this many pixels square in the tournament tab.
const LIVE_BOARD: f32 = 280.;
/// Space between live cards, and what a card adds to its board's width.
const LIVE_GAP: f32 = 12.;
const LIVE_CARD_EXTRA: f32 = 40.;
/// The finished games table scrolls past this height.
const GAMES_TABLE_H: f32 = 520.;

struct GameRow {
    game: u64,
    white: String,
    black: String,
    result: String,
    reason: String,
    /// Position of this game in the PGN file, which fastchess writes in finishing order.
    pgn_index: usize,
}

struct Run {
    dir: PathBuf,
    cwd: PathBuf,
    config: PathBuf,
    program: PathBuf,
    args: Vec<String>,
    status: Status,
    stop: Option<StopHandle>,
    total: u64,
    finished: u64,
    /// Games played before this run, when it continues a saved tournament.
    previous: u64,
    pgn_offset: usize,
    games: VecDeque<GameRow>,
    block: Vec<String>,
    in_block: bool,
    results: Vec<String>,
    log: VecDeque<String>,
    /// Games in progress, fed by fastchess's event stream; `None` when live view is off.
    live: Option<Arc<Mutex<LiveGames>>>,
    /// Why there are no live boards, when there are none.
    live_off: Option<&'static str>,
    live_seen: u64,
    /// Engine names in the order fastchess was given them; the first two are A and B.
    names: Vec<String>,
    concurrency: usize,
    /// When each game started, and the total time and count of the finished ones.
    started: HashMap<u64, Instant>,
    game_time: (Duration, u32),
    _task: Task<()>,
    _live_task: Option<Task<()>>,
}

/// A tab dropped for good (closed tabs are kept a while) takes its tournament with it;
/// fastchess saves the config on the way out, so it can still be continued.
impl Drop for Run {
    fn drop(&mut self) {
        if let Some(stop) = &self.stop {
            stop.stop();
        }
    }
}

impl Run {
    /// Games played in the whole tournament so far.
    fn played(&self) -> u64 {
        self.previous + self.finished
    }

    fn active(&self) -> bool {
        matches!(self.status, Status::Running | Status::Stopping)
    }

    fn pgn_file(&self) -> PathBuf {
        self.dir.join("games.pgn")
    }

    fn push_log(&mut self, line: String) {
        if self.log.len() == MAX_LOG {
            self.log.pop_front();
        }
        self.log.push_back(line);
    }

    fn handle(&mut self, line: String) {
        match fastchess::parse_line(&line) {
            Line::Rule => {
                if self.in_block {
                    self.results = std::mem::take(&mut self.block);
                }
                self.in_block = !self.in_block;
            }
            _ if self.in_block => self.block.push(line),
            Line::Started { game, total } => {
                self.total = total;
                self.started.insert(game, Instant::now());
            }
            Line::Finished {
                game,
                white,
                black,
                result,
                reason,
            } => {
                let row = GameRow {
                    game,
                    white: white.to_owned(),
                    black: black.to_owned(),
                    result: result.to_owned(),
                    reason: reason.to_owned(),
                    pgn_index: self.pgn_offset + self.finished as usize,
                };
                if let Some(start) = self.started.remove(&game) {
                    self.game_time.0 += start.elapsed();
                    self.game_time.1 += 1;
                }
                self.finished += 1;
                if self.games.len() == MAX_GAMES {
                    self.games.pop_back();
                }
                self.games.push_front(row);
            }
            Line::Text(_) => self.push_log(line),
        }
    }
}

/// What to start once fastchess is available.
struct Launch {
    dir: PathBuf,
    cwd: PathBuf,
    config: PathBuf,
    args: Vec<String>,
}

pub struct TournamentScreen {
    model: Entity<AppModel>,
    workspace: WeakEntity<Workspace>,
    source: Source,
    /// Fast's configured engines taking part, in the order they were ticked.
    selected: Vec<EngineId>,
    /// Executables added only for tournaments.
    extra_engines: Vec<Participant>,
    limit: Limit,
    games_per_round: u32,
    gauntlet: bool,
    chess960: bool,
    noswap: bool,
    reverse: bool,
    random_order: bool,
    sprt: bool,
    sprt_model: &'static str,
    tb_adjudication: bool,
    tb_paths: Entity<InputState>,
    tb_pieces: Entity<InputState>,
    tb_ignore50: bool,
    tb_adjudicate: &'static str,
    engine_args: Entity<InputState>,
    /// The games file's notation and move comments; `match_line` comes from its own input.
    pgn: PgnOut,
    pgn_match_line: Entity<InputState>,
    draw_adjudication: bool,
    resign_adjudication: bool,
    resign_twosided: bool,
    recover: bool,
    draw_movenumber: Entity<InputState>,
    draw_movecount: Entity<InputState>,
    draw_score: Entity<InputState>,
    resign_movecount: Entity<InputState>,
    resign_score: Entity<InputState>,
    limit_value: Entity<InputState>,
    clock_input: crate::controls::ClockInput,
    move_time: crate::controls::TimeInput,
    hash: Entity<InputState>,
    threads: Entity<InputState>,
    rounds: Entity<InputState>,
    concurrency: Entity<InputState>,
    book: TextSelect,
    book_task: Option<Task<()>>,
    openings: Entity<InputState>,
    plies: Entity<InputState>,
    elo0: Entity<InputState>,
    elo1: Entity<InputState>,
    alpha: Entity<InputState>,
    beta: Entity<InputState>,
    maxmoves: Entity<InputState>,
    seed: Entity<InputState>,
    wait: Entity<InputState>,
    time_margin: Entity<InputState>,
    restart: bool,
    affinity: bool,
    extra: Entity<InputState>,
    paste: Entity<TextareaState>,
    paste_dir: Entity<InputState>,
    fastchess_path: Entity<InputState>,
    form_error: Option<SharedString>,
    install_status: Option<SharedString>,
    install_cancel: Option<Arc<AtomicBool>>,
    install_task: Option<Task<()>>,
    pending: Option<Launch>,
    saved: Vec<fastchess::SavedRun>,
    /// Bytes in the tournaments folder, once measured.
    folder_size: Option<u64>,
    /// Free up space was clicked once and waits for the confirming click.
    clear_armed: bool,
    run: Option<Run>,
    live_games: Vec<LiveGame>,
    /// One board per fastchess thread, with the game and move count it shows.
    live_boards: HashMap<usize, (Entity<BoardView>, u64, usize)>,
    /// The game shown large, by slot.
    live_focus: Option<usize>,
    focus_board: Entity<BoardView>,
    focus_shown: (u64, usize),
    live_all: bool,
    /// Width of the live boards grid, measured at the last paint; zero until then.
    live_width: Pixels,
    games_filter: GameFilter,
    /// The column the finished games are ordered by and whether descending; `None` keeps
    /// them newest first.
    games_sort: Option<(GameSort, bool)>,
    /// The ⋯ menu above the run's header.
    menu_open: bool,
    /// The New tournament dialog.
    setup_open: bool,
    /// Whether the setup form shows its advanced settings.
    advanced_open: bool,
    /// Whether each fastchess executable offers the `-events` stream.
    events_support: HashMap<PathBuf, bool>,
    _prefs: Subscription,
}

impl TournamentScreen {
    /// A tournament is playing or stopping.
    pub fn is_running(&self) -> bool {
        self.run.as_ref().is_some_and(Run::active)
    }

    pub fn new(
        model: Entity<AppModel>,
        workspace: WeakEntity<Workspace>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let (saved, custom) = {
            let model = model.read(cx);
            (
                fastchess::saved_runs(&model.paths.data_dir),
                model
                    .config
                    .fastchess_path
                    .as_ref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_default(),
            )
        };
        let paste = cx.new(|cx| {
            TextareaState::new(window, cx)
                .placeholder(
                    "Paste a fastchess command line, e.g.\n\
                     fastchess -engine cmd=./new name=new -engine cmd=./base name=base \
                     -each tc=8+0.08 -rounds 1000 -repeat\n\n\
                     or the contents of a config.json fastchess saved, to continue that tournament.",
                )
                .rows(14)
        });
        // Boards only take new prefs as they move; a theme or board change must reach
        // the ones standing still too, or a finished run's boards keep the old colours.
        let prefs = cx.subscribe(&model, |screen, model, event, cx| {
            if matches!(event, ModelEvent::BoardPrefs) {
                let prefs = model.read(cx).config.board.prefs();
                let boards = screen.live_boards.values().map(|(board, _, _)| board);
                for board in boards.chain([&screen.focus_board]) {
                    board.update(cx, |board, cx| board.set_prefs(prefs, cx));
                }
                cx.notify();
            }
        });
        // Stop fastchess with the app, so no tournament keeps running unseen.
        cx.on_app_quit(|screen: &mut Self, _| {
            if let Some(stop) = screen.run.as_ref().and_then(|run| run.stop.clone()) {
                stop.stop();
            }
            async {}
        })
        .detach();
        let mut screen = Self {
            model,
            workspace,
            source: Source::Form,
            selected: Vec::new(),
            extra_engines: Vec::new(),
            limit: Limit::TimeControl,
            games_per_round: 2,
            gauntlet: false,
            chess960: false,
            noswap: false,
            reverse: false,
            random_order: true,
            sprt: false,
            sprt_model: "normalized",
            tb_adjudication: false,
            tb_paths: new_input(
                "",
                "Syzygy folders, separated by the system's path separator",
                window,
                cx,
            ),
            tb_pieces: new_input("", "All", window, cx),
            tb_ignore50: false,
            tb_adjudicate: "BOTH",
            engine_args: new_input("", "None", window, cx),
            pgn: PgnOut::default(),
            pgn_match_line: new_input("", "e.g. ^info string", window, cx),
            draw_adjudication: true,
            resign_adjudication: true,
            resign_twosided: true,
            recover: true,
            draw_movenumber: new_input("32", "32", window, cx),
            draw_movecount: new_input("6", "6", window, cx),
            draw_score: new_input("10", "10", window, cx),
            resign_movecount: new_input("3", "3", window, cx),
            resign_score: new_input("600", "600", window, cx),
            limit_value: new_input("20", "20", window, cx),
            clock_input: crate::controls::ClockInput::new("10", 1, "0.1", window, cx),
            move_time: crate::controls::TimeInput::new("0.5", 1, window, cx),
            hash: new_input("16", "MB", window, cx),
            threads: new_input("1", "1", window, cx),
            rounds: new_input("100", "100", window, cx),
            concurrency: new_input(
                fastchess::default_concurrency().to_string(),
                "1",
                window,
                cx,
            ),
            book: new_select(
                crate::opening_books::BOOKS
                    .iter()
                    .copied()
                    .chain(["No book", "Custom file…"])
                    .map(SharedString::from)
                    .collect(),
                0,
                window,
                cx,
            ),
            book_task: None,
            openings: new_input(
                "",
                "No book: games start from the initial position",
                window,
                cx,
            ),
            plies: new_input("", "All", window, cx),
            elo0: new_input("0", "0", window, cx),
            elo1: new_input("2", "2", window, cx),
            alpha: new_input("0.05", "0.05", window, cx),
            beta: new_input("0.05", "0.05", window, cx),
            maxmoves: new_input("", "No limit", window, cx),
            seed: new_input("", "Automatic", window, cx),
            wait: new_input("", "0", window, cx),
            time_margin: new_input("", "Fastchess default", window, cx),
            restart: false,
            affinity: false,
            extra: new_input("", "e.g. -srand 42 -use-affinity", window, cx),
            paste,
            paste_dir: new_input("", "Default: a new folder of its own", window, cx),
            fastchess_path: new_input(custom, "Downloaded automatically", window, cx),
            form_error: None,
            install_status: None,
            install_cancel: None,
            install_task: None,
            pending: None,
            saved,
            folder_size: None,
            clear_armed: false,
            run: None,
            live_games: Vec::new(),
            live_boards: HashMap::new(),
            live_focus: None,
            focus_board: cx.new(|_| BoardView::new("live-focus-board").read_only()),
            focus_shown: (0, usize::MAX),
            live_all: false,
            live_width: px(0.),
            games_filter: GameFilter::All,
            games_sort: None,
            menu_open: false,
            setup_open: false,
            advanced_open: false,
            events_support: HashMap::new(),
            _prefs: prefs,
        };
        screen.measure_folder(cx);
        screen
    }

    /// What the tab shows beside its name: progress, or that the run is over.
    pub fn tab_detail(&self) -> Option<SharedString> {
        let run = self.run.as_ref()?;
        Some(if run.active() {
            format!("{}/{} games", run.played(), run.total.max(run.played())).into()
        } else {
            "finished".into()
        })
    }

    pub fn breadcrumb(&self) -> SharedString {
        match self.tab_detail() {
            Some(detail) => format!("Tournament, {detail}").into(),
            None => "Tournament".into(),
        }
    }

    fn text(&self, state: &Entity<InputState>, cx: &Context<Self>) -> String {
        state.read(cx).value().trim().to_owned()
    }

    fn data_dir(&self, cx: &Context<Self>) -> PathBuf {
        self.model.read(cx).paths.data_dir.clone()
    }
}

impl Render for TournamentScreen {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let empty = self.run.is_none();
        let main = match &self.run {
            Some(run) => self.render_run(run, cx),
            None => self.render_empty(cx).into_any_element(),
        };
        let setup = self.setup_open.then(|| self.render_setup(window, cx));
        div()
            .relative()
            .size_full()
            .child(
                div()
                    .id("tournament-run")
                    .size_full()
                    .overflow_y_scroll()
                    .bg(rgb(T.tour_bg))
                    .p(px(24.))
                    .child(
                        h_flex()
                            .justify_center()
                            // With no run, the intro sits in the middle of the tab.
                            .when(empty, |row| row.min_h_full().items_center())
                            .child(
                                h_flex().justify_center().w_full().child(
                                    div()
                                        .w_full()
                                        .when(empty, |main| main.max_w(px(620.)))
                                        .child(main),
                                ),
                            ),
                    ),
            )
            .children(setup)
    }
}

/// A panel of the tournament tab.
/// A games-file checkbox: its id, label, current value and how it sets the value.
type PgnFlag = (&'static str, &'static str, bool, fn(&mut PgnOut, bool));
