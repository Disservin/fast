//! Runs engines over EPD test positions on worker threads: each worker owns one engine
//! process, takes positions from a queue shared by that engine's workers, and reports
//! progress and results over a channel the Test positions tab reads.

use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use chess_core::EpdPosition;
use engine::{EngineConfig, EngineEvent, EngineHandle, Limits, Score, SearchRequest};
use smol::channel::Sender;

use crate::config::{TestGoal, TestLimitKind, TestPositionSettings};

/// Live progress is sent at most this often per worker.
const PROGRESS_EVERY: Duration = Duration::from_millis(200);
/// A worker that loses its engine this many times in a row gives up.
const MAX_CRASHES: u32 = 3;

/// Everything a run needs, fixed when it starts.
#[derive(Clone, Debug)]
pub struct RunSettings {
    pub goal: TestGoal,
    pub limits: Limits,
    pub hash_mb: Option<u32>,
    pub clear_hash: bool,
    /// Run all engines at once, each with this many processes; otherwise one
    /// process at a time, engine after engine.
    pub parallel: Option<usize>,
    pub syzygy: Option<PathBuf>,
}

impl RunSettings {
    pub fn from_saved(saved: &TestPositionSettings) -> Self {
        let limits = match saved.limit {
            TestLimitKind::Depth => Limits {
                depth: Some(saved.depth.max(1)),
                ..Default::default()
            },
            TestLimitKind::Nodes => Limits {
                nodes: Some(saved.nodes.max(1)),
                ..Default::default()
            },
            TestLimitKind::Time => Limits {
                movetime_ms: Some(saved.time_ms.max(1)),
                ..Default::default()
            },
        };
        Self {
            goal: saved.goal,
            limits,
            hash_mb: saved.hash_mb,
            clear_hash: saved.clear_hash,
            parallel: saved.parallel.then_some(saved.processes.max(1) as usize),
            syzygy: saved
                .syzygy
                .then(|| saved.syzygy_path.clone())
                .flatten()
                .filter(|path| !path.as_os_str().is_empty()),
        }
    }

    /// The `go` limits for one position: a mate search also asks for that mate.
    fn limits_for(&self, record: &EpdPosition) -> Limits {
        let mut limits = self.limits;
        if self.goal == TestGoal::Mate {
            limits.mate = record.mate;
        }
        limits
    }
}

/// Why a parsed position cannot be run for a goal, if it cannot.
pub fn unusable(record: &EpdPosition, goal: TestGoal) -> Option<&'static str> {
    match goal {
        TestGoal::BestMove if record.best.is_empty() && record.avoid.is_empty() => {
            Some("no bm or am operation")
        }
        TestGoal::Mate if record.mate.is_none() => Some("no dm operation"),
        _ => None,
    }
}

/// When the engine settled on a solution and kept it to the end.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Found {
    pub depth: u32,
    pub time_ms: u64,
    pub nodes: u64,
}

/// What a search has shown so far, or showed in the end.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Search {
    pub depth: u32,
    pub score: Option<Score>,
    /// The move the engine prefers (its PV's first move, then its `bestmove`), in SAN.
    pub best: Option<String>,
    pub nodes: u64,
    pub time_ms: u64,
    pub found: Option<Found>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cell {
    Waiting,
    Running(Search),
    Solved(Search),
    Failed(Search),
    Error(String),
}

impl Cell {
    pub fn done(&self) -> bool {
        matches!(self, Self::Solved(_) | Self::Failed(_) | Self::Error(_))
    }
}

#[derive(Debug)]
pub enum RunEvent {
    /// `engine` indexes the run's engines, `position` the run's positions.
    Cell {
        engine: usize,
        position: usize,
        cell: Cell,
    },
    /// All workers have returned.
    Finished,
}

