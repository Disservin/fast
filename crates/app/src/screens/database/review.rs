//! Batch review: analysing many games with an engine.

use super::*;

/// The most engines a batch review may run at once: one per logical CPU.
pub(super) fn max_review_engines() -> u32 {
    std::thread::available_parallelism().map_or(4, |n| n.get() as u32)
}

/// Runs engine review for each game on a worker thread and stores the accuracy.
pub(super) fn review_games(
    engine: &engine::EngineConfig,
    path: &std::path::Path,
    ids: &[i64],
    player: &str,
    settings: ReviewSettings,
    control: ReviewControlRef<'_>,
    mut on_progress: impl FnMut(ReviewProgress),
) -> anyhow::Result<()> {
    use engine::{EngineEvent, EngineHandle, Score, SearchRequest};
    use std::sync::atomic::Ordering;
    let ReviewControlRef {
        cancelled,
        paused,
        next,
        finished,
    } = control;
    /// Counts a game as finished when this engine moves on from it.
    struct Finished<'a>(&'a std::sync::atomic::AtomicUsize);
    impl Drop for Finished<'_> {
        fn drop(&mut self) {
            self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
    }
    const START: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

    let database = Database::open(path)?;
    let options = engine
        .options
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let handle = EngineHandle::spawn(engine.path.clone(), Vec::new(), options);
    struct QuitGuard(EngineHandle);
    impl Drop for QuitGuard {
        fn drop(&mut self) {
            self.0.quit();
        }
    }
    let _quit = QuitGuard(handle.clone());
    let events = handle.events();
    let mut search_id = 0;
    let limit_label = settings.limit.label();
    let job_key = crate::analysis_store::identity("batch-review", engine, &limit_label);
    'games: loop {
        if cancelled.load(Ordering::Relaxed) {
            break;
        }
        let Some(&id) = ids.get(next.fetch_add(1, Ordering::Relaxed)) else {
            break;
        };
        let _finished = Finished(finished);
        let Some(game) = database.game(id)? else {
            continue;
        };
        let start_fen = if game.start_fen.is_empty() {
            START
        } else {
            &game.start_fen
        };
        let Ok(start_tree) = (if game.chess960 {
            chess_core::GameTree::from_fen_variant(start_fen, chess_core::Variant::Chess960)
        } else {
            chess_core::GameTree::from_fen(start_fen)
        }) else {
            continue;
        };
        let mut moves: Vec<String> = Vec::new();
        let mut pos = start_tree.node(chess_core::GameTree::ROOT).pos.clone();
        let first_white = pos.turn().is_white();
        let mut history = vec![pos.clone()];
        let mut evals: Vec<Option<i32>> = Vec::with_capacity(game.sans.len() + 1);
        let mut positions: HashMap<usize, ReviewPosition> = database
            .review_positions(id, &job_key)?
            .into_iter()
            .map(|position| (position.ply as usize, position))
            .collect();
        for san_index in 0..=game.sans.len() {
            if cancelled.load(Ordering::Relaxed) {
                handle.stop();
                break 'games;
            }
            let fen = chess_core::GameTree::fen_of(&pos);
            let (eval, mate_distance, depth, pv) = if let Some(saved) = positions.get(&san_index) {
                (
                    Some(saved.eval_cp),
                    saved.mate_distance,
                    saved.depth,
                    saved.pv.clone(),
                )
            } else if let Some(end) = chess_core::game_end(&pos, &history) {
                let eval = match end {
                    chess_core::GameEnd::Checkmate { winner } if winner.is_white() => 3000,
                    chess_core::GameEnd::Checkmate { .. } => -3000,
                    _ => 0,
                };
                (Some(eval), None, 0, String::new())
            } else {
                let mut eval = None;
                let mut mate_distance = None;
                let mut depth = 0;
                let mut pv = String::new();
                'position_search: loop {
                    if paused.load(Ordering::Relaxed) {
                        handle.stop();
                        while paused.load(Ordering::Relaxed) && !cancelled.load(Ordering::Relaxed) {
                            smol::block_on(smol::Timer::after(std::time::Duration::from_millis(
                                50,
                            )));
                        }
                        if cancelled.load(Ordering::Relaxed) {
                            break 'games;
                        }
                    }
                    search_id += 1;
                    handle.search(SearchRequest {
                        chess960: start_tree.variant == chess_core::Variant::Chess960,
                        search_moves: Vec::new(),
                        fen: start_fen.to_owned(),
                        moves: moves.clone(),
                        limits: Some(settings.limit.limits()),
                        search_id,
                        game: id as u64,
                    });
                    loop {
                        if cancelled.load(Ordering::Relaxed) {
                            handle.stop();
                            break 'games;
                        }
                        if paused.load(Ordering::Relaxed) {
                            handle.stop();
                            while paused.load(Ordering::Relaxed)
                                && !cancelled.load(Ordering::Relaxed)
                            {
                                smol::block_on(smol::Timer::after(
                                    std::time::Duration::from_millis(50),
                                ));
                            }
                            if cancelled.load(Ordering::Relaxed) {
                                break 'games;
                            }
                            continue 'position_search;
                        }
                        match smol::block_on(events.recv()) {
                            Ok(EngineEvent::Analysis {
                                search_id: from,
                                update,
                            }) if from == search_id => {
                                if update.depth >= depth && !update.pv.is_empty() {
                                    depth = update.depth;
                                    pv = update.pv.join(" ");
                                }
                                if let Some(score) = update.score {
                                    let white = if pos.turn().is_white() {
                                        score
                                    } else {
                                        score.negate()
                                    };
                                    eval = Some(match white {
                                        Score::Cp(cp) => cp,
                                        Score::Mate(mate) if mate > 0 => 3000,
                                        Score::Mate(_) => -3000,
                                    });
                                    mate_distance = match score {
                                        Score::Mate(mate) if pos.turn().is_white() => Some(mate),
                                        Score::Mate(mate) => Some(-mate),
                                        Score::Cp(_) => None,
                                    };
                                }
                            }
                            Ok(EngineEvent::BestMove {
                                search_id: from, ..
                            }) if from == search_id => break 'position_search,
                            Ok(EngineEvent::Crashed(reason)) => anyhow::bail!(reason),
                            Ok(_) => {}
                            Err(_) => anyhow::bail!("engine channel closed"),
                        }
                    }
                }
                (eval, mate_distance, depth, pv)
            };
            if let Some(eval_cp) = eval {
                let result = ReviewPosition {
                    ply: san_index as u32,
                    fen,
                    eval_cp,
                    mate_distance,
                    depth,
                    pv: pv.clone(),
                };
                database.set_review_position(id, &job_key, &result)?;
                positions.insert(san_index, result);
            }
            evals.push(eval);
            on_progress(ReviewProgress {
                plies: game.sans.len() + 1,
                ply: san_index + 1,
            });
            let Some(san) = game.sans.get(san_index) else {
                break;
            };
            let parsed: chess_core::shakmaty::san::San = san.parse()?;
            let mv = parsed.to_move(&pos)?;
            moves.push(start_tree.uci(mv));
            pos.play_unchecked(mv);
            history.push(pos.clone());
        }
        let player_is_white = !player.is_empty() && game.row.white.eq_ignore_ascii_case(player);
        let reviewed_white = match settings.side {
            0 => player.is_empty() || player_is_white,
            1 => true,
            2 => false,
            _ => player.is_empty() || player_is_white,
        };
        if evals.len() == game.sans.len() + 1 && evals.iter().all(Option::is_some) {
            let white_accuracy = review_accuracy(&evals, true, first_white);
            let black_accuracy = review_accuracy(&evals, false, first_white);
            database.set_review_summary(
                id,
                &job_key,
                &ReviewSummary {
                    job_key: job_key.clone(),
                    engine: format!("{} {}", engine.name, engine.version),
                    search_limit: limit_label.clone(),
                    white_accuracy,
                    black_accuracy,
                },
                crate::analysis_store::timestamp(),
            )?;
            if let Some(accuracy) = match settings.side {
                0 if reviewed_white => white_accuracy,
                0 => black_accuracy,
                1 => white_accuracy,
                2 => black_accuracy,
                _ => match (white_accuracy, black_accuracy) {
                    (Some(white), Some(black)) => Some((white + black) / 2.0),
                    _ => None,
                },
            } {
                database.set_accuracy(id, accuracy)?;
            }
        }
    }
    Ok(())
}

