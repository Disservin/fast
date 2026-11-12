//! Playing a game: starting it, the engines and clocks, moves, and how it ends.

use super::*;

/// Starts a match at the first PGN line or EPD position in a local book.
pub(super) fn opening_from_book(path: &str, text: &str) -> Result<GameTree, String> {
    if path.to_ascii_lowercase().ends_with(".pgn") {
        chess_core::parse_pgn(text)
            .map_err(|err| err.to_string())?
            .into_iter()
            .next()
            .ok_or_else(|| "The opening book is empty.".to_owned())
    } else if path.to_ascii_lowercase().ends_with(".epd") {
        let record = chess_core::parse_epd(text)
            .into_iter()
            .next()
            .ok_or_else(|| "The opening book is empty.".to_owned())?
            .parsed?;
        GameTree::from_fen(&record.fen).map_err(|err| err.to_string())
    } else {
        Err("Choose a PGN or EPD opening book.".to_owned())
    }
}

/// A side's clock as shown in the game list, such as "180s+2s", or "∞" for a fixed
/// search limit.
fn clock_label(base_ms: u64, increment_ms: u64, fixed_limit: bool) -> String {
    if fixed_limit {
        return "∞".to_owned();
    }
    format!(
        "{}s+{}s",
        crate::controls::time_seconds(base_ms),
        crate::controls::time_seconds(increment_ms)
    )
}

impl PlayScreen {
    pub(super) fn start_demo_game(&mut self, cx: &mut Context<Self>) {
        let mut tree = GameTree::new();
        let mut node = GameTree::ROOT;
        for san in crate::demo::PLAY_LINE.split(' ') {
            match tree.add_san(node, san) {
                Ok(next) => node = next,
                Err(err) => {
                    log::error!("demo line: {err}");
                    return;
                }
            }
        }
        set_game_headers(
            &mut tree,
            &self.model.read(cx).config.player_name,
            "Stockfish",
            false,
            chrono::Utc::now().date_naive(),
        );
        self.moves_scroll.scroll_to_bottom();
        self.viewed = None;
        self.game = Some(Game {
            tree,
            current: node,
            user: Color::White,
            white: None,
            white_ms: 552_000,
            black_ms: 521_000,
            increment_ms: [5_000; 2],
            fixed_limits: [None; 2],
            result: None,
            opponent_name: "Stockfish".to_owned(),
            opponent_rating: Some(self.opponent.elo),
            label: "10+5".to_owned(),
            takebacks: false,
            premoves: true,
            premove: None,
            last_tick: Instant::now(),
            elapsed_ms: 0,
            search_id: 0,
            analysis: Default::default(),
            engine: None,
            logs: [None, None],
            _engine_reader: None,
            _clock: None,
        });
        self.sync_board(false, cx);
        self.start_clock(cx);
    }

    /// Starts a fresh process of `config` for one game, limited to `elo` if given.
    pub(super) fn spawn_engine(
        &self,
        config: &EngineConfig,
        color: Color,
        elo: Option<u32>,
        cx: &mut Context<Self>,
    ) -> (EngineHandle, Task<()>) {
        // Analysis-only settings would just weaken or slow the search.
        let mut options: Vec<(String, String)> = config
            .options
            .iter()
            .filter(|(name, _)| !matches!(name.as_str(), "MultiPV" | "UCI_ShowWDL"))
            .map(|(name, value)| (name.clone(), value.clone()))
            .collect();
        if let Some(elo) = elo {
            options.push(("UCI_LimitStrength".to_owned(), "true".to_owned()));
            options.push(("UCI_Elo".to_owned(), elo.to_string()));
        }
        // Its first search sends `ucinewgame`: each game gets its own process.
        let handle = EngineHandle::spawn(config.path.clone(), Vec::new(), options);
        let reader = self.spawn_reader(&handle, color, cx);
        (handle, reader)
    }