/// Starts the run in the background. Results arrive on `events`; setting `cancel`
/// stops every engine soon after.
pub fn start(
    engines: Vec<EngineConfig>,
    positions: Arc<Vec<EpdPosition>>,
    settings: RunSettings,
    cancel: Arc<AtomicBool>,
    events: Sender<RunEvent>,
) {
    let spawned = thread::Builder::new()
        .name("test-suite".to_owned())
        .spawn(move || {
            let queue = || Arc::new(Mutex::new((0..positions.len()).collect::<VecDeque<_>>()));
            match settings.parallel {
                Some(processes) => {
                    let mut workers = Vec::new();
                    for (slot, engine) in engines.iter().enumerate() {
                        let queue = queue();
                        for process in 0..processes.min(positions.len()).max(1) {
                            let worker = Worker {
                                engine: engine.clone(),
                                slot,
                                positions: positions.clone(),
                                queue: queue.clone(),
                                settings: settings.clone(),
                                cancel: cancel.clone(),
                                events: events.clone(),
                                game_base: (process as u64) << 32,
                            };
                            match thread::Builder::new()
                                .name("test-suite-worker".to_owned())
                                .spawn(move || worker.run())
                            {
                                Ok(handle) => workers.push(handle),
                                Err(err) => log::error!("could not start test worker: {err}"),
                            }
                        }
                    }
                    for worker in workers {
                        let _ = worker.join();
                    }
                }
                None => {
                    for (slot, engine) in engines.iter().enumerate() {
                        if cancel.load(Ordering::Relaxed) {
                            break;
                        }
                        Worker {
                            engine: engine.clone(),
                            slot,
                            positions: positions.clone(),
                            queue: queue(),
                            settings: settings.clone(),
                            cancel: cancel.clone(),
                            events: events.clone(),
                            game_base: 0,
                        }
                        .run();
                    }
                }
            }
            let _ = events.send_blocking(RunEvent::Finished);
        });
    if let Err(err) = spawned {
        log::error!("could not start test suite thread: {err}");
    }
}

struct Worker {
    engine: EngineConfig,
    slot: usize,
    positions: Arc<Vec<EpdPosition>>,
    queue: Arc<Mutex<VecDeque<usize>>>,
    settings: RunSettings,
    cancel: Arc<AtomicBool>,
    events: Sender<RunEvent>,
    /// Keeps this worker's game ids apart from its siblings'.
    game_base: u64,
}

/// Quits the process when the worker is done with it.
struct Process(EngineHandle);

impl Drop for Process {
    fn drop(&mut self) {
        self.0.quit();
    }
}

enum Searched {
    Done(Cell),
    Crashed(String),
    Cancelled,
}

impl Worker {
    fn send(&self, position: usize, cell: Cell) {
        let _ = self.events.send_blocking(RunEvent::Cell {
            engine: self.slot,
            position,
            cell,
        });
    }

    fn next(&self) -> Option<usize> {
        self.queue.lock().ok()?.pop_front()
    }

    fn knows(&self, option: &str) -> bool {
        // Without a saved schema, send it and let the engine ignore what it lacks.
        self.engine.info.options.is_empty()
            || self.engine.info.options.iter().any(|o| o.name == option)
    }

    fn spawn(&self) -> Process {
        let mut options: Vec<(String, String)> = self
            .engine
            .options
            .iter()
            .filter(|(name, _)| !matches!(name.as_str(), "MultiPV" | "Hash" | "SyzygyPath"))
            .map(|(name, value)| (name.clone(), value.clone()))
            .collect();
        let hash = match self.settings.hash_mb {
            Some(hash) if self.knows("Hash") => Some(hash.to_string()),
            Some(_) => None,
            None => self.engine.options.get("Hash").cloned(),
        };
        if let Some(hash) = hash {
            options.push(("Hash".to_owned(), hash));
        }
        if self.knows("MultiPV") {
            options.push(("MultiPV".to_owned(), "1".to_owned()));
        }
        // Tablebases off also drops a path the engine is configured with.
        let syzygy = match &self.settings.syzygy {
            Some(path) if self.knows("SyzygyPath") => Some(path.display().to_string()),
            Some(_) => None,
            None => self.engine.default_value("SyzygyPath"),
        };
        if let Some(path) = syzygy {
            options.push(("SyzygyPath".to_owned(), path));
        }
        Process(EngineHandle::spawn(
            self.engine.path.clone(),
            Vec::new(),
            options,
        ))
    }

