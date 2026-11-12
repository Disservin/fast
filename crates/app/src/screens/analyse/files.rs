//! Loading and saving games: PGN and FEN files, the clipboard and stored database games.

use super::*;

/// Read every selected file without losing successful games when another file fails.
pub(super) fn read_game_files(paths: &[std::path::PathBuf]) -> (Vec<GameTree>, Vec<String>) {
    let mut games = Vec::new();
    let mut errors = Vec::new();
    for path in paths {
        let result = std::fs::read_to_string(path)
            .map_err(|err| err.to_string())
            .and_then(|text| {
                if let Ok(tree) = GameTree::from_fen(text.trim()) {
                    Ok(vec![tree])
                } else {
                    parse_pgn(&text).map_err(|err| err.to_string())
                }
            });
        match result {
            Ok(mut loaded) if !loaded.is_empty() => games.append(&mut loaded),
            Ok(_) => errors.push(format!("{}: no games", path.display())),
            Err(err) => errors.push(format!("{}: {err}", path.display())),
        }
    }
    (games, errors)
}

impl AnalyseScreen {
    pub fn load_tree(&mut self, tree: GameTree, window: &mut Window, cx: &mut Context<Self>) {
        self.tree = tree;
        self.db_game = None;
        self.db_findings.clear();
        // A different game: engines hear `ucinewgame` before its first search.
        self.live.update(cx, |live, _| live.game += 1);
        self.current = GameTree::ROOT;
        self.evals.clear();
        self.depths.clear();
        self.mates.clear();
        self.last_child.clear();
        // Evals and depths recorded in the game itself (Lichess, TCEC) fill the graphs
        // straight away, for reviewing games already played.
        let mut stack = vec![GameTree::ROOT];
        while let Some(id) = stack.pop() {
            let node = self.tree.node(id);
            stack.extend(node.children.iter().copied());
            let note =
                chess_core::parse_move_annotation(&node.comment, !node.ply.is_multiple_of(2));
            if let Some(cp) = note.eval_cp {
                self.evals.insert(id, cp);
                if let Some(mate) = note.mate_distance {
                    self.mates.insert(id, mate);
                }
                self.depths.insert(id, note.depth.unwrap_or(0));
            } else if let Some(depth) = note.depth {
                self.depths.insert(id, depth);
            }
        }
        self.position_changed(false, window, cx);
    }

