//! Starting, restarting and configuring the analysis engines, and applying their output.

use super::*;

/// The options an engine is started with for analysis: UCI asks the GUI to set
/// `UCI_AnalyseMode` when the engine is analysing rather than playing.
/// Whether a running process for `old` can keep serving `new`: same binary.
pub(super) fn same_process(old: &EngineConfig, new: &EngineConfig) -> bool {
    old.id == new.id && old.path == new.path && old.version == new.version
}

/// Whether every UCI option has the same value, counting unset options as their
/// engine defaults, so saving a default does not count as a change.
pub(super) fn same_options(old: &EngineConfig, new: &EngineConfig) -> bool {
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

impl LiveAnalysis {
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

    pub(super) fn spawn_reader(
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

    pub(super) fn apply_events(
        &mut self,
        id: EngineId,
        batch: Vec<EngineEvent>,
        cx: &mut Context<Self>,
    ) {
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
}
