//! Loading pages of games, selecting one, and opening it in Analyse.

use super::*;

impl DatabaseScreen {
    /// Runs the filter on the selected database. The rows on screen stay until the
    /// new ones arrive.
    pub(super) fn run_query(&mut self, cx: &mut Context<Self>) {
        self.filter_task = None;
        let model = self.model.read(cx);
        let entries = model.databases.clone();
        // A database created a moment ago may not be listed until the re-scan finishes.
        let missing = self
            .selected_db
            .as_ref()
            .is_none_or(|path| !entries.iter().any(|e| &e.path == path));
        if missing && (self.selected_db.is_none() || !model.scanning_databases()) {
            self.selected_db = entries.first().map(|entry| entry.path.clone());
        }
        self.db_snapshot = self
            .selected_db
            .as_ref()
            .and_then(|path| entries.iter().find(|entry| &entry.path == path).cloned());
        self.sort = self.saved_sort(cx);
        let filter = self.build_filter(cx);
        self.summary = self.summary_for(&filter, cx);
        self.applied = filter.clone();
        let Some(path) = self.selected_db.clone() else {
            self.ids.clear();
            self.rows.clear();
            self.stats = FilterStats::default();
            self.querying = false;
            cx.notify();
            return;
        };
        self.querying = true;
        let task = cx.background_spawn(async move {
            let db = Database::open(path)?;
            let indexed = db.has_structure_index()?;
            let ids = db.filtered_ids(&filter)?;
            let stats = db.stats(&filter)?;
            let leak = stats.top_leak.as_ref().and_then(|(left, _)| {
                let sans = db.sample_with_left_rep(left).ok()??;
                Some(leak_context(left, &sans))
            });
            Ok::<_, db::DbError>((ids, stats, leak, indexed))
        });
        self.query_task = Some(cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.querying = false;
                match result {
                    Ok((ids, stats, leak, indexed)) => {
                        this.pattern_indexed = indexed;
                        this.ids = ids;
                        this.stats = stats;
                        this.leak_text = leak;
                        this.rows.clear();
                        this.requested_pages.clear();
                        // Edits and deletions re-run the query; stay on the same game.
                        let kept = this
                            .selected
                            .and_then(|id| this.ids.iter().position(|&other| other == id));
                        match kept {
                            Some(index) => {
                                this.scroll.scroll_to_item(index, ScrollStrategy::Center);
                                this.select_game(Some(this.ids[index]), cx);
                            }
                            None => {
                                this.scroll.scroll_to_item(0, ScrollStrategy::Top);
                                let first = this.ids.first().copied();
                                this.select_game(first, cx);
                            }
                        }
                        this.ensure_rows(0..PAGE.min(this.ids.len()), cx);
                    }
                    Err(err) => this.model.toast(format!("Query failed: {err}"), cx),
                }
                cx.notify();
            });
        }));
    }

    /// Fetches any pages in `range` not loaded yet, off the UI thread.
    pub(super) fn ensure_rows(&mut self, range: Range<usize>, cx: &mut Context<Self>) {
        let Some(path) = self.selected_db.clone() else {
            return;
        };
        let first_page = range.start / PAGE;
        let last_page = range.end.saturating_sub(1) / PAGE;
        for page in first_page..=last_page {
            if !self.requested_pages.insert(page) {
                continue;
            }
            let start = page * PAGE;
            let end = (start + PAGE).min(self.ids.len());
            if start >= end {
                continue;
            }
            let ids = self.ids[start..end].to_vec();
            let path = path.clone();
            let task = cx.background_spawn(async move {
                Database::open(path).and_then(|db| db.rows_by_ids(&ids))
            });
            cx.spawn(async move |this, cx| {
                if let Ok(rows) = task.await {
                    let _ = this.update(cx, |this, cx| {
                        for row in rows {
                            this.rows.insert(row.id, row);
                        }
                        cx.notify();
                    });
                }
            })
            .detach();
        }
    }

    pub(super) fn select_game(&mut self, id: Option<i64>, cx: &mut Context<Self>) {
        self.selected = id;
        self.preview = None;
        let Some((id, path)) = id.zip(self.selected_db.clone()) else {
            self.preview_task = None;
            cx.notify();
            return;
        };
        let task =
            cx.background_spawn(async move { Database::open(path).and_then(|db| db.game(id)) });
        self.preview_task = Some(cx.spawn(async move |this, cx| {
            let found = task.await.ok().flatten();
            let _ = this.update(cx, |this, cx| {
                let Some(details) = found else { return };
                let key =
                    if this.applied.pawn_structure.is_some() || this.applied.material.is_some() {
                        chess_core::parse_pgn(&details.to_pgn())
                            .ok()
                            .and_then(|trees| trees.into_iter().next())
                            .and_then(|tree| {
                                std::iter::once(GameTree::ROOT)
                                    .chain(tree.mainline())
                                    .enumerate()
                                    .find_map(|(ply, node)| {
                                        let pattern =
                                            chess_core::PositionPattern::of(&tree.node(node).pos);
                                        (this
                                            .applied
                                            .pawn_structure
                                            .is_none_or(|hash| hash == pattern.pawn_hash)
                                            && this.applied.material.is_none_or(|material| {
                                                material == pattern.material
                                            }))
                                        .then_some(ply)
                                    })
                            })
                            .unwrap_or_else(|| key_moment(&details))
                    } else {
                        key_moment(&details)
                    };
                this.show_preview(&details, key, cx);
                this.preview_key = key;
                this.preview_notes = move_notes(&details);
                this.preview = Some(details);
                cx.notify();
            });
        }));
        cx.notify();
    }

    pub(super) fn show_preview(&self, details: &GameDetails, key: usize, cx: &mut Context<Self>) {
        let mut tree = if details.start_fen.is_empty() {
            GameTree::new()
        } else if details.chess960 {
            GameTree::from_fen_variant(&details.start_fen, chess_core::Variant::Chess960)
                .unwrap_or_default()
        } else {
            GameTree::from_fen(&details.start_fen).unwrap_or_default()
        };
        let mut node = GameTree::ROOT;
        for san in details.sans.iter().take(key) {
            match tree.add_san(node, san) {
                Ok(next) => node = next,
                Err(_) => break,
            }
        }
        let pos: Chess = tree.node(node).pos.clone();
        let last = tree
            .node(node)
            .mv
            .as_ref()
            .and_then(|mv| Some((mv.from()?, mv.to())));
        self.preview_board
            .update(cx, |board, cx| board.set_position(pos, last, false, cx));
    }

    /// Shows the games in `path` that reach the position with this hash, labelled
    /// with the move that leads there.
    pub fn show_position(
        &mut self,
        path: PathBuf,
        hash: u64,
        label: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.clear_filter_fields(window, cx);
        if self.selected_db.as_ref() != Some(&path) {
            self.selected = None;
        }
        self.selected_db = Some(path);
        self.reached_position = Some((hash, label));
        self.reached = true;
        self.apply_filter(cx);
    }

    /// Opens the previewed game in a new Analyse tab at the previewed move, with its
    /// review. Saving there writes the changes back to this database.
    pub(super) fn open_in_analyse(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(preview), Some(entry)) = (&self.preview, &self.db_snapshot) else {
            return;
        };
        let origin = crate::screens::analyse::DbGame {
            path: entry.path.clone(),
            id: preview.row.id,
            name: entry.name.clone(),
        };
        let key = self.preview_key;
        let details = preview.clone();
        if let Some(workspace) = self.workspace.upgrade() {
            workspace.update(cx, |workspace, cx| {
                workspace.open_db_game(&details, key, origin, window, cx);
            });
        }
    }

    /// Opens a game from the table in Analyse, loading it first when it is not the one
    /// in the preview.
    pub(super) fn open_game_in_analyse(
        &mut self,
        id: i64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self
            .preview
            .as_ref()
            .is_some_and(|preview| preview.row.id == id)
        {
            return self.open_in_analyse(window, cx);
        }
        let Some(path) = self.selected_db.clone() else {
            return;
        };
        self.selected = Some(id);
        let task =
            cx.background_spawn(async move { Database::open(path).and_then(|db| db.game(id)) });
        cx.spawn_in(window, async move |this, cx| {
            let Some(details) = task.await.ok().flatten() else {
                return;
            };
            let _ = this.update_in(cx, |this, window, cx| {
                let key = key_moment(&details);
                this.show_preview(&details, key, cx);
                this.preview_key = key;
                this.preview_notes = move_notes(&details);
                this.preview = Some(details);
                this.open_in_analyse(window, cx);
            });
        })
        .detach();
        cx.notify();
    }
}