impl DatabaseScreen {
    /// Opens or closes the batch review settings dialog.
    pub(super) fn set_review_settings_open(&mut self, open: bool, cx: &mut Context<Self>) {
        self.review_settings_open = open;
        cx.notify();
    }

    pub(super) fn toggle_review_pause(&mut self, cx: &mut Context<Self>) {
        if let Some(reviewing) = &self.reviewing {
            let paused = !reviewing.paused.load(std::sync::atomic::Ordering::Relaxed);
            reviewing
                .paused
                .store(paused, std::sync::atomic::Ordering::Relaxed);
        }
        cx.notify();
    }

    pub(super) fn analyse_all(&mut self, cx: &mut Context<Self>) {
        if let Some(reviewing) = &self.reviewing {
            reviewing
                .cancelled
                .store(true, std::sync::atomic::Ordering::Relaxed);
            return;
        }
        let model = self.model.read(cx);
        let Some(engine) = model
            .config
            .engines
            .iter()
            .find(|engine| {
                engine.enabled && engine.use_review && Some(engine.id) == self.review_engine
            })
            .or_else(|| {
                model
                    .config
                    .engines
                    .iter()
                    .find(|engine| engine.enabled && engine.use_review)
            })
            .cloned()
        else {
            self.model.toast(
                "Choose an engine for batch review in Settings › Engines.",
                cx,
            );
            return;
        };
        if model.demo {
            self.model.toast(
                "Batch review needs a real engine; demo mode only has scripted lines.",
                cx,
            );
            return;
        }
        let Some(path) = self.selected_db.clone() else {
            return;
        };
        let ids = if self.review_selected_only {
            self.selected
                .filter(|id| self.ids.contains(id))
                .into_iter()
                .collect()
        } else {
            self.ids.clone()
        };
        let player = self.applied.player.trim().to_owned();
        let settings = ReviewSettings {
            limit: ReviewLimit::ALL[self.review_limit],
            side: self.review_side,
        };
        let engines = (model.config.review_engines.max(1) as usize).min(ids.len().max(1));
        let cancelled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let paused = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let next = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let finished = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        self.reviewing = Some(Reviewing {
            finished: finished.clone(),
            total: ids.len(),
            engines,
            ply: 0,
            plies: 0,
            cancelled: cancelled.clone(),
            paused: paused.clone(),
        });
        let ids = std::sync::Arc::new(ids);
        let (tx, rx) = smol::channel::bounded(1);
        let workers: Vec<_> = (0..engines)
            .map(|_| {
                let (engine, path, ids, player, tx) = (
                    engine.clone(),
                    path.clone(),
                    ids.clone(),
                    player.clone(),
                    tx.clone(),
                );
                let (cancelled, paused, next, finished) = (
                    cancelled.clone(),
                    paused.clone(),
                    next.clone(),
                    finished.clone(),
                );
                cx.background_spawn(async move {
                    let result = review_games(
                        &engine,
                        &path,
                        &ids,
                        &player,
                        settings,
                        ReviewControlRef {
                            cancelled: &cancelled,
                            paused: &paused,
                            next: &next,
                            finished: &finished,
                        },
                        |progress| {
                            let _ = tx.try_send(progress);
                        },
                    );
                    // One failed engine stops the others too, so the error is not hidden.
                    if result.is_err() {
                        cancelled.store(true, std::sync::atomic::Ordering::Relaxed);
                    }
                    result
                })
            })
            .collect();
        drop(tx);
        cx.spawn(async move |this, cx| {
            while let Ok(progress) = rx.recv().await {
                let _ = this.update(cx, |this, cx| {
                    if let Some(reviewing) = &mut this.reviewing {
                        reviewing.ply = progress.ply;
                        reviewing.plies = progress.plies;
                    }
                    cx.notify();
                });
            }
            let mut error = None;
            for worker in workers {
                if let Err(err) = worker.await {
                    error.get_or_insert(err);
                }
            }
            let _ = this.update(cx, |this, cx| {
                this.reviewing = None;
                if let Some(err) = error {
                    this.model.toast(format!("Review stopped: {err:#}"), cx);
                }
                this.apply_filter(cx);
            });
        })
        .detach();
        cx.notify();
    }