    /// The position the new game starts from, or `None` when the pasted FEN is not valid.
    pub(super) fn start_tree(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<GameTree> {
        let choice = self
            .start_from
            .read(cx)
            .selected_index(cx)
            .map_or(0, |index| index.row);
        let _ = window;
        Some(match choice {
            1 => self
                .workspace
                .upgrade()
                .map(|workspace| {
                    let analyse = workspace.read(cx).analyse.read(cx);
                    GameTree::from_position(analyse.tree.node(analyse.current).pos.clone())
                })
                .unwrap_or_default(),
            2 => {
                // The repertoire's main line.
                let mut tree = GameTree::new();
                let mut node = GameTree::ROOT;
                let model = self.model.read(cx);
                let mut level = &model.repertoire.roots;
                while let Some(first) = level.first() {
                    match tree.add_san(node, &first.san) {
                        Ok(next) => node = next,
                        Err(_) => break,
                    }
                    level = &first.children;
                }
                tree
            }
            4 => GameTree::chess960(
                self.chess960_number
                    .read(cx)
                    .value()
                    .trim()
                    .parse::<u16>()
                    .ok()
                    .filter(|n| *n < 960)?,
            ),
            3 => GameTree::from_fen(self.start_fen.read(cx).value().trim()).ok()?,
            _ => GameTree::new(),
        })
    }

    /// Loads a local opening off the UI thread before starting the engines.
    pub(super) fn start_with_book(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.book_task.is_some() {
            return;
        }
        let path = self.book_path.read(cx).value().trim().to_owned();
        self.book_tree = None;
        if self.mode != Mode::Engines || path.is_empty() {
            self.start_game(window, cx);
            return;
        }
        let load = cx.background_spawn(async move {
            let text = std::fs::read_to_string(&path).map_err(|err| err.to_string())?;
            opening_from_book(&path, &text)
        });
        self.book_task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = load.await;
            let _ = this.update_in(cx, |screen, window, cx| {
                screen.book_task = None;
                match result {
                    Ok(tree) => {
                        screen.book_tree = Some(tree);
                        screen.start_game(window, cx);
                    }
                    Err(err) => screen
                        .model
                        .toast(format!("Could not load opening book: {err}"), cx),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    /// The player's colour for a new game against an engine, drawing it for Random.
    fn user_colour(&self) -> Color {
        match self.colour {
            ColourChoice::White => Color::White,
            ColourChoice::Black => Color::Black,
            ColourChoice::Random if crate::shell::random_u64().is_multiple_of(2) => Color::White,
            ColourChoice::Random => Color::Black,
        }
    }

    pub(super) fn start_game(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let settings = if self.mode == Mode::Engines {
            self.engine_times[0]
                .settings(cx)
                .zip(self.engine_times[1].settings(cx))
        } else {
            self.time_control(cx)
                .map(|(m, i)| ((m, i, None), (m, i, None)))
        };
        let Some((
            (base_ms, increment_ms, white_limit),
            (black_base_ms, black_increment_ms, black_limit),
        )) = settings
        else {
            self.model.toast(
                "Enter a positive time and a nonnegative increment, or a positive search limit.",
                cx,
            );
            return;
        };
        let fixed_limits = [white_limit, black_limit];
        let demo = self.model.read(cx).demo;
        let self_play = self.mode == Mode::Engines;
        let (white_choice, choice) = if self_play {
            (Some(&self.white), &self.black)
        } else {
            (None, &self.opponent)
        };
        let pick = |choice: &EngineChoice| {
            let config = self.chosen_engine(choice, cx)?;
            let elo = (supports_limit(&config) && !choice.full).then_some(choice.elo);
            Some((config, elo))
        };
        // `white` is `Some(None)` when an engine game has no engine to put on that side.
        let white = white_choice.map(pick);
        let (Some((config, elo)), false) = (pick(choice), white == Some(None)) else {
            self.model
                .toast("Add an engine for Play in Settings › Engines first.", cx);
            return;
        };
        let Some(tree) = self
            .book_tree
            .take()
            .or_else(|| self.start_tree(window, cx))
        else {
            self.model.toast("Enter a valid starting position.", cx);
            return;
        };
        if tree.variant == chess_core::Variant::Chess960 {
            let supported = |config: &EngineConfig| {
                config
                    .info
                    .options
                    .iter()
                    .any(|option| option.name == "UCI_Chess960")
            };
            if !demo
                && !(supported(&config)
                    && white.iter().flatten().all(|(white, _)| supported(white)))
            {
                self.model.toast("No Chess960 support", cx);
                return;
            }
            if let Some(number) = chess_core::chess960_number(&tree.node(GameTree::ROOT).pos) {
                self.model
                    .edit_config(cx, |config| config.chess960_number = number);
            }
        }
        let current = tree
            .line_from(GameTree::ROOT)
            .last()
            .copied()
            .unwrap_or(GameTree::ROOT);
        let user = if self_play {
            Color::White
        } else {
            self.user_colour()
        };
        let ms = base_ms;
        let white = white.flatten().map(|(config, elo)| {
            let (engine, reader) = if demo {
                (None, None)
            } else {
                let (engine, reader) = self.spawn_engine(&config, Color::White, elo, cx);
                (Some(engine), Some(reader))
            };
            WhiteEngine {
                name: engine_game_name(&config),
                rating: elo,
                engine,
                _reader: reader,
            }
        });
        let mut game = Game {
            tree,
            current,
            user,
            white,
            white_ms: ms,
            black_ms: black_base_ms,
            increment_ms: [increment_ms, black_increment_ms],
            fixed_limits,
            result: None,
            opponent_name: engine_game_name(&config),
            opponent_rating: elo,
            label: if self_play {
                format!(
                    "{} / {}",
                    clock_label(base_ms, increment_ms, white_limit.is_some()),
                    clock_label(black_base_ms, black_increment_ms, black_limit.is_some())
                )
            } else {
                clock_label(base_ms, increment_ms, false)
            },
            takebacks: self.allow_takebacks && !self_play,
            premoves: self.allow_premoves && !self_play,
            premove: None,
            last_tick: Instant::now(),
            elapsed_ms: 0,
            search_id: 0,
            analysis: Default::default(),
            engine: None,
            logs: [None, None],
            _engine_reader: None,
            _clock: None,
        };
        let player = self.model.read(cx).config.player_name.clone();
        let (white_name, black_name) = if let Some(white) = &game.white {
            (white.name.as_str(), game.opponent_name.as_str())
        } else if user == Color::White {
            (player.as_str(), game.opponent_name.as_str())
        } else {
            (game.opponent_name.as_str(), player.as_str())
        };
        set_game_headers(
            &mut game.tree,
            white_name,
            black_name,
            self_play,
            chrono::Utc::now().date_naive(),
        );
        if !demo {
            let (engine, reader) = self.spawn_engine(
                &config,
                if self_play { Color::Black } else { !user },
                elo,
                cx,
            );
            game._engine_reader = Some(reader);
            game.engine = Some(engine);
        }
        game.logs = [
            game.white
                .as_ref()
                .and_then(|white| white.engine.as_ref())
                .map(EngineHandle::log),
            game.engine.as_ref().map(EngineHandle::log),
        ];
        self.pv_hover = None;
        self.pv_at = None;
        self.moves_scroll.scroll_to_bottom();
        self.viewed = None;
        self.game = Some(game);
        self.preview = None;
        self.show_setup = false;
        if self_play {
            let saved = self.engine_times.each_ref().map(|time| time.saved(cx));
            self.model
                .edit_config(cx, |config| config.play_engine_times = saved);
        }
        self.board
            .update(cx, |board, cx| board.set_flipped(user == Color::Black, cx));
        self.sync_board(false, cx);
        // A pasted position may already be mate, stalemate or a dead draw.
        if let Some(game) = &self.game
            && let Some(end) = game.tree.game_end(game.current)
        {
            self.finish(end.describe().to_owned(), cx);
            return;
        }
        self.start_clock(cx);
        self.maybe_request_engine_move(cx);
        cx.notify();
    }

    /// Receives moves and live search lines without blocking the board.
    pub(super) fn spawn_reader(
        &self,
        handle: &EngineHandle,
        color: Color,
        cx: &mut Context<Self>,
    ) -> Task<()> {
        crate::event_reader::spawn_batched(handle.events(), None, cx, move |this, batch, cx| {
            for event in batch {
                match event {
                    EngineEvent::Ready(info) => {
                        if let Some(game) = &mut this.game
                            && !info.name.trim().is_empty()
                        {
                            game.tree.set_header(
                                if color == Color::White {
                                    "White"
                                } else {
                                    "Black"
                                },
                                &info.name,
                            );
                            if color == Color::White
                                && let Some(white) = &mut game.white
                            {
                                white.name = info.name;
                            } else {
                                game.opponent_name = info.name;
                            }
                            cx.notify();
                        }
                    }
                    EngineEvent::Analysis { search_id, update } => {
                        this.update_analysis(search_id, update, cx);
                    }
                    EngineEvent::BestMove { search_id, best } => {
                        this.engine_move(search_id, best, cx);
                    }
                    EngineEvent::Crashed(reason) => {
                        this.model
                            .toast(format!("The engine stopped: {reason}"), cx);
                        this.finish("Engine stopped".to_owned(), cx);
                    }
                }
            }
        })
    }

    pub(super) fn start_clock(&mut self, cx: &mut Context<Self>) {
        let Some(game) = &mut self.game else { return };
        // Runs under per-move search limits too, so the thinking time stays current.
        game.last_tick = Instant::now();
        game._clock = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(200))
                    .await;
                let keep_going = this.update(cx, |this, cx| this.tick(cx)).unwrap_or(false);
                if !keep_going {
                    return;
                }
            }
        }));
    }