    /// Opens multiple PGN/FEN files, with each game in its own board tab.
    pub fn open_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let this = cx.entity().downgrade();
        let workspace = self.workspace.clone();
        let target_window = window.window_handle();
        crate::model::pick_paths(self.model.clone(), "Open games", cx, move |paths, cx| {
            let read = cx
                .background_executor()
                .spawn(async move { read_game_files(&paths) });
            cx.spawn(async move |cx| {
                let (games, errors) = read.await;
                let _ = target_window.update(cx, |_, window, cx| {
                    let mut games = games.into_iter();
                    if let Some(first) = games.next()
                        && let Some(this) = this.upgrade()
                    {
                        this.update(cx, |screen, cx| screen.load_tree(first, window, cx));
                    }
                    if let Some(workspace) = workspace.upgrade() {
                        workspace.update(cx, |workspace, cx| {
                            for tree in games {
                                workspace.open_analysis(tree, window, cx);
                            }
                        });
                    }
                    if !errors.is_empty()
                        && let Some(this) = this.upgrade()
                    {
                        this.update(cx, |screen, cx| screen.model.toast(errors.join("; "), cx));
                    }
                });
            })
            .detach();
        });
    }

    /// Loads text that is either a FEN or PGN, e.g. from a file or the clipboard.
    pub fn load_text(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        let text = text.trim();
        if text.is_empty() {
            self.model.toast("Nothing to load.", cx);
            return;
        }
        if GameTree::from_fen(text).is_ok() {
            self.load_fen(text, window, cx);
            return;
        }
        let count = parse_pgn(text).map(|games| games.len()).unwrap_or(0);
        self.load_pgn_at(text, 0, window, cx);
        if count > 1 {
            self.model.toast(
                format!("Loaded the first of {count} games. Import the file in Database to browse them all."),
                cx,
            );
        }
    }

    /// Loads a FEN or PGN from the clipboard.
    pub fn paste_game(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match cx.read_from_clipboard().and_then(|item| item.text()) {
            Some(text) => self.load_text(&text, window, cx),
            None => self.model.toast("The clipboard has no text.", cx),
        }
    }

    /// Writes the game, with variations, comments and engine evals, to a PGN file.
    pub fn save_pgn_as(&mut self, cx: &mut Context<Self>) {
        let pgn = write_pgn(&self.annotated_tree());
        let model = self.model.clone();
        crate::model::pick_path(
            self.model.clone(),
            "Save game as PGN",
            Some("game.pgn"),
            cx,
            move |path, cx| {
                let result = crate::persistence::atomic_write(&path, pgn.as_bytes());
                model.update(cx, |model, cx| match result {
                    Ok(()) => model.toast(format!("Saved {}", path.display()), cx),
                    Err(err) => model.toast(format!("Could not save: {err}"), cx),
                });
            },
        );
    }

    /// Starts an empty game from the initial position.
    pub fn new_game(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.load_tree(GameTree::new(), window, cx);
    }

    pub fn copy_fen_action(&mut self, cx: &mut Context<Self>) {
        self.copy_fen(cx);
    }

    pub fn copy_pgn_action(&mut self, cx: &mut Context<Self>) {
        self.copy_pgn(cx);
    }

    /// Loads a PGN and jumps to a given ply of the main line.
    pub fn load_pgn_at(
        &mut self,
        pgn: &str,
        ply: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match parse_pgn(pgn) {
            Ok(mut games) if !games.is_empty() => {
                let tree = games.remove(0);
                self.load_tree(tree, window, cx);
                let main = self.tree.mainline();
                if let Some(&id) = main.get(ply.saturating_sub(1)).filter(|_| ply > 0) {
                    self.jump(id, window, cx);
                }
            }
            Ok(_) => self.model.toast("That PGN contains no games.", cx),
            Err(err) => self
                .model
                .toast(format!("Could not read the game: {err}"), cx),
        }
    }

    /// Opens a game from a database at `ply`, with its engine review on the eval graph
    /// and its review findings listed above the moves. Saving then updates that game,
    /// comments and variations included.
    pub fn load_db_game(
        &mut self,
        details: &db::GameDetails,
        ply: usize,
        origin: DbGame,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let tree = match parse_pgn(&details.to_pgn()) {
            Ok(mut games) if !games.is_empty() => games.remove(0),
            Ok(_) => return self.model.toast("That PGN contains no games.", cx),
            Err(err) => {
                return self
                    .model
                    .toast(format!("Could not read the game: {err}"), cx);
            }
        };
        let main = tree.mainline();
        let node_at = |ply: usize| match ply {
            0 => Some(GameTree::ROOT),
            ply => main.get(ply - 1).copied(),
        };
        let findings = crate::review::review_marks(details)
            .into_iter()
            .filter_map(|mark| Some((node_at(mark.ply + 1)?, mark)))
            .collect();
        self.load_tree(tree, window, cx);
        self.db_findings = findings;
        self.db_saved_pgn = write_pgn(&self.tree);
        let mate = chess_core::annotation::MATE_CP;
        for position in &details.review_positions {
            if let Some(id) = node_at(position.ply as usize) {
                self.evals.insert(id, position.eval_cp.clamp(-mate, mate));
                self.depths.insert(id, position.depth);
                if let Some(distance) = position.mate_distance {
                    self.mates.insert(id, distance);
                }
            }
        }
        self.db_game = Some(origin);
        if let Some(id) = node_at(ply).filter(|_| ply > 0) {
            self.jump(id, window, cx);
        }
        cx.notify();
    }

    pub fn load_fen(&mut self, fen: &str, window: &mut Window, cx: &mut Context<Self>) {
        match GameTree::from_fen(fen) {
            Ok(tree) => self.load_tree(tree, window, cx),
            Err(err) => self.model.toast(format!("Invalid FEN: {err}"), cx),
        }
    }

    /// Copies the position on the board as FEN, a previewed line's included.
    pub(super) fn copy_fen(&mut self, cx: &mut Context<Self>) {
        let fen = GameTree::fen_of(&self.analysed_pos());
        cx.write_to_clipboard(ClipboardItem::new_string(fen));
        self.copy_menu = false;
        self.model.toast("FEN copied.", cx);
    }

    /// The game with each analysed move's evaluation written into its comment as a
    /// `[%eval]` command, so saved and copied PGN keeps the analysis. An eval the
    /// game already carries stays unless the engine searched deeper.
    pub(super) fn annotated_tree(&self) -> GameTree {
        let mut tree = self.tree.clone();
        for (&id, &cp) in &self.evals {
            if id == GameTree::ROOT {
                continue;
            }
            let depth = self.depths.get(&id).copied();
            let node = tree.node(id);
            let note =
                chess_core::parse_move_annotation(&node.comment, !node.ply.is_multiple_of(2));
            if note.eval_cp.is_some() && note.depth.unwrap_or(0) >= depth.unwrap_or(0) {
                continue;
            }
            let Some(command) = chess_core::eval_command(cp, self.mates.get(&id).copied(), depth)
            else {
                continue;
            };
            let comment = chess_core::with_eval_command(&node.comment, &command);
            tree.node_mut(id).comment = comment;
        }
        tree
    }

    /// Copies the whole game, headers, variations and engine evals included.
    pub(super) fn copy_pgn(&mut self, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(write_pgn(&self.annotated_tree())));
        self.copy_menu = false;
        self.model.toast("PGN copied.", cx);
    }

    /// The copy button and, when open, its menu above it.
    pub(super) fn render_copy_menu(
        &self,
        width: f32,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let this = cx.entity();
        let item = |id: &'static str, label: &'static str, hint: &'static str| {
            h_flex()
                .id(id)
                .justify_between()
                .gap(px(16.))
                .px(px(12.))
                .py(px(7.))
                .text_size(px(14.))
                .cursor_pointer()
                .hover(|style| style.bg(rgb(T.hover)))
                .child(ui::i18n::t(label).to_owned())
                .child(div().text_as(12., T.muted).child(hint))
        };
        let fen_this = this.clone();
        let pgn_this = this.clone();
        let menu = self.copy_menu.then(|| {
            deferred(
                v_flex()
                    .id("copy-menu")
                    .absolute()
                    .right_0()
                    .bottom(px(44.))
                    .w(px(220.))
                    .py(px(4.))
                    .bg(rgb(T.surface))
                    .border_as(T.border_strong)
                    .rounded(px(6.))
                    .shadow_lg()
                    // Keeps the screen's click-away handler from closing it first.
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(
                        item("copy-fen", ui::i18n::t("Copy FEN"), "this position").on_click(
                            move |_, _, cx| fen_this.update(cx, |screen, cx| screen.copy_fen(cx)),
                        ),
                    )
                    .child(
                        item("copy-pgn", ui::i18n::t("Copy PGN"), "whole game").on_click(
                            move |_, _, cx| pgn_this.update(cx, |screen, cx| screen.copy_pgn(cx)),
                        ),
                    ),
            )
            .with_priority(1)
        });
        div()
            .relative()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                icon_btn("copy", "icons/copy.svg")
                    .w(width)
                    .selected(self.copy_menu)
                    .tooltip(ui::i18n::t("Copy FEN or PGN"))
                    .on_click(this.listen(move |screen, _, cx| {
                        screen.copy_menu = !screen.copy_menu;
                        cx.notify();
                    })),
            )
            .children(menu)
    }

    /// Copies the board as an image and saves it under Pictures.
    pub(super) fn snapshot_board(&mut self, cx: &mut Context<Self>) {
        let board = self.board.read(cx);
        let (light, dark) = board.prefs().theme.colors();
        let look = crate::snapshot::BoardLook {
            pieces: board.prefs().pieces,
            light,
            dark,
            flipped: board.is_flipped(),
            last_move: board.last_move(),
        };
        let pos = board.position().clone();
        let ply = self.analysed_ply();
        cx.spawn(async move |this, cx| {
            let saved = cx
                .background_spawn(async move {
                    let png = crate::snapshot::board_png(&pos, &look)?;
                    let dir = crate::snapshot::image_dir();
                    std::fs::create_dir_all(&dir)?;
                    let stamp = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map_or(0, |elapsed| elapsed.as_secs());
                    let path = dir.join(format!("board-{stamp}-ply{ply}.png"));
                    std::fs::write(&path, &png)?;
                    anyhow::Ok((png, path))
                })
                .await;
            let _ = this.update(cx, |screen, cx| match saved {
                Ok((png, path)) => {
                    let image = gpui::Image::from_bytes(gpui::ImageFormat::Png, png);
                    cx.write_to_clipboard(ClipboardItem::new_image(&image));
                    screen.model.toast(
                        format!("Board image copied and saved to {}", path.display()),
                        cx,
                    );
                }
                Err(err) => screen
                    .model
                    .toast(format!("Could not save the board image: {err}"), cx),
            });
        })
        .detach();
    }

    /// Saves the game: back into the database it was opened from, otherwise to
    /// "My games".
    pub fn save_game(&mut self, cx: &mut Context<Self>) {
        if let Some(origin) = self.db_game.clone() {
            return self.save_to_db_game(origin, cx);
        }
        let mut tree = self.annotated_tree();
        let player = self.model.read(cx).config.player_name.clone();
        if tree.header("White").is_none_or(|white| white == "White") {
            tree.set_header("White", &player);
        }
        let pgn = write_pgn(&tree);
        let path = self.model.read(cx).my_games_path();
        let model = self.model.clone();
        cx.spawn(async move |_, cx| {
            let saved = cx
                .background_spawn(async move {
                    let summary = import_games(
                        &path,
                        pgn.as_bytes(),
                        &ImportOptions::default(),
                        &ImportControl::default(),
                        |_| {},
                    )?;
                    if let Ok(database) = Database::open(&path) {
                        let _ = database.set_name("My games");
                    }
                    Ok::<_, db::DbError>(summary)
                })
                .await;
            model.update(cx, |model, cx| {
                match saved {
                    Ok(summary) if summary.games > 0 => model.toast("Saved to My games.", cx),
                    Ok(_) => {
                        model.toast("Could not save: custom start positions are not stored.", cx)
                    }
                    Err(err) => model.toast(format!("Could not save the game: {err}"), cx),
                }
                model.refresh_databases(cx);
            });
        })
        .detach();
    }

    /// Whether the game opened from a database has changes it has not saved back.
    pub(super) fn db_game_dirty(&self) -> bool {
        self.db_game.is_some() && write_pgn(&self.tree) != self.db_saved_pgn
    }

    /// Writes the board's game, comments and variations included, over the stored game
    /// it was opened from.
    pub(super) fn save_to_db_game(&mut self, origin: DbGame, cx: &mut Context<Self>) {
        let pgn = write_pgn(&self.annotated_tree());
        // Unsaved changes are judged on the game without the evals added here.
        let written = write_pgn(&self.tree);
        let model = self.model.clone();
        let DbGame { path, id, name } = origin;
        cx.spawn(async move |this, cx| {
            let saved = cx
                .background_spawn(async move { Database::open(&path)?.replace_game(id, &pgn) })
                .await;
            if matches!(saved, Ok(true)) {
                let _ = this.update(cx, |this, cx| {
                    this.db_saved_pgn = written;
                    cx.notify();
                });
            }
            model.update(cx, |model, cx| {
                match saved {
                    Ok(true) => model.toast(format!("Saved changes to {name}."), cx),
                    Ok(false) => model.toast("Could not save: the game is no longer there.", cx),
                    Err(err) => model.toast(format!("Could not save the game: {err}"), cx),
                }
                model.refresh_databases(cx);
                model.refresh_leaks(cx);
            });
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opening_multiple_files_keeps_every_game_and_reports_individual_failures() {
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("two games.pgn");
        let second = dir.path().join("another.pgn");
        let fen = dir.path().join("position.fen");
        std::fs::write(
            &first,
            "[Event \"One\"]\n\n1. e4 *\n\n[Event \"Two\"]\n\n1. d4 *",
        )
        .unwrap();
        std::fs::write(&second, "[Event \"Three\"]\n\n1. c4 *").unwrap();
        std::fs::write(&fen, GameTree::new().fen(GameTree::ROOT)).unwrap();
        let (games, errors) =
            read_game_files(&[first, dir.path().join("missing.pgn"), second, fen]);
        assert_eq!(games.len(), 4);
        assert_eq!(games[0].header("Event"), Some("One"));
        assert_eq!(games[1].header("Event"), Some("Two"));
        assert_eq!(games[2].header("Event"), Some("Three"));
        assert_eq!(games[0].node(games[0].mainline()[0]).san, "e4");
        assert_eq!(games[1].node(games[1].mainline()[0]).san, "d4");
        assert_eq!(games[2].node(games[2].mainline()[0]).san, "c4");
        assert!(games[3].mainline().is_empty());
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("missing.pgn"));
    }
}