    fn run(self) {
        let mut process = self.spawn();
        let mut crashes = 0;
        let mut search_id = 0;
        while let Some(index) = self.next() {
            if self.cancel.load(Ordering::Relaxed) {
                break;
            }
            search_id += 1;
            match self.search(&process.0, index, search_id) {
                Searched::Done(cell) => {
                    crashes = 0;
                    self.send(index, cell);
                }
                Searched::Cancelled => {
                    self.send(index, Cell::Waiting);
                    break;
                }
                Searched::Crashed(reason) => {
                    crashes += 1;
                    self.send(index, Cell::Error(format!("engine stopped: {reason}")));
                    if crashes >= MAX_CRASHES {
                        while let Some(index) = self.next() {
                            self.send(index, Cell::Error("engine keeps crashing".to_owned()));
                        }
                        return;
                    }
                    process = self.spawn();
                }
            }
        }
    }

    fn search(&self, handle: &EngineHandle, index: usize, search_id: u64) -> Searched {
        let record = &self.positions[index];
        let goal = self.settings.goal;
        // Clearing the hash: a new game sends `ucinewgame`, and engines with a
        // Clear Hash button get it pressed too. Otherwise the worker stays one game.
        let game = if self.settings.clear_hash {
            if self
                .engine
                .info
                .options
                .iter()
                .any(|o| o.name == "Clear Hash")
            {
                handle.set_option("Clear Hash", None);
            }
            self.game_base + index as u64 + 1
        } else {
            self.game_base
        };
        let events = handle.events();
        // Anything left from an earlier search or the handshake.
        while events.try_recv().is_ok() {}
        handle.search(SearchRequest {
            chess960: chess_core::fen_variant(&record.fen) == Some(chess_core::Variant::Chess960),
            fen: record.fen.clone(),
            moves: Vec::new(),
            search_moves: Vec::new(),
            limits: Some(self.settings.limits_for(record)),
            search_id,
            game,
        });
        let started = Instant::now();
        let mut search = Search::default();
        let mut solving = false;
        let mut last_sent: Option<Instant> = None;
        self.send(index, Cell::Running(search.clone()));
        loop {
            if self.cancel.load(Ordering::Relaxed) {
                handle.stop();
                return Searched::Cancelled;
            }
            let event = smol::block_on(smol::future::or(
                async { events.recv().await.map(Some) },
                async {
                    smol::Timer::after(Duration::from_millis(100)).await;
                    Ok(None)
                },
            ));
            match event {
                Ok(None) => continue,
                Ok(Some(EngineEvent::Analysis {
                    search_id: from,
                    update,
                })) if from == search_id => {
                    if update.multipv > 1 {
                        continue;
                    }
                    search.depth = search.depth.max(update.depth);
                    search.nodes = update.nodes.unwrap_or(search.nodes);
                    search.time_ms = update
                        .time_ms
                        .unwrap_or_else(|| started.elapsed().as_millis() as u64);
                    // Bound lines are not the engine's settled view of the position,
                    // and progress lines carry no move.
                    if update.bound || update.pv.is_empty() {
                        continue;
                    }
                    if update.score.is_some() {
                        search.score = update.score;
                    }
                    let first = update.pv.first().and_then(|uci| record.uci_move(uci));
                    if let Some(mv) = &first {
                        search.best = Some(record.san(mv));
                    }
                    let correct = match goal {
                        TestGoal::BestMove => first
                            .as_ref()
                            .and_then(|mv| record.move_solves(mv))
                            .unwrap_or(false),
                        TestGoal::Mate => mate_found(update.score, record.mate),
                    };
                    if correct && !solving {
                        search.found = Some(Found {
                            depth: update.depth,
                            time_ms: search.time_ms,
                            nodes: search.nodes,
                        });
                    } else if !correct {
                        search.found = None;
                    }
                    solving = correct;
                    if last_sent.is_none_or(|sent| sent.elapsed() >= PROGRESS_EVERY) {
                        last_sent = Some(Instant::now());
                        self.send(index, Cell::Running(search.clone()));
                    }
                }
                Ok(Some(EngineEvent::BestMove {
                    search_id: from,
                    best,
                })) if from == search_id => {
                    if search.time_ms == 0 {
                        search.time_ms = started.elapsed().as_millis() as u64;
                    }
                    let played = best.as_deref().and_then(|uci| record.uci_move(uci));
                    if let Some(mv) = &played {
                        search.best = Some(record.san(mv));
                    }
                    let solved = match goal {
                        TestGoal::BestMove => played
                            .as_ref()
                            .and_then(|mv| record.move_solves(mv))
                            .unwrap_or(false),
                        TestGoal::Mate => mate_found(search.score, record.mate),
                    };
                    if !solved {
                        search.found = None;
                    } else if search.found.is_none() {
                        search.found = Some(Found {
                            depth: search.depth,
                            time_ms: search.time_ms,
                            nodes: search.nodes,
                        });
                    }
                    return Searched::Done(if solved {
                        Cell::Solved(search)
                    } else {
                        Cell::Failed(search)
                    });
                }
                Ok(Some(EngineEvent::Crashed(reason))) => return Searched::Crashed(reason),
                Ok(Some(_)) => {}
                Err(_) => return Searched::Crashed("engine channel closed".to_owned()),
            }
        }
    }
}

