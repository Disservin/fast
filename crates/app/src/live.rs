//! Continuous multi-engine analysis of one position, throttled for the UI.

use std::{collections::BTreeMap, time::Duration};

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

#[derive(Clone, Debug, Default)]
pub struct LiveLine {
    pub update: AnalysisUpdate,
    /// White-relative score.
    pub score: Option<Score>,
    /// White-relative win/draw/loss.
    pub wdl: Option<Wdl>,
    /// PV converted to SAN from the analysed position.
    pub sans: Vec<String>,
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
    /// An engine reached an evaluation for the analysed node (White-relative cp).
    Eval(NodeId, i32),
}

pub struct LiveAnalysis {
    model: Entity<AppModel>,
    pub engines: Vec<LiveEngine>,
    pub on: bool,
    /// Searching is held: engines stay loaded and keep their last lines, but no
    /// search runs, even when the position changes, until resumed.
    pub paused: bool,
    search_id: u64,
    node: Option<NodeId>,
    pos: Option<chess_core::Chess>,
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
            paused: false,
            search_id: 0,
            node: None,
            pos: None,
            fen: String::new(),
            moves: Vec::new(),
            _model_sub: sub,
        };
        this.restart(cx);
        this
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
            let mut live = LiveEngine {
                config: config.clone(),
                handle: None,
                lines: BTreeMap::new(),
                running: false,
                log: EngineLog::default(),
                _reader: None,
            };
            if !demo && self.on {
                let handle = self.model.update(cx, |model, _| {
                    if existing.is_none() {
                        model.manager.shutdown(config.id);
                    }
                    model.manager.ensure_running(&config)
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
            self.model.update(cx, |model, _| model.manager.shutdown(id));
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
        let mut crash = None;
        let mut newest_eval = None;
        let search_id = self.search_id;
        let node = self.node;
        let pos = self.pos.clone();
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
                    let white_to_move = pos.turn().is_white();
                    let score = update
                        .score
                        .map(|score| if white_to_move { score } else { score.negate() });
                    let wdl = update
                        .wdl
                        .map(|wdl| if white_to_move { wdl } else { wdl.flip() });
                    if update.multipv == 1
                        && let Some(score) = score
                    {
                        newest_eval = Some(match score {
                            Score::Cp(cp) => cp,
                            Score::Mate(mate) => {
                                if mate > 0 {
                                    2000
                                } else {
                                    -2000
                                }
                            }
                        });
                    }
                    let sans = chess_core::pv_to_san(pos, &update.pv);
                    live.lines.insert(
                        update.multipv,
                        LiveLine {
                            update,
                            score,
                            wdl,
                            sans,
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
        if let (Some(node), Some(cp)) = (node, newest_eval) {
            cx.emit(LiveEvent::Eval(node, cp));
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

    /// Stop analysing without changing the on/off preference (Analyse not visible).
    pub fn pause(&mut self) {
        for engine in &self.engines {
            if let Some(handle) = &engine.handle {
                handle.stop();
            }
        }
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
        for mv in extra {
            pos.play_unchecked(*mv);
        }
        self.node = extra.is_empty().then_some(node);
        self.pos = Some(pos);
        self.fen = root_fen;
        self.moves = moves;
        for engine in &mut self.engines {
            engine.lines.clear();
        }
        self.search(cx);
        cx.emit(LiveEvent::Updated);
        cx.notify();
    }

    /// Starts `go infinite` on every engine for the current position.
    pub fn search(&mut self, cx: &mut App) {
        if !self.on || self.paused {
            return;
        }
        let Some(pos) = &self.pos else { return };
        let terminal = pos.is_game_over();
        self.search_id += 1;
        let hash = position_hash(pos);
        let demo = self.model.read(cx).demo;
        let limits = match self.model.read(cx).config.analysis_limit {
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
            engine.running = true;
            handle.search(SearchRequest {
                fen: self.fen.clone(),
                moves: self.moves.clone(),
                limits,
                search_id: self.search_id,
            });
        }
    }

    /// Principal engine for the eval column and line preview.
    pub fn primary(&self) -> Option<&LiveEngine> {
        self.engines
            .iter()
            .find(|engine| !engine.lines.is_empty())
            .or_else(|| self.engines.first())
    }

    /// The engine providing W/D/L: a GPU/WDL engine if running, otherwise the primary.
    pub fn wdl_engine(&self) -> Option<&LiveEngine> {
        self.engines
            .iter()
            .find(|engine| {
                engine.config.runs_on_gpu && engine.lines.values().any(|line| line.wdl.is_some())
            })
            .or_else(|| {
                self.engines
                    .iter()
                    .find(|engine| engine.lines.values().any(|line| line.wdl.is_some()))
            })
    }
}