    /// The batch review settings over the tab: engine, limit, side, scope and how many
    /// engines run at once.
    pub(super) fn render_review_dialog(&self, cx: &mut Context<Self>) -> AnyElement {
        let available: Vec<(engine::EngineId, SharedString)> = self
            .model
            .read(cx)
            .config
            .engines
            .iter()
            .filter(|engine| engine.enabled && engine.use_review)
            .map(|engine| (engine.id, engine.name.clone().into()))
            .collect();
        let chosen_engine = available
            .iter()
            .find(|(id, _)| Some(*id) == self.review_engine)
            .or(available.first())
            .map(|(id, _)| *id);
        let engines = self.model.read(cx).config.review_engines.max(1);
        let max_engines = max_review_engines();
        let running = self.reviewing.is_some();
        let close = cx.entity();
        let (engine_this, limit_this, side_this, scope_this) =
            (cx.entity(), cx.entity(), cx.entity(), cx.entity());
        let set_engines = |next: u32| {
            let this = cx.entity();
            move |_: &mut gpui::Window, cx: &mut gpui::App| {
                this.update(cx, |screen, cx| {
                    screen
                        .model
                        .edit_config(cx, |config| config.review_engines = next);
                    cx.notify();
                })
            }
        };
        let (fewer, more) = (
            engines.saturating_sub(1).max(1),
            (engines + 1).min(max_engines),
        );
        let row = |label: &'static str, control: AnyElement| {
            h_flex()
                .gap(px(10.))
                .child(
                    div()
                        .w(px(130.))
                        .flex_none()
                        .text_size(px(13.))
                        .text_color(rgb(T.muted))
                        .child(ui::i18n::t(label)),
                )
                .child(div().flex_1().min_w_0().child(control))
        };
        let engine_control = match chosen_engine {
            Some(chosen) => segmented(
                "review-engine",
                available
                    .iter()
                    .map(|(id, name)| (*id, name.clone(), false))
                    .collect(),
                chosen,
                move |id, _, cx| {
                    engine_this.update(cx, |screen, cx| {
                        screen.review_engine = Some(id);
                        cx.notify();
                    })
                },
            )
            .flex_wrap()
            .into_any_element(),
            None => div()
                .text_size(px(13.))
                .text_color(rgb(T.warn))
                .child(ui::i18n::t(
                    "Choose an engine for batch review in Settings › Engines.",
                ))
                .into_any_element(),
        };
        let limit_control = segmented(
            "review-limit",
            ReviewLimit::ALL
                .iter()
                .enumerate()
                .map(|(index, limit)| (index, limit.label().into(), false))
                .collect(),
            self.review_limit,
            move |index, _, cx| {
                limit_this.update(cx, |screen, cx| {
                    screen.review_limit = index;
                    cx.notify();
                })
            },
        )
        .flex_wrap();
        let side_control = segmented(
            "review-side",
            ["Filtered player", "White", "Black", "Both"]
                .into_iter()
                .enumerate()
                .map(|(index, label)| (index, label.into(), false))
                .collect(),
            self.review_side,
            move |index, _, cx| {
                side_this.update(cx, |screen, cx| {
                    screen.review_side = index;
                    cx.notify();
                })
            },
        )
        .flex_wrap();
        let scope_control = segmented(
            "review-scope",
            vec![
                (false, "Filtered games".into(), false),
                (true, "Selected game".into(), false),
            ],
            self.review_selected_only,
            move |selected_only, _, cx| {
                scope_this.update(cx, |screen, cx| {
                    screen.review_selected_only = selected_only;
                    cx.notify();
                })
            },
        );
        let parallel = ui::widgets::stepper("review-engines", engines.to_string())
            .value_width(28.)
            .on_down(!running && fewer != engines, set_engines(fewer))
            .on_up(!running && more != engines, set_engines(more));
        let body = v_flex()
            .px(px(16.))
            .py(px(14.))
            .gap(px(12.))
            .child(row("Engine", engine_control))
            .child(row("Limit", limit_control.into_any_element()))
            .child(row("Side", side_control.into_any_element()))
            .child(row("Games", scope_control.into_any_element()))
            .child(row("Engines at once", parallel.into_any_element()));
        ui::widgets::dialog("review-settings", "Batch review", 560., body, move |cx| {
            close.update(cx, |screen, cx| screen.set_review_settings_open(false, cx))
        })
        .into_any_element()
    }
}