/// A mate for the side to move in at most `mate` moves.
fn mate_found(score: Option<Score>, mate: Option<u32>) -> bool {
    match (score, mate) {
        (Some(Score::Mate(found)), Some(mate)) => found > 0 && found as u32 <= mate,
        _ => false,
    }
}

/// Solved, run and total time for one engine's column.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tally {
    pub solved: usize,
    pub done: usize,
    pub time_ms: u64,
}

pub fn tally<'a>(cells: impl IntoIterator<Item = &'a Cell>) -> Tally {
    let mut tally = Tally::default();
    for cell in cells {
        match cell {
            Cell::Solved(search) => {
                tally.solved += 1;
                tally.done += 1;
                tally.time_ms += search.time_ms;
            }
            Cell::Failed(search) => {
                tally.done += 1;
                tally.time_ms += search.time_ms;
            }
            Cell::Error(_) => tally.done += 1,
            Cell::Waiting | Cell::Running(_) => {}
        }
    }
    tally
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mates_count_when_no_longer_than_asked() {
        assert!(mate_found(Some(Score::Mate(2)), Some(3)));
        assert!(mate_found(Some(Score::Mate(3)), Some(3)));
        assert!(!mate_found(Some(Score::Mate(4)), Some(3)));
        assert!(!mate_found(Some(Score::Mate(-2)), Some(3)));
        assert!(!mate_found(Some(Score::Cp(900)), Some(3)));
        assert!(!mate_found(Some(Score::Mate(1)), None));
    }

    #[test]
    fn positions_need_the_operation_their_goal_checks() {
        let bm = chess_core::epd::parse_record("4k3/8/4K3/8/8/8/8/R7 w - - bm Ra8;").unwrap();
        let dm = chess_core::epd::parse_record("4k3/8/4K3/8/8/8/8/R7 w - - dm 1;").unwrap();
        assert_eq!(unusable(&bm, TestGoal::BestMove), None);
        assert!(unusable(&bm, TestGoal::Mate).is_some());
        assert_eq!(unusable(&dm, TestGoal::Mate), None);
        assert!(unusable(&dm, TestGoal::BestMove).is_some());
    }

    #[test]
    fn mate_searches_ask_for_the_mate() {
        let dm = chess_core::epd::parse_record("4k3/8/4K3/8/8/8/8/R7 w - - dm 1;").unwrap();
        let settings = RunSettings::from_saved(&TestPositionSettings {
            goal: TestGoal::Mate,
            limit: TestLimitKind::Depth,
            depth: 12,
            ..Default::default()
        });
        let limits = settings.limits_for(&dm);
        assert_eq!(limits.mate, Some(1));
        assert_eq!(limits.depth, Some(12));
    }

    #[test]
    fn tallies_count_finished_cells() {
        let search = Search {
            time_ms: 100,
            ..Default::default()
        };
        let cells = [
            Cell::Solved(search.clone()),
            Cell::Failed(search),
            Cell::Error("x".into()),
            Cell::Waiting,
        ];
        assert_eq!(
            tally(&cells),
            Tally {
                solved: 1,
                done: 3,
                time_ms: 200
            }
        );
    }
}