    pub(super) fn tick(&mut self, cx: &mut Context<Self>) -> bool {
        if self.suspended {
            return false;
        }
        let Some(game) = &mut self.game else {
            return false;
        };
        if game.result.is_some() {
            return false;
        }
        let now = Instant::now();
        let elapsed = now.duration_since(game.last_tick).as_millis() as u64;
        game.last_tick = now;
        game.elapsed_ms = game.elapsed_ms.saturating_add(elapsed);
        let turn = game.tree.node(game.current).pos.turn();
        if game.fixed_limits[usize::from(turn == Color::Black)].is_some() {
            cx.notify();
            return true;
        }
        let clock = if turn == Color::White {
            &mut game.white_ms
        } else {
            &mut game.black_ms
        };
        *clock = clock.saturating_sub(elapsed);
        let flagged = *clock == 0;
        if flagged {
            let text = if turn == Color::White {
                "Black wins on time"
            } else {
                "White wins on time"
            };
            self.finish(text.to_owned(), cx);
            return false;
        }
        cx.notify();
        true
    }

    pub(super) fn finish(&mut self, text: String, cx: &mut Context<Self>) {
        let Some(game) = &mut self.game else { return };
        if game.result.is_some() {
            return;
        }
        let result_tag = if text.contains("White wins") {
            "1-0"
        } else if text.contains("Black wins") {
            "0-1"
        } else if text.to_lowercase().contains("draw") || text.contains("Stalemate") {
            "1/2-1/2"
        } else {
            "*"
        };
        game.tree.set_header("Result", result_tag);
        game.result = Some(text);
        game._clock = None;
        let white = game.white.as_mut().and_then(|white| white.engine.take());
        for engine in game.engine.take().into_iter().chain(white) {
            engine.stop();
            engine.quit();
        }
        self.demo_task = None;
        self.board
            .update(cx, |board, cx| board.set_interactive(false, cx));
        cx.notify();
    }

