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
use gpui::{App, AsyncApp, Context, Entity, EventEmitter, Task, WeakEntity};

use crate::{
    config::AnalysisLimit,
    model::{AppModel, ModelEvent, split_engine_name},
};

/// UI updates per engine are bounded to one per this interval (≤ 10 Hz).
const THROTTLE: Duration = Duration::from_millis(100);

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
        if self
            .entries
            .get(&key)
            .is_some_and(|known| deepest(known) > deepest(lines))
        {
            return;
        }
        let stored = lines
            .iter()
            .map(|(multipv, line)| {
                let mut line = line.clone();
                line.cached = true;
                (*multipv, line)
            })
            .collect();
        self.entries.insert(key, stored);
    }

    pub fn forget_engine(&mut self, engine: EngineId) {
        self.entries.retain(|(id, _), _| *id != engine);
    }

    /// The best line's White-relative eval and depth for a position from any engine,
    /// preferring the order given.
    pub fn eval(&self, engines: &[EngineId], hash: u64) -> Option<(i32, u32)> {
        engines.iter().find_map(|&id| {
            let line = self.get(id, hash)?.get(&1)?;
            Some((score_cp(line.score?), line.update.depth))
        })
    }
}

/// The options an engine is started with for analysis: UCI asks the GUI to set
/// `UCI_AnalyseMode` when the engine is analysing rather than playing.
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
    /// An engine reached an evaluation for the analysed node (White-relative cp)
    /// at a depth.
    Eval(NodeId, i32, u32),
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
    /// Replaces the configured search limit, e.g. a fixed time per move while a game
    /// is analysed backwards.
    pub limit_override: Option<AnalysisLimit>,
    /// Bumped when Analyse loads a different game, so engines get `ucinewgame`.
    pub game: u64,
    fen: String,
    moves: Vec<String>,
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
            // Never attach a new tab's reader to another tab's event queue.
            self.model
                .update(cx, |model, _| model.manager.shutdown_all());
            self.restart(cx);
        } else {
            for engine in &mut self.engines {
                // Keep idle registry handles available to Settings and log windows.
                if let Some(handle) = engine.handle.take() {
                    handle.stop();
                }
                engine._reader = None;
                engine.running = false;
            }
        }
        cx.notify();
    }

    /// Reconciles running engines with the configuration, restarting everything.
    pub fn restart(&mut self, cx: &mut Context<Self>) {
        for engine in &self.engines {
            if let Some(handle) = &engine.handle {
                handle.stop();
            }
        }
        let configs = self.model.read(cx).analysis_engines();
        let demo = self.model.read(cx).demo;
        let previous: Vec<_> = std::mem::take(&mut self.engines);
        for config in configs {
            let existing = previous.iter().find(|live| live.config == config);
            if existing.is_none() {
                self.cache.forget_engine(config.id);
            }
            let mut live = LiveEngine {
                config: config.clone(),
                handle: None,
                lines: self
                    .cache
                    .get(config.id, self.hash)
                    .cloned()
                    .unwrap_or_default(),
                running: false,
                log: EngineLog::default(),
                _reader: None,
            };
            if !demo && self.on && self.active {
                let handle = self.model.update(cx, |model, _| {
                    if existing.is_none() {
                        model.manager.shutdown(config.id);
                    }
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

    fn spawn_reader(
        &self,
        id: EngineId,
        handle: &EngineHandle,
        cx: &mut Context<Self>,
    ) -> Task<()> {
        let events = handle.events();
        cx.spawn(async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
            while let Ok(first) = events.recv().await {
                let mut batch = vec![first];
                // Coalesce whatever else arrived, then wait out the throttle window.
                while let Ok(event) = events.try_recv() {
                    batch.push(event);
                }
                let alive = this
                    .update(cx, |this, cx| this.apply_events(id, batch, cx))
                    .is_ok();
                if !alive {
                    return;
                }
                cx.background_executor().timer(THROTTLE).await;
            }
        })
    }

    fn apply_events(&mut self, id: EngineId, batch: Vec<EngineEvent>, cx: &mut Context<Self>) {
        if !self.active {
            return;
        }
        let mut crash = None;
        let mut newest_eval = None;
        let mut analysed = false;
        let search_id = self.search_id;
        let node = self.node;
        let pos = self.pos.clone();
        let hash = self.hash;
        let Some(live) = self.engines.iter_mut().find(|live| live.config.id == id) else {
            return;
        };
        for event in batch {
            match event {
                EngineEvent::Ready(info) => {
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
                }
                EngineEvent::Analysis {
                    search_id: from,
                    update,
                } if from == search_id => {
                    let Some(pos) = &pos else { continue };
                    // Remembered lines stay until the new search is as deep.
                    if live
                        .lines
                        .get(&update.multipv)
                        .is_some_and(|line| line.cached && line.update.depth > update.depth)
                    {
                        continue;
                    }
                    analysed = true;
                    // A fail-high/low line's move is only the one being tried: keep the
                    // last real line and its score, and take just the search progress.
                    if update.bound
                        && let Some(line) = live.lines.get_mut(&update.multipv)
                    {
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
                    let white_to_move = pos.turn().is_white();
                    let score = update
                        .score
                        .map(|score| if white_to_move { score } else { score.negate() });
                    let wdl = update
                        .wdl
                        .map(|wdl| if white_to_move { wdl } else { wdl.flip() });
                    if update.multipv == 1
                        && !update.bound
                        && let Some(score) = score
                    {
                        newest_eval = Some((score_cp(score), update.depth));
                    }
                    let sans = chess_core::pv_to_san(pos, &update.pv);
                    live.lines.insert(
                        update.multipv,
                        LiveLine {
                            update,
                            score,
                            wdl,
                            sans,
                            cached: false,
                        },
                    );
                }
                EngineEvent::Analysis { .. } => {}
                EngineEvent::BestMove {
                    search_id: from, ..
                } => {
                    if from == search_id {
                        live.running = false;
                    }
                }
                EngineEvent::Crashed(reason) => crash = Some(reason),
            }
        }
        if analysed {
            self.cache.store(id, hash, &live.lines);
            let position = crate::analysis_store::position_key(&self.fen, &self.moves);
            let limit = crate::analysis_store::current_limit(
                self.limit_override
                    .unwrap_or(self.model.read(cx).config.analysis_limit),
            );
            let key = crate::analysis_store::identity(&position, &live.config, &limit);
            let record = crate::analysis_store::AnalysisRecord {
                position,
                engine: serde_json::to_string(&(
                    live.config.name.as_str(),
                    live.config.version.as_str(),
                    live.config.path.to_string_lossy(),
                    &live.config.options,
                ))
                .unwrap_or_default(),
                limit,
                lines: live.lines.clone(),
                saved_at: crate::model::today(),
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
        if let (Some(node), Some((cp, depth))) = (node, newest_eval) {
            cx.emit(LiveEvent::Eval(node, cp, depth));
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
            }
        }
        cx.emit(LiveEvent::Updated);
        cx.notify();
    }

    /// Applies an option to a running engine without restarting it, and saves it.
    pub fn set_option(&mut self, id: EngineId, name: &str, value: String, cx: &mut Context<Self>) {
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
        self.model.update(cx, |model, cx| {
            model.config.analysis_limit = limit;
            model.save_config(cx);
        });
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
        let moves: Vec<String> = tree
            .path_to(node)
            .iter()
            .filter_map(|&id| tree.node(id).mv)
            .chain(extra.iter().copied())
            .map(|mv| UciMove::from_standard(mv).to_string())
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
            (cp, 0)
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
                    .or_else(|| {
                        let key = crate::analysis_store::identity(
                            &position_key,
                            &engine.config,
                            &search_limit,
                        );
                        self.model.read(cx).analyses.as_ref().and_then(|store| {
                            match store.get(&key) {
                                Ok(Some(mut record)) => {
                                    for line in record.lines.values_mut() {
                                        line.cached = true;
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
                known = Some((score_cp(score), line.update.depth));
            }
        }
        if let (Some(node), Some((cp, depth))) = (self.node, known) {
            cx.emit(LiveEvent::Eval(node, cp, depth));
        }
        self.search(cx);
        cx.emit(LiveEvent::Updated);
        cx.notify();
    }

    /// Starts `go infinite` on every engine for the current position.
    pub fn search(&mut self, cx: &mut App) {
        if !self.on || self.paused || !self.active {
            return;
        }
        let Some(pos) = &self.pos else { return };
        let terminal = self.end.is_some();
        self.search_id += 1;
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
        };
        if demo {
            let mut scripted = crate::demo::scripted_lines(hash);
            let white_to_move = pos.turn().is_white();
            for engine in &mut self.engines {
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
                        engine
                            .log
                            .push(LogKind::Received, crate::demo::info_line(&update));
                        let score = update
                            .score
                            .map(|score| if white_to_move { score } else { score.negate() });
                        let wdl = update
                            .wdl
                            .map(|wdl| if white_to_move { wdl } else { wdl.flip() });
                        let sans = chess_core::pv_to_san(pos, &update.pv);
                        engine.lines.insert(
                            update.multipv,
                            LiveLine {
                                update,
                                score,
                                wdl,
                                sans,
                                cached: false,
                            },
                        );
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
                continue;
            }
            engine.running = true;
            handle.search(SearchRequest {
                fen: self.fen.clone(),
                moves: self.moves.clone(),
                limits,
                search_id: self.search_id,
                game: self.game,
            });
        }
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
            .find(|engine| engine.config.runs_on_gpu && has_data(engine))
            .or_else(|| self.engines.iter().filter(visible).find(has_data))
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
        assert_eq!(cache.eval(&[1], 42), Some((15, 25)));
        // Another engine's analysis of the same position is separate.
        assert_eq!(cache.eval(&[2], 42), None);
        cache.store(2, 42, &lines(8, 50));
        assert_eq!(cache.eval(&[2, 1], 42), Some((50, 8)));
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
