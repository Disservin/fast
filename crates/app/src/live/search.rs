//! What the engines search: the position, the limit, restricted moves, and starting the search.

use super::*;

/// Search ids are unique across every Analyse tab. Tabs hand the same engine processes
/// to each other, so a late event from one tab's search can never match another's.
pub(super) static NEXT_SEARCH_ID: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(1);

pub(super) fn next_search_id() -> u64 {
    NEXT_SEARCH_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

impl LiveAnalysis {
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

    pub(super) fn refresh_restricted_search(&mut self, cx: &mut Context<Self>) {
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
}
