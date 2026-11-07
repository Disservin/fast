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
    Pixels, Render, SharedString, Subscription, Task, WeakEntity, Window, deferred, div,
    prelude::*, px, relative, rgb,
};
use gpui_component::input::{InputState, Textarea, TextareaState};
use ui::{
    BoardView, MONO, T,
    widgets::{btn, check, divider, h_flex, mono, panel, section_label, segmented, v_flex},
};

use crate::{
    controls::{TextSelect, field, new_input, new_select, select_el, text_input},
    fastchess::{self, Limit, Line, Participant, Pasted, Setup, StopHandle},
    model::{AppModel, ModelEvent, pick_path},
    shell::Workspace,
    tournament_live::{self, LiveGame, LiveGames, Score, Search},
    tournament_wall::{self, LiveWall},
};

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

/// Live boards are this many pixels square in the tournament tab.
const LIVE_BOARD: f32 = 280.;

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
    random_order: bool,
    sprt: bool,
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
    run: Option<Run>,
    live_games: Vec<LiveGame>,
    /// One board per fastchess thread, with the game and move count it shows.
    live_boards: HashMap<usize, (Entity<BoardView>, u64, usize)>,
    /// The game shown large, by slot.
    live_focus: Option<usize>,
    focus_board: Entity<BoardView>,
    focus_shown: (u64, usize),
    live_all: bool,
    games_filter: GameFilter,
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
        Self {
            model,
            workspace,
            source: Source::Form,
            selected: Vec::new(),
            extra_engines: Vec::new(),
            limit: Limit::TimeControl,
            games_per_round: 2,
            gauntlet: false,
            random_order: true,
            sprt: false,
            draw_adjudication: true,
            resign_adjudication: true,
            resign_twosided: true,
            recover: true,
            draw_movenumber: new_input("32", "32", window, cx),
            draw_movecount: new_input("6", "6", window, cx),
            draw_score: new_input("10", "10", window, cx),
            resign_movecount: new_input("3", "3", window, cx),
            resign_score: new_input("600", "600", window, cx),
            limit_value: new_input("10+0.1", "10+0.1", window, cx),
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
            run: None,
            live_games: Vec::new(),
            live_boards: HashMap::new(),
            live_focus: None,
            focus_board: cx.new(|_| BoardView::new("live-focus-board").read_only()),
            focus_shown: (0, usize::MAX),
            live_all: false,
            games_filter: GameFilter::All,
            menu_open: false,
            setup_open: false,
            advanced_open: false,
            events_support: HashMap::new(),
            _prefs: prefs,
        }
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

    fn participants(&self, cx: &Context<Self>) -> Vec<Participant> {
        let model = self.model.read(cx);
        self.selected
            .iter()
            .filter_map(|id| model.engine(*id))
            .map(|engine| Participant {
                name: engine.name.clone(),
                cmd: engine.path.clone(),
                options: engine.options.clone(),
            })
            .chain(self.extra_engines.iter().cloned())
            .collect()
    }

    fn setup(&self, cx: &Context<Self>) -> Setup {
        Setup {
            participants: self.participants(cx),
            limit: self.limit,
            limit_value: self.text(&self.limit_value, cx),
            hash: self.text(&self.hash, cx),
            threads: self.text(&self.threads, cx),
            rounds: self.text(&self.rounds, cx),
            games_per_round: self.games_per_round,
            concurrency: self.text(&self.concurrency, cx),
            gauntlet: self.gauntlet,
            openings: self.text(&self.openings, cx),
            random_order: self.random_order,
            plies: self.text(&self.plies, cx),
            sprt: self.sprt,
            elo0: self.text(&self.elo0, cx),
            elo1: self.text(&self.elo1, cx),
            alpha: self.text(&self.alpha, cx),
            beta: self.text(&self.beta, cx),
            draw_adjudication: self.draw_adjudication,
            draw_movenumber: self.text(&self.draw_movenumber, cx),
            draw_movecount: self.text(&self.draw_movecount, cx),
            draw_score: self.text(&self.draw_score, cx),
            resign_adjudication: self.resign_adjudication,
            resign_movecount: self.text(&self.resign_movecount, cx),
            resign_score: self.text(&self.resign_score, cx),
            resign_twosided: self.resign_twosided,
            maxmoves: self.text(&self.maxmoves, cx),
            recover: self.recover,
            seed: self.text(&self.seed, cx),
            wait: self.text(&self.wait, cx),
            time_margin: self.text(&self.time_margin, cx),
            restart: self.restart,
            affinity: self.affinity,
            extra: self.text(&self.extra, cx),
        }
    }

    /// The executable to run: the user's own, or the pinned download if present.
    fn program(&self, cx: &Context<Self>) -> Option<PathBuf> {
        let custom = self.text(&self.fastchess_path, cx);
        if !custom.is_empty() {
            return Some(PathBuf::from(custom));
        }
        fastchess::installed(&self.data_dir(cx))
    }

    fn fail(&mut self, message: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.form_error = Some(message.into());
        cx.notify();
    }

    fn start_form(&mut self, cx: &mut Context<Self>) {
        if self.book_task.is_some() {
            return;
        }
        let mut setup = self.setup(cx);
        let index = self
            .book
            .read(cx)
            .selected_index(cx)
            .map(|index| index.row)
            .unwrap_or(0);
        if let Some(&name) = crate::opening_books::BOOKS.get(index) {
            let data = self.data_dir(cx);
            let path = crate::opening_books::path(&data, name);
            setup.openings = path.to_string_lossy().into_owned();
            if !path.is_file() {
                self.form_error = Some(format!("Downloading {name}…").into());
                let worker =
                    cx.background_spawn(async move { crate::opening_books::install(&data, name) });
                self.book_task = Some(cx.spawn(async move |this, cx| {
                    let result = worker.await;
                    let _ = this.update(cx, |screen, cx| {
                        screen.book_task = None;
                        match result {
                            Ok(()) => {
                                screen.form_error = None;
                                screen.start_setup(setup, cx);
                            }
                            Err(err) => screen.fail(
                                format!("Book download failed: {err:#}. Start again to retry."),
                                cx,
                            ),
                        }
                    });
                }));
                cx.notify();
                return;
            }
        } else if index == crate::opening_books::BOOKS.len() {
            setup.openings.clear();
        }
        self.start_setup(setup, cx);
    }

    fn start_setup(&mut self, setup: Setup, cx: &mut Context<Self>) {
        let dir = match fastchess::new_run_dir(&self.data_dir(cx)) {
            Ok(dir) => dir,
            Err(err) => return self.fail(format!("{err:#}"), cx),
        };
        match setup.args(&dir) {
            Ok(args) => self.launch(
                Launch {
                    cwd: dir.clone(),
                    config: dir.join("config.json"),
                    dir,
                    args,
                },
                cx,
            ),
            Err(err) => {
                let _ = std::fs::remove_dir(&dir);
                self.fail(format!("{err:#}"), cx)
            }
        }
    }

    fn start_paste(&mut self, cx: &mut Context<Self>) {
        let text = self.paste.read(cx).value().to_string();
        let pasted = match fastchess::parse_paste(&text) {
            Ok(pasted) => pasted,
            Err(err) => return self.fail(format!("{err:#}"), cx),
        };
        let dir = match fastchess::new_run_dir(&self.data_dir(cx)) {
            Ok(dir) => dir,
            Err(err) => return self.fail(format!("{err:#}"), cx),
        };
        let custom_cwd = self.text(&self.paste_dir, cx);
        let cwd = if custom_cwd.is_empty() || matches!(pasted, Pasted::Config(_)) {
            dir.clone()
        } else {
            PathBuf::from(custom_cwd)
        };
        if !cwd.is_dir() {
            let _ = std::fs::remove_dir(&dir);
            return self.fail(format!("Folder not found: {}", cwd.display()), cx);
        }
        match pasted.launch(&dir, &cwd) {
            Ok((args, config)) => self.launch(
                Launch {
                    dir,
                    cwd,
                    config,
                    args,
                },
                cx,
            ),
            Err(err) => self.fail(format!("{err:#}"), cx),
        }
    }

    /// Continues from a saved config, in the folder the tournament ran in.
    fn resume(&mut self, dir: PathBuf, cwd: PathBuf, config: PathBuf, cx: &mut Context<Self>) {
        self.launch(
            Launch {
                cwd,
                args: fastchess::resume_args(&config),
                dir,
                config,
            },
            cx,
        );
    }

    fn launch(&mut self, launch: Launch, cx: &mut Context<Self>) {
        if self.run.as_ref().is_some_and(Run::active) {
            return self.fail("A tournament is already running in this tab", cx);
        }
        self.form_error = None;
        match self.program(cx) {
            Some(program) => self.spawn(program, launch, cx),
            None => {
                self.pending = Some(launch);
                self.download(cx);
            }
        }
        cx.notify();
    }

    fn spawn(&mut self, program: PathBuf, launch: Launch, cx: &mut Context<Self>) {
        let pgn_offset = std::fs::read_to_string(launch.dir.join("games.pgn"))
            .map(|text| fastchess::count_games(&text))
            .unwrap_or(0);
        let previous = fastchess::saved_games(&launch.config);
        let has_events = *self
            .events_support
            .entry(program.clone())
            .or_insert_with(|| tournament_live::has_events(&program));
        let live_off = if !has_events {
            Some(ui::i18n::t(
                "Live boards need a fastchess build with the -events option.",
            ))
        } else if tournament_live::has_own_events(&launch.args) {
            Some(ui::i18n::t(
                "Live boards are off because this command sets its own -events option.",
            ))
        } else {
            None
        };
        let live = live_off
            .is_none()
            .then(|| Arc::new(Mutex::new(LiveGames::default())));
        // The event options are Fast's business: they stay out of the command shown to copy.
        let mut args = launch.args.clone();
        if live.is_some() {
            args.extend(tournament_live::events_args());
        }
        let process = match fastchess::spawn(&program, &args, &launch.cwd, live.clone()) {
            Ok(process) => process,
            Err(err) => return self.fail(format!("{err:#}"), cx),
        };
        let stop = process.stop_handle();
        let names = engines(&launch.args)
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        let concurrency = flag_value(&launch.args, "-concurrency")
            .and_then(|value| value.parse().ok())
            .unwrap_or_else(fastchess::default_concurrency);
        let task = cx.spawn(async move |this, cx| {
            while let Ok(first) = process.lines.recv().await {
                // Take whatever else is waiting so a burst costs one redraw.
                let mut lines = vec![first];
                while let Ok(line) = process.lines.try_recv() {
                    lines.push(line);
                }
                let alive = this.update(cx, |screen, cx| {
                    if let Some(run) = screen.run.as_mut() {
                        for line in lines {
                            run.handle(line);
                        }
                    }
                    cx.notify();
                });
                if alive.is_err() {
                    return;
                }
            }
            let exit = cx.background_spawn(async move { process.wait() }).await;
            let _ = this.update(cx, |screen, cx| {
                screen.finish(exit, cx);
            });
        });
        self.run = Some(Run {
            dir: launch.dir,
            cwd: launch.cwd,
            config: launch.config,
            program,
            args: launch.args,
            status: Status::Running,
            stop: Some(stop),
            total: 0,
            finished: 0,
            previous,
            pgn_offset,
            games: VecDeque::new(),
            block: Vec::new(),
            in_block: false,
            results: Vec::new(),
            log: VecDeque::new(),
            _live_task: live.is_some().then(|| {
                cx.spawn(async move |this, cx| {
                    loop {
                        cx.background_executor().timer(LIVE_REFRESH).await;
                        match this.update(cx, |screen, cx| screen.refresh_live(cx)) {
                            Ok(true) => {}
                            _ => break,
                        }
                    }
                })
            }),
            live,
            live_off,
            live_seen: 0,
            names,
            concurrency,
            started: HashMap::new(),
            game_time: (Duration::ZERO, 0),
            _task: task,
        });
        self.live_games.clear();
        self.live_boards.clear();
        self.live_focus = None;
        self.setup_open = false;
        cx.notify();
    }

    /// Copies new moves from the event stream into the boards. Returns false once the
    /// run is over, which ends the refresh loop.
    fn refresh_live(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(run) = self.run.as_mut() else {
            return false;
        };
        if !run.active() {
            return false;
        }
        let Some(live) = &run.live else {
            return false;
        };
        let games = {
            let Ok(live) = live.lock() else {
                return false;
            };
            if live.serial == run.live_seen {
                return true;
            }
            run.live_seen = live.serial;
            live.snapshot()
        };
        let prefs = self.model.read(cx).config.board.prefs();
        for game in &games {
            let entry = self.live_boards.entry(game.slot).or_insert_with(|| {
                let board = cx.new(|_| BoardView::new(("live-board", game.slot)).read_only());
                (board, 0, usize::MAX)
            });
            let mut shown = (entry.1, entry.2);
            tournament_wall::sync_board(&entry.0, &mut shown, game, prefs, cx);
            (entry.1, entry.2) = shown;
        }
        if let Some(game) = self
            .live_focus
            .and_then(|slot| games.iter().find(|game| game.slot == slot))
        {
            let board = self.focus_board.clone();
            let mut shown = self.focus_shown;
            tournament_wall::sync_board(&board, &mut shown, game, prefs, cx);
            self.focus_shown = shown;
        }
        self.live_games = games;
        cx.notify();
        true
    }

    fn focus_live(&mut self, slot: usize, cx: &mut Context<Self>) {
        self.live_focus = (self.live_focus != Some(slot)).then_some(slot);
        self.focus_shown = (0, usize::MAX);
        if let Some(run) = self.run.as_mut() {
            // Draw the focused board on the next refresh even if nothing moved.
            run.live_seen = run.live_seen.wrapping_sub(1);
        }
        self.refresh_live(cx);
        cx.notify();
    }

    fn finish(&mut self, exit: fastchess::Exit, cx: &mut Context<Self>) {
        let Some(run) = self.run.as_mut() else {
            return;
        };
        run.stop = None;
        let resumable = run.config.is_file();
        run.status = if exit.stopped {
            Status::Finished(if resumable {
                "Stopped. Continue picks up where it left off.".into()
            } else {
                "Stopped.".into()
            })
        } else if exit.success {
            Status::Finished("Finished.".into())
        } else {
            let last = run
                .log
                .iter()
                .rev()
                .find(|line| !line.trim().is_empty())
                .cloned()
                .unwrap_or_default();
            Status::Failed(match exit.code {
                Some(code) => format!("fastchess exited with code {code}. {last}"),
                None => format!("fastchess was terminated. {last}"),
            })
        };
        // The games in progress ended with the run.
        self.live_games.clear();
        self.live_focus = None;
        self.saved = fastchess::saved_runs(&self.data_dir(cx));
        cx.notify();
    }

    fn stop(&mut self, cx: &mut Context<Self>) {
        if let Some(run) = self.run.as_mut()
            && let Some(stop) = &run.stop
        {
            stop.stop();
            run.status = Status::Stopping;
            cx.notify();
        }
    }

    fn download(&mut self, cx: &mut Context<Self>) {
        if self.install_cancel.is_some() {
            return;
        }
        let data = self.data_dir(cx);
        let cancel = Arc::new(AtomicBool::new(false));
        self.install_cancel = Some(cancel.clone());
        self.install_status = Some(format!("Preparing fastchess {}…", fastchess::VERSION).into());
        let (sender, receiver) = smol::channel::unbounded::<String>();
        let worker = cx.background_spawn(async move {
            fastchess::install(&data, cancel, |message| {
                let _ = sender.try_send(message);
            })
        });
        self.install_task = Some(cx.spawn(async move |this, cx| {
            while let Ok(message) = receiver.recv().await {
                if this
                    .update(cx, |screen, cx| {
                        screen.install_status = Some(message.into());
                        cx.notify();
                    })
                    .is_err()
                {
                    return;
                }
            }
            let result = worker.await;
            let _ = this.update(cx, |screen, cx| {
                screen.install_cancel = None;
                match result {
                    Ok(program) => {
                        screen.install_status =
                            Some(format!("fastchess {} is ready.", fastchess::VERSION).into());
                        if let Some(launch) = screen.pending.take() {
                            screen.spawn(program, launch, cx);
                        }
                    }
                    Err(err) => {
                        screen.pending = None;
                        screen.install_status =
                            Some(format!("Download stopped: {err:#}. You can retry.").into());
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn cancel_download(&mut self, cx: &mut Context<Self>) {
        if let Some(cancel) = &self.install_cancel {
            cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        self.pending = None;
        cx.notify();
    }

    fn save_fastchess_path(&mut self, cx: &mut Context<Self>) {
        let text = self.text(&self.fastchess_path, cx);
        let path = (!text.is_empty()).then(|| PathBuf::from(text));
        if let Some(path) = &path
            && !path.is_file()
        {
            return self.fail(format!("No file at {}", path.display()), cx);
        }
        self.form_error = None;
        self.model.update(cx, |model, cx| {
            model.config.fastchess_path = path;
            model.save_config(cx);
        });
        cx.notify();
    }

    fn toggle_engine(&mut self, id: EngineId, on: bool, cx: &mut Context<Self>) {
        self.selected.retain(|selected| *selected != id);
        if on {
            self.selected.push(id);
        }
        cx.notify();
    }

    fn add_executable(&mut self, cx: &mut Context<Self>) {
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

    fn browse_openings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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

    fn load_config_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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

    /// Opens fastchess game `number` in Analyse: by its `GameId` tag when fastchess wrote
    /// one, otherwise by its place in the file (games are written in finishing order).
    fn open_game(
        &mut self,
        number: u64,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(run) = &self.run else { return };
        let game = std::fs::read_to_string(run.pgn_file())
            .ok()
            .and_then(|text| {
                if text.contains("[GameId ") {
                    fastchess::game_by_id(&text, number)
                } else {
                    fastchess::nth_game(&text, index)
                }
            });
        match game {
            Some(pgn) => {
                if let Some(workspace) = self.workspace.upgrade() {
                    workspace.update(cx, |workspace, cx| workspace.open_pgn(&pgn, 0, window, cx));
                }
            }
            None => self.model.update(cx, |model, cx| {
                model.toast("That game is not in the PGN file yet.", cx)
            }),
        }
    }

    fn copy_command(&self, cx: &mut Context<Self>) {
        if let Some(run) = &self.run {
            let command = fastchess::display_command(&run.program.display().to_string(), &run.args);
            cx.write_to_clipboard(ClipboardItem::new_string(command));
            self.model
                .update(cx, |model, cx| model.toast("Command copied", cx));
        }
    }

    fn copy_config(&self, cx: &mut Context<Self>) {
        let Some(run) = &self.run else { return };
        match std::fs::read_to_string(&run.config) {
            Ok(text) => {
                cx.write_to_clipboard(ClipboardItem::new_string(text));
                self.model
                    .update(cx, |model, cx| model.toast("config.json copied", cx));
            }
            Err(_) => self.model.update(cx, |model, cx| {
                model.toast("fastchess has not saved a config yet.", cx)
            }),
        }
    }

    fn render_fastchess(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let installed = fastchess::installed(&self.data_dir(cx)).is_some();
        let custom = !self.text(&self.fastchess_path, cx).is_empty();
        let status = if custom {
            "Using your own fastchess executable.".to_owned()
        } else if installed {
            format!("fastchess {} is downloaded.", fastchess::VERSION)
        } else {
            format!(
                "fastchess {} is downloaded from GitHub the first time you start a tournament.",
                fastchess::VERSION
            )
        };
        let this = cx.entity();
        let save = this.clone();
        let cancel = this.clone();
        v_flex()
            .gap(px(8.))
            .child(section_label("fastchess"))
            .child(
                div()
                    .text_size(px(13.))
                    .text_color(rgb(T.muted))
                    .child(SharedString::from(status)),
            )
            .children(self.install_status.clone().map(|status| {
                div()
                    .text_size(px(13.))
                    .text_color(rgb(T.fg_2))
                    .child(status)
            }))
            .child(
                h_flex()
                    .gap(px(6.))
                    .when(
                        !installed && !custom && self.install_cancel.is_none(),
                        |row| {
                            row.child(
                                btn("fastchess-download", ui::i18n::t("Download now")).on_click(
                                    move |_, cx| this.update(cx, |screen, cx| screen.download(cx)),
                                ),
                            )
                        },
                    )
                    .when(self.install_cancel.is_some(), |row| {
                        row.child(
                            btn("fastchess-cancel", ui::i18n::t("Cancel download")).on_click(
                                move |_, cx| {
                                    cancel.update(cx, |screen, cx| screen.cancel_download(cx))
                                },
                            ),
                        )
                    }),
            )
            .child(field(
                "Own executable (optional)",
                h_flex()
                    .gap(px(6.))
                    .child(div().flex_1().child(text_input(&self.fastchess_path)))
                    .child(btn("fastchess-path-save", ui::i18n::t("Use")).on_click(
                        move |_, cx| save.update(cx, |screen, cx| screen.save_fastchess_path(cx)),
                    )),
            ))
    }

    fn render_engines(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
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
            let label = match order {
                Some(index) if self.gauntlet && index == 0 => format!("{} · seed", engine.name),
                _ => engine.name.clone(),
            };
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
                        check(("tournament-engine", id as usize), order.is_some(), label)
                            .text(13.)
                            .on_change(move |on, _, cx| {
                                this.update(cx, |screen, cx| screen.toggle_engine(id, on, cx))
                            }),
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
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .text_size(px(13.))
                            .child(SharedString::from(format!(
                                "{} · {}",
                                engine.name,
                                engine.cmd.display()
                            ))),
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

    fn toggle<F>(
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

    fn render_form(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let pair = |a: gpui::Div, b: gpui::Div| {
            h_flex()
                .gap(px(8.))
                .items_start()
                .child(a.flex_1())
                .child(b.flex_1())
        };
        let triple = |a: gpui::Div, b: gpui::Div, c: gpui::Div| pair(a, b).child(c.flex_1());
        let limit_this = cx.entity();
        let games_this = cx.entity();
        let type_this = cx.entity();
        let order_this = cx.entity();
        let browse = cx.entity();
        let expand = cx.entity();
        let _ = window;

        // What most tournaments need: who plays, how fast, how many games, which book.
        let basic = v_flex()
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
            .child(field(self.limit.hint(), text_input(&self.limit_value)))
            .child(section_label(ui::i18n::t("Games")))
            .child(pair(
                field("Rounds", text_input(&self.rounds)),
                field("Concurrency", text_input(&self.concurrency)),
            ))
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
            });

        let open = self.advanced_open;
        let disclosure = h_flex()
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
                        "Hash and threads, pairing, adjudication, recovery, extra options",
                    )),
            )
            .on_click(move |_, _, cx| {
                expand.update(cx, |screen, cx| {
                    screen.advanced_open = !screen.advanced_open;
                    cx.notify();
                })
            });

        let advanced = open.then(|| {
            v_flex()
                .gap(px(14.))
                .pl(px(12.))
                .border_l_2()
                .border_color(rgb(T.border))
                .child(section_label(ui::i18n::t("Engines")))
                .child(pair(
                    field("Hash (MB)", text_input(&self.hash)),
                    field("Threads", text_input(&self.threads)),
                ))
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
        });

        v_flex()
            .gap(px(16.))
            .child(basic)
            .child(disclosure)
            .children(advanced)
    }

    fn render_paste(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let load = cx.entity();
        v_flex()
            .gap(px(10.))
            .child(div().text_size(px(13.)).text_color(rgb(T.muted)).child(
                "A command line starts a new tournament; a config.json continues one, \
                         including its results so far.",
            ))
            .child(
                div()
                    .rounded(px(4.))
                    .bg(rgb(T.bg))
                    .border_1()
                    .border_color(rgb(T.border_strong))
                    .font_family(MONO)
                    .text_size(px(12.5))
                    .child(Textarea::new(&self.paste).appearance(false).h(px(300.))),
            )
            .child(btn("tournament-load-config", "Load config file…").on_click(
                move |window, cx| load.update(cx, |screen, cx| screen.load_config_file(window, cx)),
            ))
            .child(field(
                "Run a command line in folder (for its relative paths)",
                text_input(&self.paste_dir),
            ))
    }

    fn render_saved(&self, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        let busy = self.run.as_ref().is_some_and(Run::active);
        let current = self.run.as_ref().map(|run| run.dir.clone());
        let rows: Vec<_> = self
            .saved
            .iter()
            .filter(|saved| Some(&saved.dir) != current.as_ref())
            .take(8)
            .cloned()
            .collect();
        if rows.is_empty() {
            return None;
        }
        let mut list = v_flex()
            .gap(px(6.))
            .child(section_label("Earlier tournaments"));
        for (index, saved) in rows.into_iter().enumerate() {
            let this = cx.entity();
            let (dir, config) = (saved.dir.clone(), saved.config.clone());
            list = list.child(
                h_flex()
                    .gap(px(8.))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .overflow_hidden()
                                    .child(SharedString::from(saved.engines.join(" · "))),
                            )
                            .child(div().text_size(px(12.)).text_color(rgb(T.muted)).child(
                                SharedString::from(format!(
                                        "{} games saved · {}",
                                        saved.games,
                                        saved
                                            .dir
                                            .file_name()
                                            .map(|name| name.to_string_lossy().into_owned())
                                            .unwrap_or_default()
                                    )),
                            )),
                    )
                    .child(
                        btn(("tournament-continue", index), "Continue")
                            .disabled(busy)
                            .on_click(move |_, cx| {
                                let (dir, config) = (dir.clone(), config.clone());
                                let cwd = dir.clone();
                                this.update(cx, |screen, cx| screen.resume(dir, cwd, config, cx))
                            }),
                    ),
            );
        }
        Some(list)
    }

    fn open_setup(&mut self, cx: &mut Context<Self>) {
        self.setup_open = true;
        self.form_error = None;
        cx.notify();
    }

    fn close_setup(&mut self, cx: &mut Context<Self>) {
        self.setup_open = false;
        cx.notify();
    }

    /// Escape closes the setup dialog. Returns whether it was open.
    pub fn escape(&mut self, cx: &mut Context<Self>) -> bool {
        let open = self.setup_open;
        if open {
            self.close_setup(cx);
        }
        open
    }

    /// The tournament setup, over the tab. Closes once fastchess starts.
    fn render_setup(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let source_this = cx.entity();
        let start = cx.entity();
        let (close, backdrop) = (cx.entity(), cx.entity());
        let busy = self.run.as_ref().is_some_and(Run::active) || self.pending.is_some();
        let source = self.source;
        let body = match source {
            Source::Form => self.render_form(window, cx).into_any_element(),
            Source::Paste => self.render_paste(cx).into_any_element(),
        };
        let dialog_width = (f32::from(window.viewport_size().width) * 0.7).clamp(760., 1200.);
        let dialog = v_flex()
            .id("tournament-setup")
            .w(px(dialog_width))
            .max_w_full()
            .max_h_full()
            .overflow_y_scroll()
            .rounded(px(12.))
            .bg(rgb(T.surface))
            .border_1()
            .border_color(rgb(T.border_strong))
            .shadow_lg()
            // Clicks inside the dialog stay out of the backdrop.
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                v_flex()
                    .flex_none()
                    .gap(px(16.))
                    .px(px(22.))
                    .pt(px(18.))
                    .pb(px(24.))
                    .child(
                        h_flex()
                            .justify_between()
                            .items_center()
                            .child(
                                div()
                                    .text_size(px(18.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(ui::i18n::t("New tournament")),
                            )
                            .child(
                                btn("tournament-setup-close", "×")
                                    .h(28.)
                                    .w(28.)
                                    .pad(4.)
                                    .tooltip(ui::i18n::t("Close"))
                                    .on_click(move |_, cx| {
                                        close.update(cx, |screen, cx| screen.close_setup(cx))
                                    }),
                            ),
                    )
                    .child(self.render_fastchess(cx))
                    .child(segmented(
                        "tournament-source",
                        vec![
                            (Source::Form, "Set up".into(), false),
                            (Source::Paste, "Paste config".into(), false),
                        ],
                        source,
                        move |source, _, cx| {
                            source_this.update(cx, |screen, cx| {
                                screen.source = source;
                                screen.form_error = None;
                                cx.notify();
                            })
                        },
                    ))
                    .child(body)
                    .children(self.form_error.clone().map(|error| {
                        div()
                            .text_size(px(13.))
                            .text_color(rgb(T.warn))
                            .child(error)
                    }))
                    .child(
                        btn("tournament-start", "Start tournament")
                            .primary()
                            .full()
                            .h(38.)
                            .disabled(busy)
                            .on_click(move |_, cx| {
                                start.update(cx, |screen, cx| match screen.source {
                                    Source::Form => screen.start_form(cx),
                                    Source::Paste => screen.start_paste(cx),
                                })
                            }),
                    ),
            );
        div()
            .id("tournament-setup-backdrop")
            .absolute()
            .inset_0()
            .occlude()
            .flex()
            .items_center()
            .justify_center()
            .p(px(24.))
            .bg(gpui::rgba(0x0000_0099))
            .on_mouse_down(gpui::MouseButton::Left, move |_, _, cx| {
                backdrop.update(cx, |screen, cx| screen.close_setup(cx))
            })
            .child(dialog)
            .into_any_element()
    }

    /// Opens the watch-all window on the games in progress.
    fn watch_all(&self, cx: &mut Context<Self>) {
        let Some(run) = &self.run else { return };
        let Some(live) = run.live.clone() else { return };
        LiveWall::open(self.model.clone(), live, run.names.clone(), cx);
    }

    fn render_run(&self, run: &Run, cx: &mut Context<Self>) -> AnyElement {
        let this = cx.entity();
        let live = self.live_games.len() as u64;
        let total = run.total.max(run.played());
        let fraction = if total == 0 {
            0.
        } else {
            run.played() as f32 / total as f32
        };
        let queued = total.saturating_sub(run.played() + live);
        let (label, fg, bg, note) = match &run.status {
            Status::Running => ("Running", T.live_fg, T.live_bg, None),
            Status::Stopping => (
                "Stopping",
                T.warn,
                T.tour_tile,
                Some((
                    "Stopping: fastchess is saving its config. Stop again to kill it.".to_owned(),
                    T.warn,
                )),
            ),
            Status::Finished(text) => {
                let label = if text.starts_with("Stopped") {
                    "Stopped"
                } else {
                    "Finished"
                };
                (label, T.muted, T.tour_tile, Some((text.clone(), T.muted)))
            }
            Status::Failed(text) => (
                "Failed",
                T.danger_fg,
                T.danger_bg,
                Some((text.clone(), T.danger_fg)),
            ),
        };
        let pill = h_flex()
            .gap(px(6.))
            .items_center()
            .px(px(10.))
            .py(px(3.))
            .rounded(px(999.))
            .bg(rgb(bg))
            .child(div().size(px(7.)).rounded_full().bg(rgb(fg)))
            .child(div().text_size(px(12.)).text_color(rgb(fg)).child(label));
        let (a, b) = (
            run.names.first().map(String::as_str).unwrap_or("Engine A"),
            run.names.get(1).map(String::as_str).unwrap_or("Engine B"),
        );
        let title = h_flex()
            .gap(px(12.))
            .items_center()
            .flex_wrap()
            .child(name_chip(a, engine_colour(&run.names, a), 22.))
            .child(
                div()
                    .text_size(px(22.))
                    .text_color(rgb(T.muted))
                    .child(ui::i18n::t("vs")),
            )
            .child(name_chip(b, engine_colour(&run.names, b), 22.));

        let open_last = run.games.front().map(|game| (game.game, game.pgn_index));
        let can_resume = !run.active() && run.config.is_file();
        let folder = run.dir.clone();
        let (dir, cwd, config) = (run.dir.clone(), run.cwd.clone(), run.config.clone());
        let more = this.clone();
        let menu =
            self.menu_open.then(|| {
                let (copy, copy_config) = (this.clone(), this.clone());
                // Deferred so it paints above the cards that follow the header, and occluding
                // so they neither show through nor take its clicks.
                deferred(
                    v_flex()
                        .id("tournament-more-menu")
                        .absolute()
                        .top(px(40.))
                        .right(px(0.))
                        .w(px(190.))
                        .p(px(6.))
                        .gap(px(4.))
                        .rounded(px(10.))
                        .bg(rgb(T.surface))
                        .border_1()
                        .border_color(rgb(T.border_strong))
                        .shadow_lg()
                        .occlude()
                        .child(btn("tournament-copy", "Copy command").full().on_click(
                            move |_, cx| {
                                copy.update(cx, |screen, cx| {
                                    screen.menu_open = false;
                                    screen.copy_command(cx)
                                })
                            },
                        ))
                        .child(
                            btn("tournament-copy-config", "Copy config")
                                .full()
                                .on_click(move |_, cx| {
                                    copy_config.update(cx, |screen, cx| {
                                        screen.menu_open = false;
                                        screen.copy_config(cx)
                                    })
                                }),
                        ),
                )
                .with_priority(1)
            });
        let overflow = div()
            .relative()
            .child(
                btn("tournament-more", "⋯")
                    .tooltip(ui::i18n::t("More actions"))
                    .on_click(move |_, cx| {
                        more.update(cx, |screen, cx| {
                            screen.menu_open = !screen.menu_open;
                            cx.notify();
                        })
                    }),
            )
            .children(menu);

        let actions = h_flex()
            .gap(px(8.))
            .items_center()
            .flex_wrap()
            .child(
                btn("tournament-open-folder", "Open folder")
                    .tooltip(folder.display().to_string())
                    .on_click(move |_, _| reveal_folder(&folder)),
            )
            .when_some(open_last, |row, (number, index)| {
                let open = this.clone();
                row.child(btn("tournament-open-last", "Open last game").on_click(
                    move |window, cx| {
                        open.update(cx, |screen, cx| screen.open_game(number, index, window, cx))
                    },
                ))
            })
            .when(run.live.is_some() && run.active(), |row| {
                let watch = this.clone();
                row.child(
                    btn("tournament-watch-all", "Watch all live")
                        .primary()
                        .on_click(move |_, cx| watch.update(cx, |screen, cx| screen.watch_all(cx))),
                )
            })
            .when(!run.active(), |row| {
                let new = this.clone();
                row.child(
                    btn("tournament-new", ui::i18n::t("New tournament"))
                        .on_click(move |_, cx| new.update(cx, |screen, cx| screen.open_setup(cx))),
                )
            })
            .when(can_resume, |row| {
                let resume = this.clone();
                row.child(
                    btn("tournament-resume", ui::i18n::t("Continue"))
                        .primary()
                        .on_click(move |_, cx| {
                            let (dir, cwd, config) = (dir.clone(), cwd.clone(), config.clone());
                            resume.update(cx, |screen, cx| screen.resume(dir, cwd, config, cx))
                        }),
                )
            })
            .when(run.active(), |row| {
                let stop = this.clone();
                row.child(
                    div()
                        .id("tournament-stop")
                        .flex()
                        .items_center()
                        .h(px(32.))
                        .px(px(14.))
                        .rounded(px(8.))
                        .border_1()
                        .border_color(rgb(T.danger_border))
                        .bg(rgb(T.danger_bg))
                        .text_size(px(13.))
                        .text_color(rgb(T.danger_fg))
                        .cursor_pointer()
                        .child(ui::i18n::t("Stop"))
                        .on_click(move |_, _, cx| stop.update(cx, |screen, cx| screen.stop(cx))),
                )
            })
            .child(overflow);

        let header = card()
            .child(
                h_flex()
                    .justify_between()
                    .items_start()
                    .flex_wrap()
                    .gap(px(16.))
                    .child(
                        v_flex()
                            .gap(px(8.))
                            .min_w_0()
                            .child(
                                h_flex()
                                    .gap(px(14.))
                                    .items_center()
                                    .flex_wrap()
                                    .child(title)
                                    .child(pill),
                            )
                            .children(note.map(|(text, colour)| {
                                div()
                                    .text_size(px(13.))
                                    .text_color(rgb(colour))
                                    .child(SharedString::from(text))
                            })),
                    )
                    .child(actions),
            )
            .child(
                v_flex()
                    .gap(px(8.))
                    .child(
                        h_flex()
                            .justify_between()
                            .text_size(px(13.))
                            .text_color(rgb(T.muted))
                            .child(SharedString::from(format!(
                                "{} / {} games · {:.0}%",
                                run.played(),
                                total,
                                fraction * 100.
                            )))
                            .child(SharedString::from(format!(
                                "{live} live · {queued} queued · ETA {}",
                                eta(run)
                            ))),
                    )
                    .child(
                        div()
                            .h(px(8.))
                            .w_full()
                            .rounded(px(4.))
                            .bg(rgb(T.tour_tile))
                            .child(
                                div()
                                    .h_full()
                                    .rounded(px(4.))
                                    .bg(rgb(T.engine_a))
                                    .w(relative(fraction.clamp(0., 1.))),
                            ),
                    ),
            );

        let mut log = v_flex().gap(px(2.));
        for line in run.log.iter().rev().take(150) {
            log = log.child(
                div()
                    .font_family(MONO)
                    .text_size(px(12.))
                    .text_color(rgb(T.fg_2))
                    .child(SharedString::from(line.clone())),
            );
        }

        v_flex()
            .gap(px(20.))
            .child(header)
            .child(self.render_results(run))
            .child(self.render_live(run, cx))
            .child(self.render_games(run, cx))
            .child(locked_summary(run))
            .when(!run.active(), |column| {
                column.children(self.render_saved(cx).map(|saved| card().child(saved)))
            })
            .child(card().child(heading("fastchess output")).child(log))
            .into_any_element()
    }

    fn render_results(&self, run: &Run) -> Div {
        let numbers = fastchess::results(&run.results);
        let view = numbers
            .as_ref()
            .and_then(|n| n.engines.as_ref().map(|(a, _)| a.clone()))
            .or_else(|| run.names.first().cloned())
            .unwrap_or_else(|| "Engine A".into());
        let games = numbers
            .as_ref()
            .map(|n| n.games)
            .unwrap_or_else(|| run.played());
        let body = match (&numbers, run.results.is_empty()) {
            (Some(n), _) => {
                let other = n
                    .engines
                    .as_ref()
                    .map(|(_, b)| b.clone())
                    .or_else(|| run.names.get(1).cloned())
                    .unwrap_or_else(|| "Engine B".into());
                results_body(n, &view, &other).into_any_element()
            }
            (None, false) => {
                let mut block = v_flex();
                for line in &run.results {
                    block = block.child(
                        div()
                            .font_family(MONO)
                            .text_size(px(12.5))
                            .whitespace_nowrap()
                            .child(SharedString::from(line.clone())),
                    );
                }
                div()
                    .id("tournament-results-block")
                    .overflow_x_scroll()
                    .p(px(12.))
                    .rounded(px(10.))
                    .bg(rgb(T.tour_tile))
                    .border_1()
                    .border_color(rgb(T.tour_tile_border))
                    .child(block)
                    .into_any_element()
            }
            (None, true) => {
                small("Results appear after the first finished game.").into_any_element()
            }
        };
        card()
            .child(
                h_flex()
                    .justify_between()
                    .items_center()
                    .flex_wrap()
                    .gap(px(8.))
                    .child(heading(format!("Results · {games} games")))
                    .child(small(format!("from {view}'s view"))),
            )
            .child(body)
    }

    /// Games in progress: a board per game, with its clocks and evaluation.
    fn render_live(&self, run: &Run, cx: &mut Context<Self>) -> AnyElement {
        let count_text = if self.live_games.is_empty() {
            String::new()
        } else {
            format!("{} in progress", self.live_games.len())
        };
        let head = |more: Option<AnyElement>| {
            h_flex()
                .justify_between()
                .items_center()
                .gap(px(10.))
                .flex_wrap()
                .child(
                    h_flex()
                        .gap(px(10.))
                        .items_center()
                        .child(heading("Live games"))
                        .child(small(count_text.clone())),
                )
                .children(more)
        };
        if run.live.is_none() {
            return card()
                .child(head(None))
                .child(small(run.live_off.unwrap_or("Live boards are off.")))
                .into_any_element();
        }
        if self.live_games.is_empty() {
            return card()
                .child(head(None))
                .child(small(if run.active() {
                    "Waiting for the first moves…"
                } else {
                    "No games in progress."
                }))
                .into_any_element();
        }
        let this = cx.entity();
        let more = self.live_games.len() > LIVE_LIMIT;
        let toggle = more.then(|| {
            btn(
                "live-show-all",
                if self.live_all {
                    "Show fewer".to_owned()
                } else {
                    format!("Show all {}", self.live_games.len())
                },
            )
            .mini()
            .on_click(move |_, cx| {
                this.update(cx, |screen, cx| {
                    screen.live_all = !screen.live_all;
                    cx.notify();
                })
            })
        });
        let shown = if self.live_all {
            usize::MAX
        } else {
            LIVE_LIMIT
        };
        let mut grid = h_flex().flex_wrap().items_start().gap(px(12.));
        for game in self.live_games.iter().take(shown) {
            let Some((board, _, _)) = self.live_boards.get(&game.slot) else {
                continue;
            };
            let this = cx.entity();
            let slot = game.slot;
            let focused = self.live_focus == Some(slot);
            grid = grid.child(
                div()
                    .id(("live-card", slot))
                    .cursor_pointer()
                    .on_click(move |_, _, cx| {
                        this.update(cx, |screen, cx| screen.focus_live(slot, cx))
                    })
                    .child(game_card(game, board, px(LIVE_BOARD), &run.names, focused)),
            );
        }
        let focus = self
            .live_focus
            .and_then(|slot| self.live_games.iter().find(|game| game.slot == slot))
            .map(|game| self.render_focus(game));
        card()
            .child(head(toggle.map(IntoElement::into_any_element)))
            .children(focus)
            .child(grid)
            .into_any_element()
    }

    /// The finished games, filterable, with a row per game that opens it in Analyse.
    fn render_games(&self, run: &Run, cx: &mut Context<Self>) -> Div {
        let filter = self.games_filter;
        let counts = GameFilter::ALL.map(|f| {
            let count = run.games.iter().filter(|row| f.matches(row)).count();
            (
                f,
                SharedString::from(format!("{} ({count})", f.label())),
                false,
            )
        });
        let this = cx.entity();
        let bar = segmented(
            "tournament-filter",
            counts.into(),
            filter,
            move |f, _, cx| {
                this.update(cx, |screen, cx| {
                    screen.games_filter = f;
                    cx.notify();
                })
            },
        );
        let rows: Vec<&GameRow> = run
            .games
            .iter()
            .filter(|row| filter.matches(row))
            .take(200)
            .collect();
        let body = if run.games.is_empty() {
            small("No finished games yet.").into_any_element()
        } else if rows.is_empty() {
            small("No games match this filter.").into_any_element()
        } else {
            let mut table = v_flex().min_w(px(760.)).gap(px(2.)).child(
                h_flex()
                    .gap(px(12.))
                    .px(px(10.))
                    .py(px(6.))
                    .text_size(px(12.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(T.faint))
                    .child(div().w(px(72.)).child("#"))
                    .child(div().flex_1().min_w_0().child(ui::i18n::t("WHITE")))
                    .child(div().flex_1().min_w_0().child(ui::i18n::t("BLACK")))
                    .child(div().w(px(96.)).child(ui::i18n::t("RESULT")))
                    .child(div().flex_1().min_w_0().child(ui::i18n::t("TERMINATION")))
                    .child(div().w(px(96.)).child("")),
            );
            for row in rows {
                let this = cx.entity();
                let (number, index) = (row.game, row.pgn_index);
                let (chip_bg, chip_fg) = chip_colours(row, &run.names);
                let decisive = row.result != "1/2-1/2";
                table = table.child(
                    h_flex()
                        .id(("tournament-game", row.game as usize))
                        .gap(px(12.))
                        .px(px(10.))
                        .py(px(7.))
                        .rounded(px(6.))
                        .items_center()
                        .cursor_pointer()
                        .bg(rgb(if decisive { T.surface_2 } else { T.tour_card }))
                        .hover(|r| r.bg(rgb(T.hover)))
                        .text_size(px(13.))
                        .on_click(move |_, window, cx| {
                            this.update(cx, |screen, cx| {
                                screen.open_game(number, index, window, cx)
                            })
                        })
                        .child(
                            div()
                                .w(px(72.))
                                .font_family(MONO)
                                .text_color(rgb(T.muted))
                                .child(SharedString::from(format!("#{}", row.game))),
                        )
                        .child(name_cell(&row.white, &run.names))
                        .child(name_cell(&row.black, &run.names))
                        .child(
                            div().w(px(96.)).child(
                                div()
                                    .px(px(8.))
                                    .py(px(2.))
                                    .rounded(px(6.))
                                    .font_family(MONO)
                                    .bg(rgb(chip_bg))
                                    .text_color(rgb(chip_fg))
                                    .child(SharedString::from(row.result.clone())),
                            ),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .text_color(rgb(T.muted))
                                .child(SharedString::from(row.reason.clone())),
                        )
                        .child(
                            div()
                                .w(px(96.))
                                .text_color(rgb(T.engine_a))
                                .child(ui::i18n::t("Analyse →")),
                        ),
                );
            }
            div()
                .id("tournament-games-table")
                .overflow_x_scroll()
                .child(table)
                .into_any_element()
        };
        card()
            .child(
                h_flex()
                    .justify_between()
                    .items_center()
                    .flex_wrap()
                    .gap(px(12.))
                    .child(heading("Games (click to open in Analyse)"))
                    .child(bar),
            )
            .child(body)
    }

    fn render_focus(&self, game: &LiveGame) -> impl IntoElement + use<> {
        let (pos, _) = game.position();
        let start_ply = (pos.fullmoves().get() - 1) * 2 + u32::from(pos.turn().is_black());
        // The line of whoever searches now, from the current position.
        let search = if game.thinking.is_some() && game.thinking == game.white {
            Some(&game.white_search)
        } else if game.thinking.is_some() && game.thinking == game.black {
            Some(&game.black_search)
        } else {
            None
        };
        let pv = search
            .map(|search| pv_to_san(&pos, &search.pv))
            .filter(|sans| !sans.is_empty())
            .map(|sans| format_pv(start_ply, &sans))
            .unwrap_or_else(|| "–".into());
        let detail = |name: &Option<String>, search: &Search| {
            v_flex()
                .gap(px(2.))
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(SharedString::from(
                            name.clone().unwrap_or_else(|| "?".into()),
                        )),
                )
                .child(
                    div()
                        .font_family(MONO)
                        .text_size(px(13.))
                        .text_color(rgb(T.fg_2))
                        .child(SharedString::from(search_text(search, true))),
                )
        };
        panel().p(px(14.)).child(
            h_flex()
                .items_start()
                .gap(px(18.))
                .child(
                    v_flex()
                        .gap(px(6.))
                        .child(player_line(game, false, 13.))
                        .child(div().size(px(360.)).child(self.focus_board.clone()))
                        .child(player_line(game, true, 13.)),
                )
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .gap(px(14.))
                        .child(detail(&game.white, &game.white_search))
                        .child(detail(&game.black, &game.black_search))
                        .child(
                            v_flex().gap(px(4.)).child(section_label("Line")).child(
                                div()
                                    .font_family(MONO)
                                    .text_size(px(13.))
                                    .child(SharedString::from(pv)),
                            ),
                        )
                        .child(div().text_size(px(12.)).text_color(rgb(T.muted)).child(
                            SharedString::from(format!(
                                "Game {} · Move {} · {} plies played · click the board's card again to close",
                                game.number,
                                pos.fullmoves(),
                                game.moves.len()
                            )),
                        )),
                ),
        )
    }

    fn render_empty(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let folder = fastchess::tournaments_dir(&self.data_dir(cx));
        let new = cx.entity();
        let saved = self
            .render_saved(cx)
            .map(|saved| card().mt(px(14.)).child(saved));
        v_flex()
            .gap(px(10.))
            .max_w(px(620.))
            .child(
                div()
                    .text_size(px(18.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(ui::i18n::t("Engine against engine")),
            )
            .child(div().text_color(rgb(T.fg_2)).child(
                "Pick two or more engines and a time control, then start. fastchess plays the \
                     games in the background; results, the game list and its output appear here.",
            ))
            .child(div().text_color(rgb(T.muted)).text_size(px(13.)).child(
                "Each tournament gets its own folder with games.pgn and config.json, so it can \
                 be stopped and continued later, here or with fastchess -config file=config.json.",
            ))
            .child(
                mono(format!("Folder: {}", folder.display()))
                    .text_size(px(12.))
                    .text_color(rgb(T.muted)),
            )
            .child(
                div().mt(px(6.)).child(
                    btn("tournament-new-empty", ui::i18n::t("New tournament"))
                        .primary()
                        .h(38.)
                        .on_click(move |_, cx| new.update(cx, |screen, cx| screen.open_setup(cx))),
                ),
            )
            .children(saved)
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

/// Name, score and clock beside a live board: Black above it, White below. Scores are
/// from White's side.
fn player_line(game: &LiveGame, white: bool, size: f32) -> impl IntoElement + use<> {
    let (name, clock, search) = if white {
        (&game.white, game.wtime, &game.white_search)
    } else {
        (&game.black, game.btime, &game.black_search)
    };
    let thinking = game.thinking.is_some() && game.thinking == *name;
    h_flex()
        .gap(px(6.))
        .text_size(px(size))
        .child(
            div()
                .size(px(7.))
                .flex_none()
                .rounded_full()
                .when(thinking, |dot| dot.bg(rgb(T.good))),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .child(SharedString::from(
                    name.clone().unwrap_or_else(|| "…".into()),
                )),
        )
        .child(
            div()
                .font_family(MONO)
                .text_color(rgb(T.muted))
                .child(SharedString::from(search_text(search, false))),
        )
        .child(div().font_family(MONO).child(SharedString::from(
            clock.map(tournament_live::format_clock).unwrap_or_default(),
        )))
}

/// A panel of the tournament tab.
fn card() -> Div {
    v_flex()
        .gap(px(14.))
        .p(px(20.))
        .rounded(px(12.))
        .bg(rgb(T.tour_card))
        .border_1()
        .border_color(rgb(T.tour_card_border))
}

/// A tile inside a panel.
fn tile(label: &str) -> Div {
    v_flex()
        .min_w_0()
        .overflow_hidden()
        .gap(px(4.))
        .p(px(14.))
        .rounded(px(10.))
        .bg(rgb(T.tour_tile))
        .border_1()
        .border_color(rgb(T.tour_tile_border))
        .child(
            div()
                .text_size(px(12.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(T.faint))
                .child(SharedString::from(ui::i18n::t(label).to_uppercase())),
        )
}

fn heading(text: impl Into<SharedString>) -> Div {
    let text: SharedString = text.into();
    div()
        .text_size(px(13.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgb(T.muted))
        .child(SharedString::from(ui::i18n::t(&text).to_uppercase()))
}

fn small(text: impl Into<SharedString>) -> Div {
    div()
        .text_size(px(12.))
        .text_color(rgb(T.muted))
        .child(ui::i18n::t(&text.into()).to_owned())
}

fn big(text: impl Into<SharedString>) -> Div {
    div()
        .text_size(px(30.))
        .font_weight(FontWeight::BOLD)
        .child(ui::i18n::t(&text.into()).to_owned())
}

/// A thin bar filled to `fraction`.
fn meter(fraction: f32, colour: u32) -> Div {
    div()
        .h(px(6.))
        .w_full()
        .rounded(px(3.))
        .bg(rgb(T.tour_tile_border))
        .child(
            div()
                .h_full()
                .rounded(px(3.))
                .bg(rgb(colour))
                .w(relative(fraction.clamp(0., 1.))),
        )
}

fn swatch(colour: u32) -> Div {
    div()
        .size(px(8.))
        .flex_none()
        .rounded_full()
        .bg(rgb(colour))
}

fn name_chip(name: &str, colour: u32, size: f32) -> Div {
    h_flex()
        .gap(px(8.))
        .items_center()
        .child(swatch(colour))
        .child(
            div()
                .text_size(px(size))
                .font_weight(FontWeight::BOLD)
                .child(SharedString::from(name.to_owned())),
        )
}

fn ratio(label: &str, value: String) -> Div {
    h_flex()
        .justify_between()
        .gap(px(12.))
        .text_size(px(13.))
        .child(
            div()
                .text_color(rgb(T.muted))
                .child(SharedString::from(ui::i18n::t(label).to_owned())),
        )
        .child(div().font_family(MONO).child(SharedString::from(value)))
}

fn def_row(label: &str, value: String) -> Div {
    h_flex()
        .justify_between()
        .gap(px(12.))
        .text_size(px(13.))
        .child(
            div()
                .flex_none()
                .text_color(rgb(T.muted))
                .child(SharedString::from(ui::i18n::t(label).to_owned())),
        )
        .child(
            div()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .text_right()
                .child(SharedString::from(value)),
        )
}

/// Engine A is blue, engine B orange, any others grey.
fn engine_colour(names: &[String], name: &str) -> u32 {
    match names.iter().position(|engine| engine == name) {
        Some(0) => T.engine_a,
        Some(1) => T.engine_b,
        _ => T.muted,
    }
}

/// Where the eval bar's white part ends, from 0 (Black ahead) to 1 (White ahead).
fn white_share(score: Option<Score>) -> f32 {
    match score {
        None => 0.5,
        Some(Score::Mate(moves)) => {
            if moves > 0 {
                1.
            } else {
                0.
            }
        }
        Some(Score::Cp(cp)) => 1. / (1. + 10f32.powf(-(cp as f32) / 400.)),
    }
}

/// The search whose line a game card shows: whoever is thinking, else the last one with a score.
fn shown_search(game: &LiveGame) -> &Search {
    let black_turn = game.thinking.is_some() && game.thinking == game.black;
    if black_turn {
        &game.black_search
    } else if game.white_search.score.is_some() || game.thinking.is_some() {
        &game.white_search
    } else {
        &game.black_search
    }
}

/// The evaluation as a vertical bar: White's share fills from the bottom.
fn eval_bar(game: &LiveGame, edge: Pixels) -> ui::eval_bar::EvalBar {
    let share = white_share(shown_search(game).score);
    ui::eval_bar::EvalBar::new(("game-eval", game.number as usize), share, edge)
}

/// A player's name, colour and clock; the side to move gets a "to move" label and a lit clock.
fn player_row(game: &LiveGame, white: bool, names: &[String], to_move: bool) -> Div {
    let (name, clock) = if white {
        (&game.white, game.wtime)
    } else {
        (&game.black, game.btime)
    };
    let label = name.clone().unwrap_or_else(|| "…".into());
    let clock = clock
        .map(tournament_live::format_clock)
        .unwrap_or_else(|| "–".into());
    h_flex()
        .gap(px(8.))
        .items_center()
        .text_size(px(13.))
        .child(swatch(engine_colour(names, &label)))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .font_weight(FontWeight::SEMIBOLD)
                .child(SharedString::from(label)),
        )
        .children(to_move.then(|| {
            div()
                .text_size(px(11.))
                .text_color(rgb(T.live_fg))
                .child(ui::i18n::t("to move"))
        }))
        .child(
            div()
                .px(px(8.))
                .py(px(2.))
                .rounded(px(6.))
                .font_family(MONO)
                .bg(rgb(if to_move { T.live_bg } else { T.tour_tile }))
                .text_color(rgb(if to_move { T.live_fg } else { T.fg_2 }))
                .child(SharedString::from(clock)),
        )
}

/// One game: Black, the board with its eval bar, White, then the game number and evaluation.
/// Used by the tab and by the watch-all window.
pub(crate) fn game_card(
    game: &LiveGame,
    board: &Entity<BoardView>,
    edge: Pixels,
    names: &[String],
    focused: bool,
) -> Div {
    let (pos, _) = game.position();
    let black_to_move = pos.turn().is_black();
    let search = shown_search(game);
    let eval = search
        .score
        .map(Score::format)
        .unwrap_or_else(|| "–".into());
    let depth = search
        .depth
        .map(|depth| format!(" / d{depth}"))
        .unwrap_or_default();
    v_flex()
        .w(edge + px(40.))
        .gap(px(8.))
        .p(px(12.))
        .rounded(px(12.))
        .bg(rgb(T.tour_card))
        .border_1()
        .border_color(rgb(if focused {
            T.border_strong
        } else {
            T.tour_card_border
        }))
        .child(player_row(game, false, names, black_to_move))
        .child(
            h_flex()
                .gap(px(8.))
                .items_start()
                .child(eval_bar(game, edge))
                .child(div().size(edge).child(board.clone())),
        )
        .child(player_row(game, true, names, !black_to_move))
        .child(
            h_flex()
                .justify_between()
                .text_size(px(12.))
                .text_color(rgb(T.muted))
                .child(SharedString::from(format!("Game #{}", game.number)))
                .child(SharedString::from(format!("{eval}{depth}"))),
        )
}

/// Result chip colours: draws grey, engine A's wins blue, engine B's wins orange.
fn chip_colours(row: &GameRow, names: &[String]) -> (u32, u32) {
    let winner = match row.result.as_str() {
        "1-0" => Some(&row.white),
        "0-1" => Some(&row.black),
        _ => None,
    };
    match winner {
        None => (T.chip_draw_bg, T.chip_draw_fg),
        Some(name) if names.first() == Some(name) => (T.chip_a_bg, T.chip_a_fg),
        Some(name) if names.get(1) == Some(name) => (T.chip_b_bg, T.chip_b_fg),
        Some(_) => (T.tour_tile, T.fg_2),
    }
}

fn name_cell(name: &str, names: &[String]) -> Div {
    div()
        .flex_1()
        .min_w_0()
        .overflow_hidden()
        .whitespace_nowrap()
        .text_ellipsis()
        .child(
            h_flex()
                .gap(px(8.))
                .items_center()
                .child(swatch(engine_colour(names, name)))
                .child(SharedString::from(name.to_owned())),
        )
}

/// The results block as tiles, W/D/L and the pentanomial. Scores are from engine A's side.
fn results_body(n: &fastchess::Results, a: &str, b: &str) -> Div {
    let games = n.games.max(1) as f64;
    // Equal columns, so every tile has the same width and, row by row, the same height:
    // three across, or two by two once the LLR tile joins them.
    let mut tiles = div()
        .grid()
        .grid_cols(if n.llr.is_some() { 2 } else { 3 })
        .gap(px(12.));
    tiles = tiles.child(
        tile("Elo")
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_x(px(8.))
                    .items_end()
                    .child(big(format!("{:+.2}", n.elo)))
                    .child(small(format!("± {:.2}", n.elo_err))),
            )
            .child(small(format!("nElo {:+.2} ± {:.2}", n.nelo, n.nelo_err))),
    );
    tiles = tiles.child(
        tile("LOS")
            .child(big(format!("{:.1} %", n.los)))
            .child(meter(n.los as f32 / 100., T.engine_a)),
    );
    tiles = tiles.child(
        tile("Score")
            .child(big(format!("{:.1} / {}", n.points, n.games)))
            .child(small(format!(
                "{:.1}% · draws {:.0}%",
                n.score_pct,
                n.draws as f64 / games * 100.
            ))),
    );
    if let Some(llr) = n.llr {
        let span = (llr.upper - llr.lower).max(f64::EPSILON);
        tiles = tiles.child(
            tile("LLR")
                .child(big(format!("{:+.2}", llr.value)))
                .child(small(format!(
                    "bounds {:.2} to {:.2}",
                    llr.lower, llr.upper
                )))
                .child(meter(((llr.value - llr.lower) / span) as f32, T.engine_a)),
        );
    }

    let total = (n.wins + n.draws + n.losses).max(1) as f32;
    let wdl = v_flex()
        .gap(px(6.))
        .child(
            h_flex()
                .justify_between()
                .text_size(px(12.))
                .child(
                    h_flex()
                        .gap(px(6.))
                        .items_center()
                        .child(swatch(T.engine_a))
                        .child(SharedString::from(format!("{a} wins {}", n.wins))),
                )
                .child(small(format!("Draws {}", n.draws)))
                .child(
                    h_flex()
                        .gap(px(6.))
                        .items_center()
                        .child(SharedString::from(format!("{b} wins {}", n.losses)))
                        .child(swatch(T.engine_b)),
                ),
        )
        .child(
            h_flex()
                .h(px(14.))
                .w_full()
                .rounded(px(7.))
                .overflow_hidden()
                .child(
                    div()
                        .h_full()
                        .w(relative(n.wins as f32 / total))
                        .bg(rgb(T.engine_a)),
                )
                .child(
                    div()
                        .h_full()
                        .w(relative(n.draws as f32 / total))
                        .bg(rgb(T.border_strong)),
                )
                .child(
                    div()
                        .h_full()
                        .w(relative(n.losses as f32 / total))
                        .bg(rgb(T.engine_b)),
                ),
        );

    // Losing pairs are orange, the middle grey, winning pairs blue.
    let max = n.ptnml.iter().copied().max().unwrap_or(0).max(1) as f32;
    let bins = ["LL", "LD", "DD/WL", "WD", "WW"];
    let colours = [
        T.engine_b,
        T.engine_b,
        T.border_strong,
        T.engine_a,
        T.engine_a,
    ];
    let chart = v_flex()
        .flex_1()
        .min_w(px(240.))
        .gap(px(6.))
        .child(heading("Pentanomial"))
        .child(
            h_flex()
                .items_end()
                .gap(px(10.))
                .h(px(110.))
                .children((0..5).map(|i| {
                    v_flex()
                        .flex_1()
                        .h_full()
                        .justify_end()
                        .items_center()
                        .gap(px(4.))
                        .child(small(n.ptnml[i].to_string()))
                        .child(
                            div()
                                .w_full()
                                .h(px(72. * n.ptnml[i] as f32 / max))
                                .rounded(px(4.))
                                .bg(rgb(colours[i])),
                        )
                        .child(small(bins[i]))
                })),
        );
    let ratios = v_flex()
        .w(px(170.))
        .gap(px(10.))
        .child(ratio("Pairs ratio", format!("{:.2}", n.pairs_ratio)))
        .child(ratio("WL/DD ratio", format!("{:.2}", n.wl_dd_ratio)))
        .child(ratio("Draw ratio", format!("{:.1} %", n.draw_ratio)));

    v_flex().gap(px(16.)).child(tiles).child(wdl).child(
        h_flex()
            .items_end()
            .flex_wrap()
            .gap(px(20.))
            .child(chart)
            .child(ratios),
    )
}

/// What a tournament was started with, read back from its command.
fn locked_summary(run: &Run) -> Div {
    let args = &run.args;
    let engines = engines(args);
    let names: Vec<String> = engines.iter().map(|(name, _)| name.clone()).collect();
    let mut list = v_flex().gap(px(10.));
    for (name, cmd) in &engines {
        list = list.child(
            h_flex()
                .gap(px(10.))
                .items_start()
                .child(swatch(engine_colour(&names, name)))
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .child(
                            div()
                                .text_size(px(13.))
                                .font_weight(FontWeight::BOLD)
                                .child(SharedString::from(name.clone())),
                        )
                        .child(
                            div()
                                .font_family(MONO)
                                .text_size(px(11.))
                                .text_color(rgb(T.muted))
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .child(SharedString::from(cmd.clone())),
                        ),
                ),
        );
    }

    let each = flag_values(args, "-each");
    let limit = ["tc", "st", "nodes", "depth"]
        .into_iter()
        .find_map(|key| {
            kv(&each, key).map(|value| {
                if key == "tc" {
                    value.to_owned()
                } else {
                    format!("{key} {value}")
                }
            })
        })
        .unwrap_or_else(|| "–".into());
    let book = kv(&flag_values(args, "-openings"), "file")
        .map(|file| {
            Path::new(file)
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| file.to_owned())
        })
        .unwrap_or_else(|| "None".into());
    let schedule = format!(
        "{} · {}",
        if flag_value(args, "-tournament") == Some("gauntlet") {
            "Gauntlet"
        } else {
            "Round robin"
        },
        if has(args, "-repeat") {
            "2 games per round"
        } else {
            "1 game per round"
        }
    );
    let settings = [
        ("Time control", limit),
        (
            "Hash · Threads",
            format!(
                "{} MB · {}",
                kv(&each, "option.Hash").unwrap_or("–"),
                kv(&each, "option.Threads").unwrap_or("–")
            ),
        ),
        ("Schedule", schedule),
        (
            "Rounds",
            flag_value(args, "-rounds").unwrap_or("–").to_owned(),
        ),
        (
            "Concurrency",
            flag_value(args, "-concurrency").unwrap_or("–").to_owned(),
        ),
        ("Book", book),
    ];
    let mut defs = v_flex().gap(px(8.));
    for (label, value) in settings {
        defs = defs.child(def_row(label, value));
    }
    let on_off = |flag: &str| if has(args, flag) { "On" } else { "Off" };
    let sprt = if has(args, "-sprt") {
        let sprt = flag_values(args, "-sprt");
        format!(
            "elo0 {} · elo1 {}",
            kv(&sprt, "elo0").unwrap_or("–"),
            kv(&sprt, "elo1").unwrap_or("–")
        )
    } else {
        "Off".to_owned()
    };
    let draw = if has(args, "-draw") {
        let draw = flag_values(args, "-draw");
        format!(
            "From move {} · {} moves within ±{} cp",
            kv(&draw, "movenumber").unwrap_or("–"),
            kv(&draw, "movecount").unwrap_or("–"),
            kv(&draw, "score").unwrap_or("–")
        )
    } else {
        "Off".to_owned()
    };
    let resign = if has(args, "-resign") {
        let resign = flag_values(args, "-resign");
        format!(
            "{} moves beyond {} cp{}",
            kv(&resign, "movecount").unwrap_or("–"),
            kv(&resign, "score").unwrap_or("–"),
            if kv(&resign, "twosided") == Some("true") {
                " · both engines"
            } else {
                ""
            }
        )
    } else {
        "Off".to_owned()
    };
    let mut rules = v_flex().gap(px(8.));
    for (label, value) in [
        ("Draw adjudication", draw),
        ("Resign adjudication", resign),
        ("Recover crashes", on_off("-recover").to_owned()),
        ("SPRT", sprt),
    ] {
        rules = rules.child(def_row(label, value));
    }

    v_flex()
        .w_full()
        .gap(px(20.))
        .child(
            card()
                .w_full()
                .gap(px(12.))
                .child(heading("Engines"))
                .child(list),
        )
        .child(
            card()
                .w_full()
                .gap(px(12.))
                .child(heading("Settings"))
                .child(defs)
                .child(divider())
                .child(rules),
        )
}

/// Engines in the order a command gives them, as (name, executable).
fn engines(args: &[String]) -> Vec<(String, String)> {
    args.iter()
        .enumerate()
        .filter(|(_, arg)| arg.as_str() == "-engine")
        .map(|(index, _)| {
            let values: Vec<&str> = args[index + 1..]
                .iter()
                .take_while(|arg| !arg.starts_with('-'))
                .map(String::as_str)
                .collect();
            (
                kv(&values, "name").unwrap_or("engine").to_owned(),
                kv(&values, "cmd").unwrap_or("").to_owned(),
            )
        })
        .collect()
}

/// The values after a flag, up to the next flag.
fn flag_values<'a>(args: &'a [String], flag: &str) -> Vec<&'a str> {
    let Some(start) = args.iter().position(|arg| arg.as_str() == flag) else {
        return Vec::new();
    };
    args[start + 1..]
        .iter()
        .take_while(|arg| !arg.starts_with('-'))
        .map(String::as_str)
        .collect()
}

fn flag_value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    flag_values(args, flag).first().copied()
}

/// The value of `key=value` among `values`.
fn kv<'a>(values: &[&'a str], key: &str) -> Option<&'a str> {
    values
        .iter()
        .find_map(|&value| value.strip_prefix(key)?.strip_prefix('='))
}