    /// Applies a move from either side: clock, tree, and game-end detection.
    pub(super) fn apply_move(&mut self, mv: Move, cx: &mut Context<Self>) {
        self.tick(cx);
        let Some(game) = &mut self.game else { return };
        if game.result.is_some() {
            return;
        }
        let turn = game.tree.node(game.current).pos.turn();
        if turn == Color::White {
            game.white_ms += game.increment_ms[0];
        } else {
            game.black_ms += game.increment_ms[1];
        }
        game.current = game.tree.add_move(game.current, mv);
        if game.self_play() {
            let side = usize::from(turn == Color::Black);
            let line = &game.analysis[side];
            let score = (line.search_id == Some(game.search_id))
                .then_some(line.score)
                .flatten();
            let clock = game.fixed_limits[side].is_none().then_some(if side == 0 {
                game.white_ms
            } else {
                game.black_ms
            });
            game.tree.node_mut(game.current).comment =
                match_move_comment(game.elapsed_ms, clock, score, line.depth);
            if let Some(nodes) = line
                .nodes
                .filter(|_| line.search_id == Some(game.search_id))
            {
                game.tree
                    .node_mut(game.current)
                    .comment
                    .push_str(&format!(" [%nodes {nodes}]"));
            }
        }
        if game.self_play() {
            game.analysis[usize::from(turn == Color::Black)].think_ms = Some(game.elapsed_ms);
        }
        game.elapsed_ms = 0;
        if self.viewed.is_none() {
            self.moves_scroll.scroll_to_bottom();
        }
        let mut recovery_tree = game.tree.clone();
        recovery_tree.set_header("FastWhiteMs", &game.white_ms.to_string());
        recovery_tree.set_header("FastBlackMs", &game.black_ms.to_string());
        let path = self.recovery_path.clone();
        if let Err(err) =
            crate::recovery::Recovery::save(&path, chess_core::write_pgn(&recovery_tree))
        {
            log::error!("Play recovery failed: {err}");
        }
        // Repetition and the move-count rules end the game too, as on Lichess.
        let end = game.tree.game_end(game.current);
        let after = game.tree.node(game.current).pos.clone();
        // The engine just moved: a queued premove is played now if still legal, and
        // dropped otherwise.
        let premove = if after.turn() == game.user {
            game.premove.take()
        } else {
            None
        };
        self.model
            .read(cx)
            .config
            .board
            .play_move_sound(&mv, &after, end);
        self.sync_board(true, cx);
        if let Some(end) = end {
            self.finish(end.describe().to_owned(), cx);
            return;
        }
        if let Some((from, to)) = premove
            && let Some(mv) = ui::board::premove_move(&after, from, to)
        {
            self.user_move(mv, cx);
        }
    }

