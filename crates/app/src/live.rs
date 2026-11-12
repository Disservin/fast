//! Continuous multi-engine analysis of one position, throttled for the UI.

use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap},
    time::Duration,
};

use chess_core::{GameTree, Move, NodeId, Position, position_hash, shakmaty::uci::UciMove};
use engine::{
    AnalysisUpdate, EngineConfig, EngineEvent, EngineHandle, EngineId, EngineLog, EngineStatus,
    Limits, LogKind, Score, SearchRequest, Wdl,
};
use gpui::{App, Context, Entity, EventEmitter, Task};

use crate::{
    config::AnalysisLimit,
    model::{AppModel, ConfigExt as _, ModelEvent, ToastExt as _, split_engine_name},
};

/// UI updates per engine are bounded to one per this interval (≤ 10 Hz).
const THROTTLE: Duration = Duration::from_millis(100);

/// Search ids are unique across every Analyse tab. Tabs hand the same engine processes
/// to each other, so a late event from one tab's search can never match another's.
static NEXT_SEARCH_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

fn next_search_id() -> u64 {
    NEXT_SEARCH_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

/// Positions remembered per engine before the cache starts over.
const CACHE_LIMIT: usize = 50_000;

/// Lines per engine and position hash. Keyed by position, not by node, so a
/// transposition reached by another move order finds the same analysis.
#[derive(Default)]
pub struct LineCache {
    entries: HashMap<(EngineId, u64), BTreeMap<u32, LiveLine>>,
}

impl LineCache {
    pub fn get(&self, engine: EngineId, hash: u64) -> Option<&BTreeMap<u32, LiveLine>> {
        self.entries.get(&(engine, hash))
    }

    /// Stores lines, keeping deeper ones already known for the position.
    pub fn store(&mut self, engine: EngineId, hash: u64, lines: &BTreeMap<u32, LiveLine>) {
        if lines.is_empty() {
            return;
        }
        if self.entries.len() >= CACHE_LIMIT {
            self.entries.clear();
        }
        let deepest =
            |lines: &BTreeMap<u32, LiveLine>| lines.get(&1).map_or(0, |line| line.update.depth);
        let key = (engine, hash);
        if self.entries.get(&key).is_some_and(|known| {
            known.get(&1).map(|line| line.position_context.as_str())
                == lines.get(&1).map(|line| line.position_context.as_str())
                && deepest(known) > deepest(lines)
        }) {
            return;
        }
        let stored: BTreeMap<_, _> = lines
            .iter()
            .filter(|(_, line)| !line.stale)
            .map(|(multipv, line)| {
                let mut line = line.clone();
                line.cached = true;
                (*multipv, line)
            })
            .collect();
        if stored.is_empty() {
            return;
        }
        self.entries.insert(key, stored);
    }

    pub fn forget_engine(&mut self, engine: EngineId) {
        self.entries.retain(|(id, _), _| *id != engine);
    }

    /// The best line's White-relative eval and depth for a position from any engine,
    /// preferring the order given.
    pub fn eval(
        &self,
        engines: &[EngineId],
        hash: u64,
        position_context: &str,
    ) -> Option<(i32, u32)> {
        engines.iter().find_map(|&id| {
            let line = self.get(id, hash)?.get(&1)?;
            if line.position_context != position_context {
                return None;
            }
            Some((score_cp(line.score?), line.update.depth))
        })
    }
}

/// The options an engine is started with for analysis: UCI asks the GUI to set
/// `UCI_AnalyseMode` when the engine is analysing rather than playing.
/// Whether a running process for `old` can keep serving `new`: same binary.
fn same_process(old: &EngineConfig, new: &EngineConfig) -> bool {
    old.id == new.id && old.path == new.path && old.version == new.version
}

/// Whether every UCI option has the same value, counting unset options as their
/// engine defaults, so saving a default does not count as a change.
fn same_options(old: &EngineConfig, new: &EngineConfig) -> bool {
    let (old, new) = (analysis_config(old), analysis_config(new));
    old.options
        .keys()
        .chain(new.options.keys())
        .all(|name| old.option_value(name) == new.option_value(name))
}

pub fn analysis_config(config: &EngineConfig) -> EngineConfig {
    let mut config = config.clone();
    if config
        .info
        .options
        .iter()
        .any(|option| option.name == "UCI_AnalyseMode")
    {
        config
            .options
            .insert("UCI_AnalyseMode".to_owned(), "true".to_owned());
    }
    config
}

/// A White-relative score as centipawns for graphs, mates at the edge.
pub fn score_cp(score: Score) -> i32 {
    match score {
        Score::Cp(cp) => cp,
        Score::Mate(mate) if mate > 0 => chess_core::annotation::MATE_CP,
        Score::Mate(_) => -chess_core::annotation::MATE_CP,
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct LiveLine {
    pub update: AnalysisUpdate,
    /// White-relative score.
    pub score: Option<Score>,
    /// White-relative win/draw/loss.
    pub wdl: Option<Wdl>,
    /// PV converted to SAN from the analysed position.
    pub sans: Vec<String>,
    /// Remembered from an earlier visit to this position; replaced once the running
    /// search reaches the same depth.
    pub cached: bool,
    /// Cached from a different engine signature or search limit.
    #[serde(default)]
    pub stale: bool,
    #[serde(default)]
    search_limit: String,
    #[serde(default)]
    engine_signature: String,
    #[serde(default)]
    position_context: String,
}

impl LiveLine {
    /// A fresh line from a search of `pos`, with its score and WDL made White-relative.
    pub(crate) fn new(
        pos: &chess_core::Chess,
        update: AnalysisUpdate,
        search_limit: String,
        engine_signature: String,
        position_context: String,
    ) -> Self {
        let white_to_move = pos.turn().is_white();
        Self {
            score: update
                .score
                .map(|score| if white_to_move { score } else { score.negate() }),
            wdl: update
                .wdl
                .map(|wdl| if white_to_move { wdl } else { wdl.flip() }),
            sans: chess_core::pv_to_san(pos, &update.pv),
            update,
            cached: false,
            stale: false,
            search_limit,
            engine_signature,
            position_context,
        }
    }
}

pub struct LiveEngine {
    pub config: EngineConfig,
    pub handle: Option<EngineHandle>,
    pub lines: BTreeMap<u32, LiveLine>,
    pub running: bool,
    /// Kept after a crash so the failure can be read.
    pub log: EngineLog,
    _reader: Option<Task<()>>,
}

impl LiveEngine {
    /// Latest numbers for the whole search, from the best line.
    pub fn stats(&self) -> Option<&AnalysisUpdate> {
        self.lines.get(&1).map(|line| &line.update)
    }
}

pub enum LiveEvent {
    Updated,
    /// An engine reached an evaluation for the analysed node (White-relative) at a
    /// depth.
    Eval(NodeId, Score, u32),
}

pub struct LiveAnalysis {
    model: Entity<AppModel>,
    pub engines: Vec<LiveEngine>,
    pub on: bool,
    active: bool,
    /// Searching is held: engines stay loaded and keep their last lines, but no
    /// search runs, even when the position changes, until resumed.
    pub paused: bool,
    search_id: u64,
    node: Option<NodeId>,
    pos: Option<chess_core::Chess>,
    hash: u64,
    /// The analysed position ends the game (mate, stalemate, a draw rule): nothing to
    /// search.
    pub end: Option<chess_core::GameEnd>,
    pub cache: LineCache,
    /// Replaces the configured search limit, e.g. a fixed node count per position
    /// while a game is analysed backwards.
    pub limit_override: Option<AnalysisLimit>,
    /// Bumped when Analyse loads a different game, so engines get `ucinewgame`.
    pub game: u64,
    fen: String,
    moves: Vec<String>,
    /// Root-move restrictions for this position only; never used as a full eval.
    pub search_moves: Vec<String>,
    _model_sub: gpui::Subscription,
}

impl EventEmitter<LiveEvent> for LiveAnalysis {}

impl LiveAnalysis {
    pub fn new(model: Entity<AppModel>, cx: &mut Context<Self>) -> Self {
        let on = model.read(cx).config.engine_on;
        let sub = cx.subscribe(&model, |this, _, event, cx| {
            if *event == ModelEvent::Engines {
                this.restart(cx);
            }
        });
        let mut this = Self {
            model,
            engines: Vec::new(),
            on,
            active: false,
            paused: false,
            search_id: 0,
            node: None,
            pos: None,
            hash: 0,
            end: None,
            cache: LineCache::default(),
            limit_override: None,
            game: 0,
            fen: String::new(),
            moves: Vec::new(),
            search_moves: Vec::new(),
            _model_sub: sub,
        };
        this.restart(cx);
        this
    }

    /// Only the visible analysis tab owns the shared engine registry and event readers.
    pub fn set_active(&mut self, active: bool, cx: &mut Context<Self>) {
        if self.active == active {
            return;
        }
        self.active = active;
        if active {
            // The processes keep running between tabs; this tab takes over their event
            // queues, and search ids keep the previous tab's late events out. Engines
            // taken out of analysis meanwhile are quit.
            let analysed: Vec<EngineId> = self
                .model
                .read(cx)
                .analysis_engines()
                .iter()
                .map(|config| config.id)
                .collect();
            self.model.update(cx, |model, _| {
                model.manager.retain(|id| analysed.contains(&id));
            });
            self.restart(cx);
        } else {
            for engine in &mut self.engines {
                // Keep idle registry handles available to Settings and log windows.
                if let Some(handle) = engine.handle.take() {
                    handle.stop();
                }
                engine._reader = None;
                engine.running = false;
                let id = engine.config.id;
                self.model.update(cx, |model, cx| {
                    model.set_engine_status(id, EngineStatus::Stopped, cx);
                });
            }
        }
        cx.notify();
    }

    /// Reconciles running engines with the configuration. A process is only
    /// relaunched when its binary changed; option changes are applied in place by
    /// whoever changed them, and the search only restarts when option values differ.
    pub fn restart(&mut self, cx: &mut Context<Self>) {
        let configs = self.model.read(cx).analysis_engines();
        let unchanged = self.engines.len() == configs.len()
            && self.engines.iter().zip(&configs).all(|(live, config)| {
                same_process(&live.config, config)
                    && same_options(&live.config, config)
                    && (!self.active
                        || !self.on
                        || live.handle.is_some()
                            && self.model.read(cx).manager.get(config.id).is_some())
            });
        if unchanged {
            // Names, colours or other uses changed: nothing the engines need to hear.
            for (live, config) in self.engines.iter_mut().zip(configs) {
                live.config = config;
            }
            cx.emit(LiveEvent::Updated);
            cx.notify();
            return;
        }
        for engine in &self.engines {
            if let Some(handle) = &engine.handle {
                handle.stop();
            }
        }
        let demo = self.model.read(cx).demo;
        let previous: Vec<_> = std::mem::take(&mut self.engines);
        for config in configs {
            let old = previous.iter().find(|live| live.config.id == config.id);
            let relaunch = old.is_none_or(|old| !same_process(&old.config, &config));
            if old.is_none_or(|old| relaunch || !same_options(&old.config, &config)) {
                self.cache.forget_engine(config.id);
            }
            let mut live = LiveEngine {
                config: config.clone(),
                handle: None,
                lines: if !self.search_moves.is_empty() {
                    BTreeMap::new()
                } else {
                    self.cache
                        .get(config.id, self.hash)
                        .cloned()
                        .unwrap_or_default()
                },
                running: false,
                log: EngineLog::default(),
                _reader: None,
            };
            if !demo && self.on && self.active {
                let handle = self.model.update(cx, |model, _| {
                    if relaunch {
                        model.manager.shutdown(config.id);
                    }
                    // A process another tab already started is ready at once.
                    let status = if model.manager.get(config.id).is_none() {
                        EngineStatus::Loading
                    } else if self.paused {
                        EngineStatus::Paused
                    } else {
                        EngineStatus::Ready
                    };
                    model.engine_status.insert(config.id, status);
                    model.manager.ensure_running(&analysis_config(&config))
                });
                live._reader = Some(self.spawn_reader(config.id, &handle, cx));
                live.log = handle.log();
                live.handle = Some(handle);
            }
            self.engines.push(live);
        }
        // Engines taken out of analysis stop running, unless they crashed and already did.
        for removed in previous.iter().filter(|old| {
            !self
                .engines
                .iter()
                .any(|live| live.config.id == old.config.id)
        }) {
            let id = removed.config.id;
            if self.active {
                self.model.update(cx, |model, _| model.manager.shutdown(id));
            }
        }
        self.search(cx);
        cx.emit(LiveEvent::Updated);
        cx.notify();
    }

    /// Quits one analysis engine and starts it again, e.g. when it hangs or its hash
    /// should be cleared. Lines already found stay on screen.
    pub fn relaunch(&mut self, id: EngineId, cx: &mut Context<Self>) {
        if self.active {
            self.model.update(cx, |model, _| model.manager.shutdown(id));
        }
        self.restart(cx);
    }

    fn spawn_reader(
        &self,
        id: EngineId,
        handle: &EngineHandle,
        cx: &mut Context<Self>,
    ) -> Task<()> {
        crate::event_reader::spawn_batched(
            handle.events(),
            Some(THROTTLE),
            cx,
            move |this, batch, cx| this.apply_events(id, batch, cx),
        )
    }

    fn apply_events(&mut self, id: EngineId, batch: Vec<EngineEvent>, cx: &mut Context<Self>) {
        if !self.active {
            return;
        }
        let mut crash = None;
        let mut newest_eval = None;
        let mut analysed = false;
        let mut ready = false;
        let search_id = self.search_id;
        let node = self.node;
        let pos = self.pos.clone();
        let hash = self.hash;
        let Some(live) = self.engines.iter_mut().find(|live| live.config.id == id) else {
            return;
        };
        let current_limit = crate::analysis_store::current_limit(
            self.limit_override
                .unwrap_or(self.model.read(cx).config.analysis_limit),
        );
        let current_signature = crate::analysis_store::engine_signature(&live.config);
        for event in batch {
            match event {
                EngineEvent::Ready(info) => {
                    ready = true;
                    let name = info.name.clone();
                    let model = self.model.clone();
                    model.update(cx, |model, cx| {
                        model.engine_status.insert(id, EngineStatus::Ready);
                        if let Some(config) = model.engine_mut(id) {
                            let (parsed, version) = split_engine_name(&name, &config.name);
                            if config.version.is_empty() {
                                config.version = version;
                                config.name = parsed;
                            }
                            config.info = info;
                            model.save_config(cx);
                        }
                    });
                    // Keep the running engine's copy in step, or the next reconcile would
                    // take the new version for another binary and restart the process.
                    if let Some(config) = self.model.read(cx).engine(id) {
                        live.config.name = config.name.clone();
                        live.config.version = config.version.clone();
                        live.config.info = config.info.clone();
                    }
                }
                EngineEvent::Analysis {
                    search_id: from,
                    update,
                } if from == search_id => {
                    let Some(pos) = &pos else { continue };
                    // Remembered lines stay until the new search is as deep.
                    if live.lines.get(&update.multipv).is_some_and(|line| {
                        line.cached && !line.stale && line.update.depth > update.depth
                    }) {
                        continue;
                    }
                    // A fail-high/low line's move is only the one being tried, and a
                    // progress line has no move at all: keep the last real line and
                    // its score, and take just the search progress.
                    let progress_only = update.pv.is_empty();
                    if progress_only || update.bound {
                        if let Some(line) = live.lines.get_mut(&update.multipv) {
                            analysed = true;
                            let kept = &mut line.update;
                            kept.depth = kept.depth.max(update.depth);
                            kept.seldepth = update.seldepth.or(kept.seldepth);
                            kept.nodes = update.nodes.or(kept.nodes);
                            kept.nps = update.nps.or(kept.nps);
                            kept.tbhits = update.tbhits.or(kept.tbhits);
                            kept.hashfull = update.hashfull.or(kept.hashfull);
                            kept.time_ms = update.time_ms.or(kept.time_ms);
                            continue;
                        }
                        if progress_only {
                            continue;
                        }
                    }
                    analysed = true;
                    let line = LiveLine::new(
                        pos,
                        update,
                        current_limit.clone(),
                        current_signature.clone(),
                        crate::analysis_store::position_key(&self.fen, &self.moves),
                    );
                    if line.update.multipv == 1
                        && !line.update.bound
                        && let Some(score) = line.score
                    {
                        newest_eval = Some((score, line.update.depth));
                    }
                    live.lines.insert(line.update.multipv, line);
                }
                EngineEvent::Analysis { .. } => {}
                EngineEvent::BestMove {
                    search_id: from, ..
                } => {
                    if from == search_id {
                        live.running = false;
                        self.model.update(cx, |model, cx| {
                            model.set_engine_status(id, EngineStatus::Ready, cx);
                        });
                    }
                }
                EngineEvent::Crashed(reason) => crash = Some(reason),
            }
        }
        if analysed && self.search_moves.is_empty() {
            self.cache.store(id, hash, &live.lines);
            let position = crate::analysis_store::position_key(&self.fen, &self.moves);
            let limit = crate::analysis_store::current_limit(
                self.limit_override
                    .unwrap_or(self.model.read(cx).config.analysis_limit),
            );
            let key = crate::analysis_store::identity(&position, &live.config, &limit);
            let record = crate::analysis_store::AnalysisRecord {
                position,
                engine: crate::analysis_store::engine_signature(&live.config),
                limit,
                lines: live.lines.clone(),
                saved_at: crate::analysis_store::timestamp(),
            };
            if let Some(store) = self.model.read(cx).analyses.as_ref()
                && let Err(err) = store.put(&key, id, &record)
            {
                log::warn!("could not save engine analysis: {err:#}");
            }
        }
        if let Some(reason) = crash {
            live.handle = None;
            live.running = false;
            live.lines.clear();
            let name = live.config.name.clone();
            log::error!("engine {name} crashed: {reason}");
            self.model.update(cx, |model, cx| {
                model.manager.shutdown(id);
                model.engine_status.insert(id, EngineStatus::Error);
                model.toast(format!("{name} stopped: {reason}"), cx);
            });
        }
        if self.search_moves.is_empty()
            && let (Some(node), Some((score, depth))) = (node, newest_eval)
        {
            cx.emit(LiveEvent::Eval(node, score, depth));
        }
        if ready {
            self.search(cx);
        }
        cx.emit(LiveEvent::Updated);
        cx.notify();
    }

    pub fn toggle(&mut self, cx: &mut Context<Self>) {
        self.on = !self.on;
        self.paused = false;
        self.model.update(cx, |model, cx| {
            model.config.engine_on = self.on;
            model.save_config(cx);
            if !self.on {
                model.manager.shutdown_all();
            }
        });
        if self.on {
            self.restart(cx);
        } else {
            for engine in &mut self.engines {
                engine.handle = None;
                engine._reader = None;
                engine.running = false;
                engine.lines.clear();
                let id = engine.config.id;
                self.model.update(cx, |model, cx| {
                    model.set_engine_status(id, EngineStatus::Stopped, cx);
                });
            }
        }
        cx.emit(LiveEvent::Updated);
        cx.notify();
    }

    /// Applies an option to a running engine without restarting it, and saves it.
    pub fn set_option(&mut self, id: EngineId, name: &str, value: String, cx: &mut Context<Self>) {
        let current = self
            .engines
            .iter()
            .find(|live| live.config.id == id)
            .and_then(|live| live.config.option_value(name));
        if current.as_ref() == Some(&value) {
            return;
        }
        self.model.update(cx, |model, cx| {
            if let Some(config) = model.engine_mut(id) {
                config.options.insert(name.to_owned(), value.clone());
            }
            model.save_config(cx);
        });
        // Lines found under other settings (another network, fewer lines) no longer apply.
        self.cache.forget_engine(id);
        if let Some(live) = self.engines.iter_mut().find(|live| live.config.id == id) {
            live.config.options.insert(name.to_owned(), value.clone());
            live.lines.clear();
            if let Some(handle) = &live.handle {
                handle.set_option(name, Some(value));
                live.running = !self.paused;
            }
        }
        if self.model.read(cx).demo {
            self.search(cx);
        }
        cx.emit(LiveEvent::Updated);
        cx.notify();
    }

    /// Changes when analysis stops, and restarts the search under the new limit.
    pub fn set_limit(&mut self, limit: AnalysisLimit, cx: &mut Context<Self>) {
        self.model.edit_config(cx, |config| {
            config.analysis_limit = limit;
            if limit != AnalysisLimit::Infinite {
                config.finite_limit = limit;
            }
        });
        self.search(cx);
        cx.emit(LiveEvent::Updated);
        cx.notify();
    }

    /// Switches between searching without end and the limit chosen in Search options.
    pub fn toggle_infinite(&mut self, cx: &mut Context<Self>) {
        let config = &self.model.read(cx).config;
        let limit = match config.analysis_limit {
            AnalysisLimit::Infinite => match config.finite_limit {
                AnalysisLimit::Infinite => AnalysisLimit::default(),
                finite => finite,
            },
            _ => AnalysisLimit::Infinite,
        };
        self.set_limit(limit, cx);
    }

    /// Explicitly replaces a restored record with a fresh search under current settings.
    pub fn replace_cached(&mut self, id: EngineId, cx: &mut Context<Self>) {
        let position = crate::analysis_store::position_key(&self.fen, &self.moves);
        let limit = crate::analysis_store::current_limit(
            self.limit_override
                .unwrap_or(self.model.read(cx).config.analysis_limit),
        );
        let config = self
            .engines
            .iter()
            .find(|engine| engine.config.id == id)
            .map(|e| e.config.clone());
        if let Some(config) = config {
            let key = crate::analysis_store::identity(&position, &config, &limit);
            if let Some(store) = self.model.read(cx).analyses.as_ref()
                && let Err(err) = store.delete(&key)
            {
                self.model
                    .toast(format!("Could not replace cached analysis: {err:#}"), cx);
                return;
            }
        }
        self.cache.forget_engine(id);
        if let Some(engine) = self
            .engines
            .iter_mut()
            .find(|engine| engine.config.id == id)
        {
            engine.lines.clear();
        }
        self.search(cx);
        cx.emit(LiveEvent::Updated);
        cx.notify();
    }

    /// Holds or resumes searching. Resuming searches the current position afresh.
    pub fn set_paused(&mut self, paused: bool, cx: &mut Context<Self>) {
        if self.paused == paused {
            return;
        }
        self.paused = paused;
        if paused {
            for engine in &mut self.engines {
                if let Some(handle) = &engine.handle {
                    handle.stop();
                }
                engine.running = false;
                let id = engine.config.id;
                self.model.update(cx, |model, cx| {
                    model.set_engine_status(id, EngineStatus::Paused, cx);
                });
            }
        } else {
            self.search(cx);
        }
        cx.emit(LiveEvent::Updated);
        cx.notify();
    }

    /// Points analysis at a node. Cancels the running search immediately.
    pub fn set_position(&mut self, tree: &GameTree, node: NodeId, cx: &mut Context<Self>) {
        self.set_line(tree, node, &[], cx);
    }

    /// Analyses the position `extra` moves past a node, without adding them to the game.
    /// Evals found there are not recorded on the node.
    pub fn set_line(
        &mut self,
        tree: &GameTree,
        node: NodeId,
        extra: &[Move],
        cx: &mut Context<Self>,
    ) {
        let root_fen = tree.fen(GameTree::ROOT);
        self.search_moves.clear();
        let moves: Vec<String> = tree
            .path_to(node)
            .iter()
            .filter_map(|&id| tree.node(id).mv)
            .chain(extra.iter().copied())
            .map(|mv| tree.uci(mv))
            .collect();
        let mut pos = tree.node(node).pos.clone();
        let mut history = tree.history(node);
        for mv in extra {
            pos.play_unchecked(*mv);
            history.push(pos.clone());
        }
        self.end = chess_core::game_end(&pos, &history);
        self.node = extra.is_empty().then_some(node);
        self.hash = position_hash(&pos);
        self.pos = Some(pos);
        self.fen = root_fen;
        self.moves = moves;
        let hash = self.hash;
        let position_key = crate::analysis_store::position_key(&self.fen, &self.moves);
        let search_limit = crate::analysis_store::current_limit(
            self.limit_override
                .unwrap_or(self.model.read(cx).config.analysis_limit),
        );
        let mut known = self.end.map(|end| {
            let cp = match end {
                chess_core::GameEnd::Checkmate { winner } if winner.is_white() => {
                    chess_core::annotation::MATE_CP
                }
                chess_core::GameEnd::Checkmate { .. } => -chess_core::annotation::MATE_CP,
                _ => 0,
            };
            (Score::Cp(cp), 0)
        });
        for engine in &mut self.engines {
            // A drawn repetition may have lines from its earlier visits; they no longer
            // apply once the game is over.
            engine.lines = if self.end.is_some() {
                BTreeMap::new()
            } else {
                self.cache
                    .get(engine.config.id, hash)
                    .cloned()
                    .map(|mut lines| {
                        let signature = crate::analysis_store::engine_signature(&engine.config);
                        for line in lines.values_mut() {
                            line.cached = true;
                            line.stale = line.search_limit != search_limit
                                || line.engine_signature != signature
                                || line.position_context != position_key;
                        }
                        lines
                    })
                    .or_else(|| {
                        let key = crate::analysis_store::identity(
                            &position_key,
                            &engine.config,
                            &search_limit,
                        );
                        self.model.read(cx).analyses.as_ref().and_then(|store| {
                            let loaded = store.get(&key).and_then(|record| match record {
                                Some(record) => Ok(Some(record)),
                                None => store.latest_for_position(engine.config.id, &position_key),
                            });
                            match loaded {
                                Ok(Some(mut record)) => {
                                    let current_engine =
                                        crate::analysis_store::engine_signature(&engine.config);
                                    let stale = record.limit != search_limit
                                        || record.engine != current_engine;
                                    for line in record.lines.values_mut() {
                                        line.cached = true;
                                        line.stale = stale;
                                        line.search_limit = record.limit.clone();
                                        line.engine_signature = record.engine.clone();
                                        line.position_context = record.position.clone();
                                    }
                                    Some(record.lines)
                                }
                                Ok(None) => None,
                                Err(err) => {
                                    log::warn!("could not load saved analysis: {err:#}");
                                    None
                                }
                            }
                        })
                    })
                    .unwrap_or_default()
            };
            if known.is_none()
                && let Some(line) = engine.lines.get(&1)
                && let Some(score) = line.score
            {
                known = Some((score, line.update.depth));
            }
        }
        if let (Some(node), Some((score, depth))) = (self.node, known) {
            cx.emit(LiveEvent::Eval(node, score, depth));
        }
        self.search(cx);
        cx.emit(LiveEvent::Updated);
        cx.notify();
    }

    /// Starts `go infinite` on every engine for the current position.
    pub fn toggle_search_move(&mut self, mv: Move, cx: &mut Context<Self>) {
        if !self.pos.as_ref().is_some_and(|pos| pos.is_legal(mv)) {
            return;
        }
        let uci = UciMove::from_move(
            mv,
            self.pos
                .as_ref()
                .map_or(chess_core::shakmaty::CastlingMode::Standard, |pos| {
                    pos.castles().mode()
                }),
        )
        .to_string();
        if let Some(index) = self.search_moves.iter().position(|known| known == &uci) {
            self.search_moves.remove(index);
        } else {
            self.search_moves.push(uci);
        }
        self.refresh_restricted_search(cx);
    }

    pub fn clear_search_moves(&mut self, cx: &mut Context<Self>) {
        self.search_moves.clear();
        self.refresh_restricted_search(cx);
    }

    fn refresh_restricted_search(&mut self, cx: &mut Context<Self>) {
        self.search_id = next_search_id();
        // Restricted scores must not overwrite unrestricted cache entries or be
        // reused to skip the new search at a fixed depth.
        for engine in &mut self.engines {
            engine.lines.clear();
        }
        self.search(cx);
        cx.emit(LiveEvent::Updated);
        cx.notify();
    }

    pub fn search(&mut self, cx: &mut App) {
        if !self.on || self.paused || !self.active {
            return;
        }
        let Some(pos) = &self.pos else { return };
        let terminal = self.end.is_some();
        self.search_id = next_search_id();
        let hash = position_hash(pos);
        let limit = self
            .limit_override
            .unwrap_or(self.model.read(cx).config.analysis_limit);
        let depth_limit = match limit {
            AnalysisLimit::Depth(depth) => Some(depth),
            _ => None,
        };
        let demo = self.model.read(cx).demo;
        let limits = match limit {
            AnalysisLimit::Infinite => None,
            AnalysisLimit::Depth(depth) => Some(Limits {
                depth: Some(depth),
                ..Default::default()
            }),
            AnalysisLimit::Seconds(seconds) => Some(Limits {
                movetime_ms: Some(u64::from(seconds) * 1000),
                ..Default::default()
            }),
            AnalysisLimit::Nodes(nodes) => Some(Limits {
                nodes: Some(nodes),
                ..Default::default()
            }),
        };
        if demo {
            let mut scripted = crate::demo::scripted_lines(hash);
            let search_limit = crate::analysis_store::current_limit(limit);
            for engine in &mut self.engines {
                let signature = crate::analysis_store::engine_signature(&engine.config);
                engine.lines.clear();
                if let Some(lines) = scripted.remove(engine.config.name.as_str()) {
                    let requested = engine
                        .config
                        .options
                        .get("MultiPV")
                        .and_then(|value| value.parse::<usize>().ok())
                        .unwrap_or(3);
                    // The design's table shows a fourth scripted line at its "3".
                    let shown = if requested >= 3 {
                        lines.len()
                    } else {
                        requested
                    };
                    engine
                        .log
                        .push(LogKind::Sent, format!("position fen {}", self.fen));
                    engine.log.push(LogKind::Sent, "go infinite");
                    for update in lines.into_iter().take(shown) {
                        if !self.search_moves.is_empty()
                            && !update
                                .pv
                                .first()
                                .is_some_and(|mv| self.search_moves.contains(mv))
                        {
                            continue;
                        }
                        engine
                            .log
                            .push(LogKind::Received, crate::demo::info_line(&update));
                        let line = LiveLine::new(
                            pos,
                            update,
                            search_limit.clone(),
                            signature.clone(),
                            crate::analysis_store::position_key(&self.fen, &self.moves),
                        );
                        engine.lines.insert(line.update.multipv, line);
                    }
                }
            }
            return;
        }
        for engine in &mut self.engines {
            let Some(handle) = &engine.handle else {
                continue;
            };
            if terminal {
                handle.stop();
                // No search is started for this position, so there will be no
                // matching bestmove event to clear the previous running state.
                engine.running = false;
                self.model.update(cx, |model, cx| {
                    model.set_engine_status(engine.config.id, EngineStatus::Ready, cx);
                });
                continue;
            }
            if self.pos.as_ref().is_some_and(|pos| {
                pos.castles().mode() == chess_core::shakmaty::CastlingMode::Chess960
            }) && !self
                .model
                .read(cx)
                .engine(engine.config.id)
                .is_some_and(|config| {
                    config
                        .info
                        .options
                        .iter()
                        .any(|option| option.name == "UCI_Chess960")
                })
            {
                handle.stop();
                engine.running = false;
                engine.lines.clear();
                continue;
            }
            // A remembered line already reaches the depth limit: nothing to search.
            if let Some(limit) = depth_limit
                && engine
                    .lines
                    .get(&1)
                    .is_some_and(|line| line.update.depth >= limit)
            {
                handle.stop();
                engine.running = false;
                self.model.update(cx, |model, cx| {
                    model.set_engine_status(engine.config.id, EngineStatus::Ready, cx);
                });
                continue;
            }
            engine.running = true;
            self.model.update(cx, |model, cx| {
                model.set_engine_status(engine.config.id, EngineStatus::Searching, cx);
            });
            handle.search(SearchRequest {
                chess960: self.pos.as_ref().is_some_and(|pos| {
                    pos.castles().mode() == chess_core::shakmaty::CastlingMode::Chess960
                }),
                search_moves: self.search_moves.clone(),
                fen: self.fen.clone(),
                moves: self.moves.clone(),
                limits,
                search_id: self.search_id,
                game: self.game,
            });
        }
    }

    /// An engine is searching for this tab right now.
    pub fn searching(&self) -> bool {
        self.active
            && self.on
            && !self.paused
            && self
                .engines
                .iter()
                .any(|engine| engine.handle.is_some() && engine.running)
    }

    /// Engines with a live process.
    pub fn loaded(&self) -> usize {
        self.engines
            .iter()
            .filter(|engine| engine.handle.is_some())
            .count()
    }

    /// Every loaded engine has finished its search, and analysis is not paused.
    pub fn idle(&self) -> bool {
        !self.paused
            && self
                .engines
                .iter()
                .filter(|engine| engine.handle.is_some())
                .all(|engine| !engine.running)
    }

    /// Engine ids in table order, for cache lookups.
    pub fn engine_ids(&self) -> Vec<EngineId> {
        self.engines.iter().map(|engine| engine.config.id).collect()
    }

    /// Principal engine for the eval column and line preview.
    pub fn primary(&self) -> Option<&LiveEngine> {
        self.engines
            .iter()
            .find(|engine| !engine.lines.is_empty())
            .or_else(|| self.engines.first())
    }

    /// Prefer an engine with actual WDL data; retain an enabled source while waiting.
    pub fn wdl_engine(&self) -> Option<&LiveEngine> {
        let has_data = |engine: &&LiveEngine| engine.lines.values().any(|line| line.wdl.is_some());
        let visible = |engine: &&LiveEngine| wdl_visible(&engine.config, has_data(engine));
        self.engines
            .iter()
            .filter(visible)
            .find(has_data)
            .or_else(|| self.engines.iter().find(visible))
    }
}

/// Explicitly disabling the UCI option also hides cached WDL. Engines which
/// report WDL natively need no UCI_ShowWDL option.
pub fn wdl_visible(config: &EngineConfig, has_data: bool) -> bool {
    match config.options.get("UCI_ShowWDL").map(String::as_str) {
        Some("false") => false,
        Some("true") => true,
        _ => has_data,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(depth: u32, cp: i32) -> BTreeMap<u32, LiveLine> {
        let line = LiveLine {
            update: AnalysisUpdate {
                depth,
                multipv: 1,
                ..Default::default()
            },
            score: Some(Score::Cp(cp)),
            position_context: "ctx".to_owned(),
            ..Default::default()
        };
        [(1, line)].into_iter().collect()
    }

    #[test]
    fn wdl_visibility_respects_option_and_native_output() {
        let mut config = EngineConfig::default();
        assert!(!wdl_visible(&config, false));
        assert!(wdl_visible(&config, true));
        config.options.insert("UCI_ShowWDL".into(), "true".into());
        assert!(wdl_visible(&config, false));
        config.options.insert("UCI_ShowWDL".into(), "false".into());
        assert!(!wdl_visible(&config, true));
    }

    #[test]
    fn cache_keeps_the_deepest_lines_and_marks_them() {
        let mut cache = LineCache::default();
        cache.store(1, 42, &lines(20, 30));
        cache.store(1, 42, &lines(12, -10));
        let kept = cache.get(1, 42).unwrap();
        assert_eq!(kept[&1].update.depth, 20);
        assert!(kept[&1].cached);
        cache.store(1, 42, &lines(25, 15));
        assert_eq!(cache.eval(&[1], 42, "ctx"), Some((15, 25)));
        // Another engine's analysis of the same position is separate.
        assert_eq!(cache.eval(&[2], 42, "ctx"), None);
        cache.store(2, 42, &lines(8, 50));
        assert_eq!(cache.eval(&[2, 1], 42, "ctx"), Some((50, 8)));
        cache.forget_engine(1);
        assert!(cache.get(1, 42).is_none());
        assert!(cache.get(2, 42).is_some());
    }

    #[test]
    fn analysis_mode_is_set_only_where_offered() {
        let mut config = EngineConfig::default();
        assert!(
            !analysis_config(&config)
                .options
                .contains_key("UCI_AnalyseMode")
        );
        config.info.options.push(engine::UciOption {
            name: "UCI_AnalyseMode".to_owned(),
            kind: engine::OptionKind::Check { default: false },
        });
        assert_eq!(
            analysis_config(&config)
                .options
                .get("UCI_AnalyseMode")
                .map(String::as_str),
            Some("true")
        );
        // The saved configuration is left alone.
        assert!(config.options.is_empty());
    }

    #[test]
    fn mates_map_to_the_graph_edge() {
        assert_eq!(score_cp(Score::Mate(3)), 2000);
        assert_eq!(score_cp(Score::Mate(-1)), -2000);
        assert_eq!(score_cp(Score::Cp(-37)), -37);
    }
}