fn has(args: &[String], flag: &str) -> bool {
    args.iter().any(|arg| arg.as_str() == flag)
}

/// Time left at the average pace of finished games: average game time × games left ÷
/// concurrency.
fn eta(run: &Run) -> String {
    let (time, count) = run.game_time;
    if count == 0 || run.concurrency == 0 {
        return "–".into();
    }
    let remaining = run.total.max(run.played()).saturating_sub(run.played());
    let average = time.as_secs_f64() / f64::from(count);
    format_duration(average * remaining as f64 / run.concurrency as f64)
}

fn format_duration(secs: f64) -> String {
    let secs = secs.round() as u64;
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m {:02}s", secs / 60, secs % 60)
    } else {
        format!("{}h {:02}m", secs / 3600, secs / 60 % 60)
    }
}

/// Shows a folder in the system's file manager.
fn reveal_folder(path: &Path) {
    #[cfg(target_os = "windows")]
    let program = "explorer";
    #[cfg(target_os = "macos")]
    let program = "open";
    #[cfg(all(unix, not(target_os = "macos")))]
    let program = "xdg-open";
    if let Err(err) = std::process::Command::new(program).arg(path).spawn() {
        log::warn!("could not open {}: {err}", path.display());
    }
}

fn search_text(search: &Search, long: bool) -> String {
    let Some(score) = search.score else {
        return String::new();
    };
    if !long {
        let depth = search
            .depth
            .map(|depth| format!("/{depth}"))
            .unwrap_or_default();
        return format!("{}{depth}", score.format());
    }
    let nodes = search
        .nodes
        .map(|nodes| format!(" · {nodes} nodes"))
        .unwrap_or_default();
    format!(
        "{} · depth {}{nodes}",
        score.format(),
        search.depth.unwrap_or(0)
    )
}