    pub(super) fn user_move(&mut self, mv: Move, cx: &mut Context<Self>) {
        let Some(game) = &self.game else { return };
        let turn = game.tree.node(game.current).pos.turn();
        if self.viewed.is_some() || game.result.is_some() || game.self_play() || turn != game.user {
            return;
        }
        self.apply_move(mv, cx);
        self.maybe_request_engine_move(cx);
    }

    pub(super) fn maybe_request_engine_move(&mut self, cx: &mut Context<Self>) {
        let demo = self.model.read(cx).demo;
        let Some(game) = &mut self.game else { return };
        if game.result.is_some() {
            return;
        }
        let pos = &game.tree.node(game.current).pos;
        let turn = pos.turn();
        if turn == game.user && !game.self_play() {
            return;
        }
        if demo {
            let moves = pos.legal_moves();
            let Some(mv) = moves
                .get(game.tree.node(game.current).ply as usize % moves.len().max(1))
                .cloned()
            else {
                return;
            };
            self.demo_task = Some(cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(Duration::from_millis(900))
                    .await;
                let _ = this.update(cx, |this, cx| {
                    this.apply_move(mv, cx);
                    if this.game.as_ref().is_some_and(Game::self_play) {
                        this.maybe_request_engine_move(cx);
                    }
                    cx.notify();
                });
            }));
            return;
        }
        game.search_id += 1;
        let engine = match &game.white {
            Some(white) if turn == Color::White => &white.engine,
            _ => &game.engine,
        };
        let Some(engine) = engine else { return };
        let moves = game
            .tree
            .path_to(game.current)
            .iter()
            .filter_map(|&id| game.tree.node(id).mv)
            .map(|mv| game.tree.uci(mv))
            .collect();
        engine.search(SearchRequest {
            chess960: game.tree.variant == chess_core::Variant::Chess960,
            search_moves: Vec::new(),
            fen: game.tree.fen(GameTree::ROOT),
            moves,
            limits: Some(
                game.fixed_limits[usize::from(turn == Color::Black)].unwrap_or(Limits {
                    white_ms: Some(game.white_ms),
                    black_ms: Some(game.black_ms),
                    white_inc_ms: game.increment_ms[0],
                    black_inc_ms: game.increment_ms[1],
                    ..Default::default()
                }),
            ),
            search_id: game.search_id,
            game: 0,
        });
    }

    pub(super) fn engine_move(
        &mut self,
        search_id: u64,
        best: Option<String>,
        cx: &mut Context<Self>,
    ) {
        if self.suspended {
            return;
        }
        let Some(game) = &self.game else { return };
        if search_id != game.search_id || game.result.is_some() {
            return;
        }
        let pos = game.tree.node(game.current).pos.clone();
        let parsed = best
            .as_deref()
            .and_then(|uci| uci.parse::<UciMove>().ok())
            .and_then(|uci| uci.to_move(&pos).ok());
        match parsed {
            Some(mv) => {
                self.apply_move(mv, cx);
                if self.game.as_ref().is_some_and(Game::self_play) {
                    self.maybe_request_engine_move(cx);
                }
                cx.notify();
            }
            None => {
                self.model.toast("The engine returned no legal move.", cx);
                self.finish("Engine stopped".to_owned(), cx);
            }
        }
    }

    /// Keeps the moving engine's score White-relative and its PV in chess notation.
    pub(super) fn update_analysis(
        &mut self,
        search_id: u64,
        update: engine::AnalysisUpdate,
        cx: &mut Context<Self>,
    ) {
        let Some(game) = &mut self.game else { return };
        if self.suspended
            || game.result.is_some()
            || search_id != game.search_id
            || update.multipv > 1
        {
            return;
        }
        let node = game.tree.node(game.current);
        let black = node.pos.turn() == Color::Black;
        let line = &mut game.analysis[usize::from(black)];
        if line.search_id != Some(search_id) {
            *line = GameAnalysis {
                search_id: Some(search_id),
                ..Default::default()
            };
        }
        if let Some(nodes) = update.nodes {
            line.nodes = Some(nodes);
        }
        if update.depth > 0 {
            line.depth = update.depth;
            line.seldepth = update.seldepth;
        }
        if let Some(hashfull) = update.hashfull {
            line.hashfull = Some(hashfull);
        }
        if !update.bound {
            if let Some(score) = update.score {
                line.score = Some(if black { score.negate() } else { score });
            }
            if !update.pv.is_empty() {
                let mut tree = GameTree::from_position(node.pos.clone());
                let mut current = GameTree::ROOT;
                line.pv = update
                    .pv
                    .iter()
                    .enumerate()
                    .map_while(|(index, uci)| {
                        let position = &tree.node(current).pos;
                        let mv = uci.parse::<UciMove>().ok()?.to_move(position).ok()?;
                        let last = mv.from().map(|from| (from, mv.to()));
                        current = tree.add_move(current, mv);
                        let next = tree.node(current);
                        Some(PvMove {
                            label: chess_core::format_pv(
                                node.ply + index as u32 + 1,
                                std::slice::from_ref(&next.san),
                            ),
                            position: next.pos.clone(),
                            last,
                        })
                    })
                    .collect();
            }
        }
        cx.notify();
    }

    pub(super) fn resign(&mut self, cx: &mut Context<Self>) {
        let Some(game) = &self.game else { return };
        if game.self_play() {
            self.finish("Game stopped".to_owned(), cx);
            return;
        }
        let text = if game.user == Color::White {
            "Black wins by resignation"
        } else {
            "White wins by resignation"
        };
        self.finish(text.to_owned(), cx);
    }

    /// Stops a running engine match and opens the setup for the next one. The stopped
    /// game stays available through Back to game.
    pub(super) fn new_engine_game(&mut self, cx: &mut Context<Self>) {
        if self.game.as_ref().is_some_and(|game| game.result.is_none()) {
            self.finish("Game stopped".to_owned(), cx);
        }
        self.show_setup = true;
        cx.notify();
    }

    pub(super) fn offer_draw(&mut self, cx: &mut Context<Self>) {
        // UCI has no draw protocol. Long, material-light games are accepted.
        const LATE_PLY: u32 = 80;
        const LOW_PIECES: usize = 10;
        let Some(game) = &self.game else { return };
        let node = game.tree.node(game.current);
        let accepted = node.ply >= LATE_PLY || node.pos.board().occupied().count() <= LOW_PIECES;
        if accepted {
            self.finish("Draw agreed".to_owned(), cx);
        } else {
            self.model
                .toast(format!("{} declines the draw.", game.opponent_name), cx);
        }
    }

    pub(super) fn takeback(&mut self, cx: &mut Context<Self>) {
        let Some(game) = &mut self.game else { return };
        if !game.takebacks || game.result.is_some() {
            self.model.toast("Takebacks are off for this game.", cx);
            return;
        }
        // A premove was meant for the position being taken back.
        game.premove = None;
        // Back to the user's previous turn: undo the reply and the user's move.
        let mut node = game.current;
        for _ in 0..2 {
            if let Some(parent) = game.tree.node(node).parent {
                node = parent;
            }
        }
        if game.tree.node(node).pos.turn() != game.user {
            return;
        }
        game.search_id += 1;
        if let Some(engine) = &game.engine {
            engine.stop();
        }
        game.current = node;
        self.viewed = None;
        self.moves_scroll.scroll_to_bottom();
        let keep = game.tree.node(node).children.first().copied();
        if let Some(keep) = keep {
            game.tree.delete_from(keep);
        }
        self.sync_board(false, cx);
        cx.notify();
    }

    pub(super) fn analyse_game(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(game) = &self.game else { return };
        let tree = game.tree.clone();
        // Deferred: opening a tab saves the session, which reads every Play tab,
        // including this one while it is still being updated.
        if let Some(workspace) = self.workspace.upgrade() {
            window.defer(cx, move |window, cx| {
                workspace.update(cx, |workspace, cx| {
                    workspace.open_analysis(tree, window, cx);
                });
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Books seed the played line and reject empty or unsupported files.
    #[test]
    fn opening_books_seed_matches() {
        let tree = opening_from_book("opening.pgn", "1. e4 e5 2. Nf3 *").unwrap();
        assert_eq!(tree.line_from(GameTree::ROOT).len(), 3);
        let tree =
            opening_from_book("opening.epd", "8/8/8/8/8/4k3/8/4K3 w - - id \"test\";").unwrap();
        assert_eq!(tree.line_from(GameTree::ROOT).len(), 0);
        assert!(opening_from_book("empty.epd", "").is_err());
        assert!(opening_from_book("book.bin", "anything").is_err());
    }
}