/// Runs a real engine: `FAST_TEST_ENGINE=/path/to/stockfish cargo test -- --ignored`.
#[cfg(test)]
mod engine_tests {
    use super::*;

    #[test]
    #[ignore = "needs a UCI engine in FAST_TEST_ENGINE"]
    fn solves_best_moves_and_mates_with_a_real_engine() {
        let Ok(path) = std::env::var("FAST_TEST_ENGINE") else {
            return;
        };
        let engine = EngineConfig {
            id: 1,
            name: "test".into(),
            path: path.into(),
            ..Default::default()
        };
        let suite = chess_core::parse_epd(
            "2rr3k/pp3pp1/1nnqbN1p/3pN3/2pP4/2P3Q1/PPB4P/R4RK1 w - - bm Qg6; id \"WAC.001\";\n\
             4k3/8/4K3/8/8/8/8/R7 w - - bm Ra8; dm 1;\n\
             4k3/8/4K3/8/8/8/8/R7 w - - am Ra8;\n",
        );
        let positions: Arc<Vec<EpdPosition>> =
            Arc::new(suite.into_iter().map(|line| line.parsed.unwrap()).collect());
        for (goal, parallel, expect) in [
            (TestGoal::BestMove, None, [true, true, false]),
            (TestGoal::BestMove, Some(2), [true, true, false]),
        ] {
            let (tx, rx) = smol::channel::unbounded();
            let settings = RunSettings {
                goal,
                limits: Limits {
                    depth: Some(22),
                    ..Default::default()
                },
                hash_mb: Some(16),
                clear_hash: true,
                parallel,
                syzygy: None,
            };
            start(
                vec![engine.clone(), engine.clone()],
                positions.clone(),
                settings,
                Arc::new(AtomicBool::new(false)),
                tx,
            );
            let mut cells = vec![vec![Cell::Waiting; positions.len()]; 2];
            while let Ok(event) = smol::block_on(rx.recv()) {
                match event {
                    RunEvent::Cell {
                        engine,
                        position,
                        cell,
                    } => cells[engine][position] = cell,
                    RunEvent::Finished => break,
                }
            }
            for column in &cells {
                let solved: Vec<bool> = column
                    .iter()
                    .map(|cell| matches!(cell, Cell::Solved(_)))
                    .collect();
                assert_eq!(solved, expect, "{goal:?} {parallel:?}: {column:?}");
            }
        }
        // Mates: only the second record has a dm.
        let mates: Arc<Vec<EpdPosition>> = Arc::new(positions[1..2].to_vec());
        let (tx, rx) = smol::channel::unbounded();
        start(
            vec![engine],
            mates.clone(),
            RunSettings {
                goal: TestGoal::Mate,
                limits: Limits {
                    movetime_ms: Some(2000),
                    ..Default::default()
                },
                hash_mb: None,
                clear_hash: false,
                parallel: None,
                syzygy: None,
            },
            Arc::new(AtomicBool::new(false)),
            tx,
        );
        let mut cells = vec![Cell::Waiting; mates.len()];
        while let Ok(event) = smol::block_on(rx.recv()) {
            match event {
                RunEvent::Cell { position, cell, .. } => cells[position] = cell,
                RunEvent::Finished => break,
            }
        }
        assert!(
            cells.iter().all(|cell| matches!(cell, Cell::Solved(_))),
            "{cells:?}"
        );
    }
}
