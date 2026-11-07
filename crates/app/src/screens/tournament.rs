//! Engine tournaments run by fastchess: set one up in a form, paste a command line or a
//! saved config to continue, and follow the results live.

use std::{
    collections::{HashMap, VecDeque},
    path::PathBuf,
    sync::{Arc, Mutex, atomic::AtomicBool},
    time::Duration,
};

use chess_core::{Position as _, format_pv, pv_to_san};

use engine::EngineId;
use gpui::{
    AppContext as _, ClipboardItem, Context, Entity, FontWeight, IntoElement, Render, SharedString,
    Task, WeakEntity, Window, div, prelude::*, px, rgb,
};
use gpui_component::input::{InputState, Textarea, TextareaState};
use ui::{
    BoardView, MONO, T,
    widgets::{btn, check, h_flex, mono, panel, section_label, segmented, v_flex},
};

use crate::{
    controls::{field, new_input, text_input},
    fastchess::{self, Limit, Line, Participant, Pasted, Setup, StopHandle},
    model::{AppModel, pick_path},
    shell::Workspace,
    tournament_live::{self, LiveGame, LiveGames, Search},
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
            Line::Started { total, .. } => self.total = total,
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
    recover: bool,
    limit_value: Entity<InputState>,
    hash: Entity<InputState>,
    threads: Entity<InputState>,
    rounds: Entity<InputState>,
    concurrency: Entity<InputState>,
    openings: Entity<InputState>,
    plies: Entity<InputState>,
    elo0: Entity<InputState>,
    elo1: Entity<InputState>,
    alpha: Entity<InputState>,
    beta: Entity<InputState>,
    maxmoves: Entity<InputState>,
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
    /// Whether each fastchess executable offers the `-events` stream.
    events_support: HashMap<PathBuf, bool>,
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
            recover: true,
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
            events_support: HashMap::new(),
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
            resign_adjudication: self.resign_adjudication,
            maxmoves: self.text(&self.maxmoves, cx),
            recover: self.recover,
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
        let setup = self.setup(cx);
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
            Some("Live boards need a fastchess build with the -events option.")
        } else if tournament_live::has_own_events(&launch.args) {
            Some("Live boards are off because this command sets its own -events option.")
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
            _task: task,
        });
        self.live_games.clear();
        self.live_boards.clear();
        self.live_focus = None;
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
        let show = |board: &Entity<BoardView>,
                    shown: &mut (u64, usize),
                    game: &LiveGame,
                    cx: &mut Context<Self>| {
            if *shown == (game.number, game.moves.len()) {
                return;
            }
            let animate = shown.0 == game.number && shown.1 + 1 == game.moves.len();
            let (pos, last) = game.position();
            board.update(cx, |board, cx| {
                board.set_prefs(prefs, cx);
                board.set_position(pos, last, animate, cx);
            });
            *shown = (game.number, game.moves.len());
        };
        for game in &games {
            let entry = self.live_boards.entry(game.slot).or_insert_with(|| {
                let board = cx.new(|_| BoardView::new(("live-board", game.slot)).read_only());
                (board, 0, usize::MAX)
            });
            let mut shown = (entry.1, entry.2);
            show(&entry.0, &mut shown, game, cx);
            (entry.1, entry.2) = shown;
        }
        if let Some(game) = self
            .live_focus
            .and_then(|slot| games.iter().find(|game| game.slot == slot))
        {
            let board = self.focus_board.clone();
            let mut shown = self.focus_shown;
            show(&board, &mut shown, game, cx);
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
                            row.child(btn("fastchess-download", "Download now").on_click(
                                move |_, cx| this.update(cx, |screen, cx| screen.download(cx)),
                            ))
                        },
                    )
                    .when(self.install_cancel.is_some(), |row| {
                        row.child(btn("fastchess-cancel", "Cancel download").on_click(
                            move |_, cx| cancel.update(cx, |screen, cx| screen.cancel_download(cx)),
                        ))
                    }),
            )
            .child(field(
                "Own executable (optional)",
                h_flex()
                    .gap(px(6.))
                    .child(div().flex_1().child(text_input(&self.fastchess_path)))
                    .child(btn("fastchess-path-save", "Use").on_click(move |_, cx| {
                        save.update(cx, |screen, cx| screen.save_fastchess_path(cx))
                    })),
            ))
    }

    fn render_engines(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let engines = self.model.read(cx).config.engines.clone();
        let mut list = v_flex().gap(px(6.));
        if engines.is_empty() && self.extra_engines.is_empty() {
            list = list.child(div().text_size(px(13.)).text_color(rgb(T.muted)).child(
                "No engines yet. Add them in Settings › Engines, or add an executable here.",
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
            list = list.child(
                check(("tournament-engine", id as usize), order.is_some(), label)
                    .text(13.)
                    .on_change(move |on, _, cx| {
                        this.update(cx, |screen, cx| screen.toggle_engine(id, on, cx))
                    }),
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
                            .tooltip("Remove")
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
            .child(section_label("Engines"))
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
        let limit_this = cx.entity();
        let games_this = cx.entity();
        let type_this = cx.entity();
        let order_this = cx.entity();
        let browse = cx.entity();
        let _ = window;
        v_flex()
            .gap(px(14.))
            .child(self.render_engines(cx))
            .child(section_label("Time"))
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
            .child(pair(
                field("Hash (MB)", text_input(&self.hash)),
                field("Threads", text_input(&self.threads)),
            ))
            .child(section_label("Schedule"))
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
            .child(pair(
                field("Rounds", text_input(&self.rounds)),
                field("Concurrency", text_input(&self.concurrency)),
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
            .child(section_label("Openings"))
            .child(field(
                "Book (.epd or .pgn)",
                h_flex()
                    .gap(px(6.))
                    .child(div().flex_1().child(text_input(&self.openings)))
                    .child(
                        btn("tournament-book", "Browse…").on_click(move |window, cx| {
                            browse.update(cx, |screen, cx| screen.browse_openings(window, cx))
                        }),
                    ),
            ))
            .child(pair(
                field(
                    "Order",
                    segmented(
                        "tournament-order",
                        vec![
                            (true, "Random".into(), false),
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
            .child(section_label("Stopping and adjudication"))
            .child(
                self.toggle("tournament-sprt", self.sprt, "SPRT", cx, |screen, on| {
                    screen.sprt = on
                }),
            )
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
            })
            .child(self.toggle(
                "tournament-draw",
                self.draw_adjudication,
                "Draw adjudication (from move 32, 6 moves within ±10 cp)",
                cx,
                |screen, on| screen.draw_adjudication = on,
            ))
            .child(self.toggle(
                "tournament-resign",
                self.resign_adjudication,
                "Resign adjudication (3 moves beyond 600 cp, both engines)",
                cx,
                |screen, on| screen.resign_adjudication = on,
            ))
            .child(field("Maximum moves", text_input(&self.maxmoves)))
            .child(self.toggle(
                "tournament-recover",
                self.recover,
                "Recover crashed engines",
                cx,
                |screen, on| screen.recover = on,
            ))
            .child(field("Extra fastchess options", text_input(&self.extra)))
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

    fn render_left(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let source_this = cx.entity();
        let start = cx.entity();
        let busy = self.run.as_ref().is_some_and(Run::active) || self.pending.is_some();
        let source = self.source;
        let body = match source {
            Source::Form => self.render_form(window, cx).into_any_element(),
            Source::Paste => self.render_paste(cx).into_any_element(),
        };
        v_flex()
            .id("tournament-setup")
            .w(px(380.))
            .h_full()
            .flex_none()
            .overflow_y_scroll()
            .bg(rgb(T.surface))
            .border_r_1()
            .border_color(rgb(T.border))
            .child(
                v_flex()
                    .flex_none()
                    .gap(px(16.))
                    .px(px(18.))
                    .pt(px(20.))
                    .pb(px(24.))
                    .child(
                        div()
                            .text_size(px(18.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Engine tournament"),
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
                    )
                    .children(self.render_saved(cx)),
            )
    }

    fn render_run(&self, run: &Run, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let (status, colour) = match &run.status {
            Status::Running => ("Running".to_owned(), T.good),
            Status::Stopping => (
                "Stopping: fastchess is saving its config. Stop again to kill it.".to_owned(),
                T.faint,
            ),
            Status::Finished(text) => (text.clone(), T.fg_2),
            Status::Failed(text) => (text.clone(), T.warn),
        };
        let total = run.total.max(run.played());
        let fraction = if total == 0 {
            0.
        } else {
            run.played() as f32 / total as f32
        };
        let summary = fastchess::summarize(&run.results);
        let stop = cx.entity();
        let resume = cx.entity();
        let copy = cx.entity();
        let copy_config = cx.entity();
        let open_last = cx.entity();
        let (dir, cwd, config) = (run.dir.clone(), run.cwd.clone(), run.config.clone());
        let can_resume = !run.active() && run.config.is_file();
        let last_index = run.games.front().map(|game| (game.game, game.pgn_index));
        let tile = |label: &'static str, value: Option<String>| {
            v_flex()
                .gap(px(2.))
                .min_w(px(150.))
                .child(section_label(label))
                .child(
                    div()
                        .font_family(MONO)
                        .text_size(px(14.))
                        .child(SharedString::from(value.unwrap_or_else(|| "–".into()))),
                )
        };
        let mut games = v_flex().gap(px(2.));
        for row in run.games.iter().take(200) {
            let this = cx.entity();
            let (number, index) = (row.game, row.pgn_index);
            games = games.child(
                h_flex()
                    .id(("tournament-game", row.game as usize))
                    .gap(px(10.))
                    .px(px(6.))
                    .py(px(3.))
                    .rounded(px(4.))
                    .cursor_pointer()
                    .hover(|row| row.bg(rgb(T.selected)))
                    .text_size(px(13.))
                    .on_click(move |_, window, cx| {
                        this.update(cx, |screen, cx| screen.open_game(number, index, window, cx))
                    })
                    .child(
                        div()
                            .w(px(56.))
                            .font_family(MONO)
                            .text_color(rgb(T.muted))
                            .child(SharedString::from(format!("#{}", row.game))),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .child(SharedString::from(format!("{} – {}", row.white, row.black))),
                    )
                    .child(
                        div()
                            .w(px(64.))
                            .font_family(MONO)
                            .child(SharedString::from(row.result.clone())),
                    )
                    .child(
                        div()
                            .w(px(240.))
                            .overflow_hidden()
                            .text_color(rgb(T.muted))
                            .child(SharedString::from(row.reason.clone())),
                    ),
            );
        }
        let mut log = v_flex();
        for line in run.log.iter().rev().take(150) {
            log = log.child(
                div()
                    .font_family(MONO)
                    .text_size(px(12.))
                    .text_color(rgb(T.fg_2))
                    .child(SharedString::from(line.clone())),
            );
        }
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
        v_flex()
            .gap(px(16.))
            .child(
                h_flex()
                    .gap(px(10.))
                    .child(div().size(px(9.)).rounded_full().bg(rgb(colour)))
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(14.))
                            .child(SharedString::from(status)),
                    )
                    .when(run.active(), |row| {
                        row.child(
                            btn("tournament-stop", "Stop").on_click(move |_, cx| {
                                stop.update(cx, |screen, cx| screen.stop(cx))
                            }),
                        )
                    })
                    .when(can_resume, |row| {
                        row.child(btn("tournament-resume", "Continue").primary().on_click(
                            move |_, cx| {
                                let (dir, cwd, config) = (dir.clone(), cwd.clone(), config.clone());
                                resume.update(cx, |screen, cx| screen.resume(dir, cwd, config, cx))
                            },
                        ))
                    })
                    .when_some(last_index, |row, (number, index)| {
                        row.child(btn("tournament-open-last", "Open last game").on_click(
                            move |window, cx| {
                                open_last.update(cx, |screen, cx| {
                                    screen.open_game(number, index, window, cx)
                                })
                            },
                        ))
                    })
                    .child(
                        btn("tournament-copy", "Copy command").on_click(move |_, cx| {
                            copy.update(cx, |screen, cx| screen.copy_command(cx))
                        }),
                    )
                    .child(
                        btn("tournament-copy-config", "Copy config").on_click(move |_, cx| {
                            copy_config.update(cx, |screen, cx| screen.copy_config(cx))
                        }),
                    ),
            )
            .child(
                v_flex()
                    .gap(px(6.))
                    .child(
                        h_flex()
                            .gap(px(16.))
                            .text_size(px(13.))
                            .text_color(rgb(T.muted))
                            .child(div().flex_none().child(SharedString::from(format!(
                                "{} of {} games",
                                run.played(),
                                total
                            ))))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .text_right()
                                    .child(SharedString::from(run.dir.display().to_string())),
                            ),
                    )
                    .child(
                        div()
                            .h(px(6.))
                            .w_full()
                            .rounded(px(3.))
                            .bg(rgb(T.bg))
                            .child(
                                div()
                                    .h_full()
                                    .rounded(px(3.))
                                    .bg(rgb(T.fg_2))
                                    .w(gpui::relative(fraction.clamp(0., 1.))),
                            ),
                    ),
            )
            .child(self.render_live(run, cx))
            .child(
                panel().p(px(16.)).child(
                    v_flex()
                        .gap(px(14.))
                        .children(summary.heading.clone().map(|heading| {
                            div()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(SharedString::from(heading))
                        }))
                        .child(
                            h_flex()
                                .flex_wrap()
                                .gap(px(18.))
                                .child(tile("Elo", summary.elo.clone()))
                                .child(tile("LLR", summary.llr.clone()))
                                .child(tile("Games", summary.games.clone()))
                                .child(tile("Pentanomial", summary.ptnml.clone()))
                                .child(tile("LOS", summary.los.clone())),
                        )
                        // Head-to-head results fill the tiles; tables for more engines
                        // are shown as fastchess prints them.
                        .when(!run.results.is_empty() && summary.elo.is_none(), |column| {
                            column.child(
                                div()
                                    .id("tournament-results-block")
                                    .overflow_x_scroll()
                                    .child(block),
                            )
                        })
                        .when(run.results.is_empty(), |column| {
                            column.child(
                                div()
                                    .text_size(px(13.))
                                    .text_color(rgb(T.muted))
                                    .child("Results appear after the first finished game."),
                            )
                        }),
                ),
            )
            .child(
                v_flex()
                    .gap(px(6.))
                    .child(section_label("Games (click to open in Analyse)"))
                    .child(panel().p(px(6.)).child(if run.games.is_empty() {
                        div()
                            .p(px(6.))
                            .text_size(px(13.))
                            .text_color(rgb(T.muted))
                            .child("No finished games yet.")
                            .into_any_element()
                    } else {
                        games.into_any_element()
                    })),
            )
            .child(
                v_flex()
                    .gap(px(6.))
                    .child(section_label("fastchess output"))
                    .child(panel().p(px(10.)).child(log)),
            )
    }

    fn render_live(&self, run: &Run, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let count = if self.live_games.is_empty() {
            String::new()
        } else {
            format!("{} in progress", self.live_games.len())
        };
        let header = || {
            h_flex()
                .gap(px(10.))
                .child(section_label("Live games"))
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(rgb(T.muted))
                        .child(SharedString::from(count.clone())),
                )
        };
        let note = |text: &'static str| {
            v_flex().gap(px(6.)).child(header()).child(
                div()
                    .text_size(px(13.))
                    .text_color(rgb(T.muted))
                    .child(text),
            )
        };
        if run.live.is_none() {
            return note(run.live_off.unwrap_or("Live boards are off.")).into_any_element();
        }
        if self.live_games.is_empty() {
            return note(if run.active() {
                "Waiting for the first moves…"
            } else {
                "No games in progress."
            })
            .into_any_element();
        }
        let toggle = cx.entity();
        let more = self.live_games.len() > LIVE_LIMIT;
        let header = header().when(more, |row| {
            row.child(
                btn(
                    "live-show-all",
                    if self.live_all {
                        "Show fewer".to_owned()
                    } else {
                        format!("Show all {}", self.live_games.len())
                    },
                )
                .h(24.)
                .pad(7.)
                .text(12.)
                .on_click(move |_, cx| {
                    toggle.update(cx, |screen, cx| {
                        screen.live_all = !screen.live_all;
                        cx.notify();
                    })
                }),
            )
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
                v_flex()
                    .id(("live-card", slot))
                    .w(px(216.))
                    .gap(px(4.))
                    .p(px(6.))
                    .rounded(px(6.))
                    .border_1()
                    .border_color(rgb(if focused { T.fg_2 } else { T.border }))
                    .bg(rgb(T.surface))
                    .cursor_pointer()
                    .on_click(move |_, _, cx| {
                        this.update(cx, |screen, cx| screen.focus_live(slot, cx))
                    })
                    .child(player_line(game, false, 12.))
                    .child(div().size(px(202.)).child(board.clone()))
                    .child(player_line(game, true, 12.)),
            );
        }
        let focus = self
            .live_focus
            .and_then(|slot| self.live_games.iter().find(|game| game.slot == slot))
            .map(|game| self.render_focus(game));
        v_flex()
            .gap(px(8.))
            .child(header)
            .children(focus)
            .child(grid)
            .into_any_element()
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

    fn render_empty(&self, cx: &Context<Self>) -> impl IntoElement + use<> {
        let folder = fastchess::tournaments_dir(&self.data_dir(cx));
        v_flex()
            .gap(px(10.))
            .max_w(px(620.))
            .child(
                div()
                    .text_size(px(18.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Engine against engine"),
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
    }
}

impl Render for TournamentScreen {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let left = self.render_left(window, cx);
        let right = match &self.run {
            Some(run) => self.render_run(run, cx).into_any_element(),
            None => self.render_empty(cx).into_any_element(),
        };
        h_flex().items_start().size_full().child(left).child(
            div()
                .id("tournament-run")
                .flex_1()
                .min_w_0()
                .h_full()
                .overflow_y_scroll()
                .p(px(24.))
                .child(right),
        )
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
