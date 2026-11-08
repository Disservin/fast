//! Database: database list + filter form, virtualized games table, preview panel.

use std::{collections::HashMap, ops::Range, path::PathBuf};

use chess_core::{Chess, GameTree, Position, classify, position_hash};
use db::{
    Database, Filter, FilterStats, GameDetails, GameRow, ImportControl, ImportProgress,
    ReviewPosition, ReviewSummary, SideFilter, TimeClass, import_games,
};
use gpui::{
    AppContext as _, Context, Entity, FontWeight, IntoElement, Render, ScrollStrategy,
    SharedString, Subscription, Task, UniformListScrollHandle, WeakEntity, Window, div, prelude::*,
    px, rgb, uniform_list,
};
use gpui_component::{
    input::{InputEvent, InputState},
    select::SelectEvent,
};
use ui::{
    BoardView, MONO, T,
    widgets::{badge, btn, check, h_flex, mono, panel, section_label, stat, v_flex},
};

use crate::{
    controls::{TextSelect, field, new_input, new_select, select_el, text_input},
    model::{AppModel, DbEntry, ModelEvent, ToastExt as _, pick_path},
    shell::Workspace,
};

const COLOURS: [&str; 3] = ["Any", "White", "Black"];
const TIME_CONTROLS: [&str; 5] = ["Any", "Rapid + Classical", "Classical", "Rapid", "Blitz"];
/// Rows fetched per page as the table scrolls.
const PAGE: usize = 200;
const ROW_H: f32 = 37.5;

// Column widths for the games table.
const W_RESULT: f32 = 82.;
const W_LEFT: f32 = 99.;
const W_MOVES: f32 = 78.;
const W_DATE: f32 = 116.;
const W_SOURCE: f32 = 87.;
const W_ACC: f32 = 44.;

struct Importing {
    control: ImportControl,
    progress: ImportProgress,
    file: SharedString,
}

/// Progress of the "Analyse all" batch engine review.
struct Reviewing {
    /// Games finished by all engines together.
    finished: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    total: usize,
    /// Engines reviewing at once.
    engines: usize,
    ply: usize,
    plies: usize,
    cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
    paused: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

/// The position one review engine has reached in its current game.
#[derive(Clone, Copy)]
struct ReviewProgress {
    plies: usize,
    ply: usize,
}

#[derive(Clone, Copy, Debug)]
pub enum ReviewLimit {
    Time(u64),
    Depth(u32),
    Nodes(u64),
}

#[derive(Clone, Copy, Debug)]
struct ReviewSettings {
    limit: ReviewLimit,
    side: usize,
}

/// Shared state of a batch review: stop and pause flags, and the queue of games that
/// every engine takes its next game from.
struct ReviewControlRef<'a> {
    cancelled: &'a std::sync::atomic::AtomicBool,
    paused: &'a std::sync::atomic::AtomicBool,
    next: &'a std::sync::atomic::AtomicUsize,
    finished: &'a std::sync::atomic::AtomicUsize,
}

impl ReviewLimit {
    const ALL: [Self; 4] = [
        Self::Time(100),
        Self::Time(500),
        Self::Depth(18),
        Self::Nodes(500_000),
    ];

    fn label(self) -> String {
        match self {
            Self::Time(ms) => format!("{ms} ms / position"),
            Self::Depth(depth) => format!("Depth {depth}"),
            Self::Nodes(nodes) => format!("{nodes} nodes"),
        }
    }

    fn limits(self) -> engine::Limits {
        match self {
            Self::Time(ms) => engine::Limits {
                movetime_ms: Some(ms),
                ..Default::default()
            },
            Self::Depth(depth) => engine::Limits {
                depth: Some(depth),
                ..Default::default()
            },
            Self::Nodes(nodes) => engine::Limits {
                nodes: Some(nodes),
                ..Default::default()
            },
        }
    }
}

pub struct DatabaseScreen {
    model: Entity<AppModel>,
    workspace: WeakEntity<Workspace>,
    selected_db: Option<PathBuf>,
    db_snapshot: Option<DbEntry>,
    player: Entity<InputState>,
    colour: TextSelect,
    eco: Entity<InputState>,
    opp_rating: Entity<InputState>,
    time_control: TextSelect,
    reached: bool,
    /// Position captured from the Analyse board, and a label for the summary.
    reached_position: Option<(u64, String)>,
    only_left_rep: bool,
    /// Filter currently applied to the table.
    applied: Filter,
    summary: SharedString,
    /// Preview count for the "Show N games" button.
    pending_count: Option<u64>,
    ids: Vec<i64>,
    rows: HashMap<i64, GameRow>,
    requested_pages: std::collections::HashSet<usize>,
    stats: FilterStats,
    leak_text: Option<String>,
    selected: Option<i64>,
    preview: Option<GameDetails>,
    preview_key: usize,
    preview_board: Entity<BoardView>,
    scroll: UniformListScrollHandle,
    importing: Option<Importing>,
    reviewing: Option<Reviewing>,
    review_engine: Option<engine::EngineId>,
    review_limit: usize,
    review_side: usize,
    review_selected_only: bool,
    query_task: Option<Task<()>>,
    count_task: Option<Task<()>>,
    preview_task: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

/// Splits "B90–B99" into its bounds.
fn parse_eco(text: &str) -> (String, String) {
    let text = text.trim().to_uppercase();
    match text.split_once(['–', '-']) {
        Some((low, high)) => (low.trim().to_owned(), high.trim().to_owned()),
        None => (text.clone(), text),
    }
}

/// "1/2-1/2" as "½–½", "1-0" as "1–0".
fn result_text(result: &str) -> String {
    match result {
        "1/2-1/2" => "½–½".to_owned(),
        other => other.replace('-', "–"),
    }
}

impl DatabaseScreen {
    pub fn new(
        model: Entity<AppModel>,
        workspace: WeakEntity<Workspace>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let demo = model.read(cx).demo;
        let me = model.read(cx).config.player_name.clone();
        let player = new_input(
            if demo { me.as_str() } else { "" },
            "Any player",
            window,
            cx,
        );
        let colour = new_select(
            COLOURS
                .iter()
                .map(|text| SharedString::from(ui::i18n::t(text)))
                .collect(),
            usize::from(demo),
            window,
            cx,
        );
        let eco = new_input(if demo { "B90–B99" } else { "" }, "A00–E99", window, cx);
        let opp_rating = new_input(
            if demo { "1900" } else { "" },
            ui::i18n::t("Any"),
            window,
            cx,
        );
        let time_control = new_select(
            TIME_CONTROLS
                .iter()
                .map(|text| SharedString::from(ui::i18n::t(text)))
                .collect(),
            usize::from(demo),
            window,
            cx,
        );
        let preview_board = cx.new(|_| BoardView::new("preview-board").read_only());
        let prefs = model.read(cx).config.board.prefs();
        preview_board.update(cx, |board, cx| board.set_prefs(prefs, cx));
        let reached_position = demo.then(|| {
            let tree = chess_core::sample::najdorf();
            let id = tree.mainline()[10];
            (
                position_hash(&tree.node(id).pos),
                "Najdorf, English Attack".to_owned(),
            )
        });
        let mut subscriptions = Vec::new();
        for input in [&player, &eco, &opp_rating] {
            subscriptions.push(cx.subscribe_in(
                input,
                window,
                |this, _, event, _, cx| match event {
                    InputEvent::Change => this.update_pending_count(cx),
                    InputEvent::PressEnter { .. } => this.apply_filter(cx),
                    _ => {}
                },
            ));
        }
        for select in [&colour, &time_control] {
            subscriptions.push(cx.subscribe(select, |this, _, _: &SelectEvent<_>, cx| {
                this.update_pending_count(cx);
            }));
        }
        subscriptions.push(
            cx.subscribe_in(&model, window, |this, _, event, _, cx| match event {
                ModelEvent::BoardPrefs => {
                    let prefs = this.model.read(cx).config.board.prefs();
                    this.preview_board
                        .update(cx, |board, cx| board.set_prefs(prefs, cx));
                }
                ModelEvent::Databases | ModelEvent::Repertoire => {
                    this.apply_filter(cx);
                }
                ModelEvent::Engines | ModelEvent::Restored => {}
            }),
        );
        let selected_db = model
            .read(cx)
            .databases
            .first()
            .map(|entry| entry.path.clone());
        let review_engine = model
            .read(cx)
            .config
            .engines
            .iter()
            .find(|engine| engine.enabled && engine.use_review)
            .map(|engine| engine.id);
        let mut this = Self {
            model,
            workspace,
            selected_db,
            db_snapshot: None,
            player,
            colour,
            eco,
            opp_rating,
            time_control,
            reached: demo,
            reached_position,
            only_left_rep: false,
            applied: Filter::default(),
            summary: "".into(),
            pending_count: None,
            ids: Vec::new(),
            rows: HashMap::new(),
            requested_pages: Default::default(),
            stats: FilterStats::default(),
            leak_text: None,
            selected: None,
            preview: None,
            preview_key: 0,
            preview_board,
            scroll: UniformListScrollHandle::new(),
            importing: None,
            reviewing: None,
            review_engine,
            review_limit: 0,
            review_side: 0,
            review_selected_only: false,
            query_task: None,
            count_task: None,
            preview_task: None,
            _subscriptions: subscriptions,
        };
        this.apply_filter(cx);
        this
    }

    pub fn breadcrumb(&self) -> SharedString {
        self.db_snapshot
            .as_ref()
            .map_or_else(|| "No database".into(), |entry| entry.name.clone())
    }

    fn build_filter(&self, cx: &gpui::App) -> Filter {
        let (eco_min, eco_max) = parse_eco(self.eco.read(cx).value().as_ref());
        let colour = self
            .colour
            .read(cx)
            .selected_index(cx)
            .map_or(0, |index| index.row);
        let time = self
            .time_control
            .read(cx)
            .selected_index(cx)
            .map_or(0, |index| index.row);
        Filter {
            player: self.player.read(cx).value().to_string(),
            side: match colour {
                1 => SideFilter::White,
                2 => SideFilter::Black,
                _ => SideFilter::Any,
            },
            eco_min,
            eco_max,
            min_opponent_rating: self.opp_rating.read(cx).value().trim().parse().ok(),
            time_classes: match time {
                1 => vec![TimeClass::Rapid, TimeClass::Classical],
                2 => vec![TimeClass::Classical],
                3 => vec![TimeClass::Rapid],
                4 => vec![TimeClass::Blitz],
                _ => Vec::new(),
            },
            position: self
                .reached_position
                .as_ref()
                .filter(|_| self.reached)
                .map(|(hash, _)| *hash),
            only_left_repertoire: self.only_left_rep,
        }
    }

    fn summary_for(&self, filter: &Filter) -> SharedString {
        let mut parts = Vec::new();
        if let Some((_, label)) = self.reached_position.as_ref().filter(|_| self.reached) {
            parts.push(label.clone());
        } else if !filter.eco_min.is_empty() {
            parts.push(format!("ECO {}–{}", filter.eco_min, filter.eco_max));
        }
        match filter.side {
            SideFilter::White => parts.push("as White".to_owned()),
            SideFilter::Black => parts.push("as Black".to_owned()),
            SideFilter::Any => {
                if !filter.player.trim().is_empty() {
                    parts.push(filter.player.trim().to_owned());
                }
            }
        }
        if filter.only_left_repertoire {
            parts.push("left repertoire".to_owned());
        }
        parts.join(", ").into()
    }

    /// Counts matches in the background for the "Show N games" button.
    fn update_pending_count(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.selected_db.clone() else {
            return;
        };
        let filter = self.build_filter(cx);
        let task = cx.background_spawn(async move {
            Database::open(path)
                .and_then(|db| db.stats(&filter))
                .map(|stats| stats.games)
        });
        self.count_task = Some(cx.spawn(async move |this, cx| {
            let count = task.await.ok();
            let _ = this.update(cx, |this, cx| {
                this.pending_count = count;
                cx.notify();
            });
        }));
    }

    pub fn apply_filter(&mut self, cx: &mut Context<Self>) {
        let entries = self.model.read(cx).databases.clone();
        if self
            .selected_db
            .as_ref()
            .is_none_or(|path| !entries.iter().any(|e| &e.path == path))
        {
            self.selected_db = entries.first().map(|entry| entry.path.clone());
        }
        self.db_snapshot = self
            .selected_db
            .as_ref()
            .and_then(|path| entries.iter().find(|entry| &entry.path == path).cloned());
        let filter = self.build_filter(cx);
        self.summary = self.summary_for(&filter);
        self.applied = filter.clone();
        let Some(path) = self.selected_db.clone() else {
            self.ids.clear();
            self.rows.clear();
            self.stats = FilterStats::default();
            self.pending_count = Some(0);
            cx.notify();
            return;
        };
        let task = cx.background_spawn(async move {
            let db = Database::open(path)?;
            let ids = db.filtered_ids(&filter)?;
            let stats = db.stats(&filter)?;
            let leak = stats.top_leak.as_ref().and_then(|(left, _)| {
                let sans = db.sample_with_left_rep(left).ok()??;
                Some(leak_context(left, &sans))
            });
            Ok::<_, db::DbError>((ids, stats, leak))
        });
        self.query_task = Some(cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok((ids, stats, leak)) => {
                        this.pending_count = Some(stats.games);
                        this.ids = ids;
                        this.stats = stats;
                        this.leak_text = leak;
                        this.rows.clear();
                        this.requested_pages.clear();
                        this.scroll.scroll_to_item(0, ScrollStrategy::Top);
                        let first = this.ids.first().copied();
                        this.select_game(first, cx);
                        this.ensure_rows(0..PAGE.min(this.ids.len()), cx);
                    }
                    Err(err) => this.model.toast(format!("Query failed: {err}"), cx),
                }
                cx.notify();
            });
        }));
    }

    /// Fetches any pages in `range` not loaded yet, off the UI thread.
    fn ensure_rows(&mut self, range: Range<usize>, cx: &mut Context<Self>) {
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

    fn select_game(&mut self, id: Option<i64>, cx: &mut Context<Self>) {
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
                let key = key_moment(&details);
                this.show_preview(&details, key, cx);
                this.preview_key = key;
                this.preview = Some(details);
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn show_preview(&self, details: &GameDetails, key: usize, cx: &mut Context<Self>) {
        let mut tree = GameTree::new();
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

    pub fn search_player(&mut self, query: &str, window: &mut Window, cx: &mut Context<Self>) {
        let text = query.to_owned();
        self.player
            .update(cx, |player, cx| player.set_value(text, window, cx));
        self.apply_filter(cx);
    }

    fn open_in_analyse(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(preview) = &self.preview else { return };
        let pgn = preview.to_pgn();
        let key = self.preview_key;
        if let Some(workspace) = self.workspace.upgrade() {
            workspace.update(cx, |workspace, cx| {
                workspace.open_pgn(&pgn, key, window, cx);
            });
        }
    }

    pub fn prompt_import(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if self.importing.is_some() {
            return;
        }
        let this = cx.entity().downgrade();
        pick_path(
            self.model.clone(),
            "Import PGN",
            None,
            cx,
            move |path, cx| {
                let _ = this.update(cx, |this, cx| this.import_file(path, cx));
            },
        );
    }

    /// Streams a PGN into the selected database, with progress and cancel.
    pub fn import_file(&mut self, file: PathBuf, cx: &mut Context<Self>) {
        let target = self
            .selected_db
            .clone()
            .unwrap_or_else(|| self.model.read(cx).my_games_path());
        let control = ImportControl::default();
        self.importing = Some(Importing {
            control: control.clone(),
            progress: ImportProgress::default(),
            file: file
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default()
                .into(),
        });
        let (progress_tx, progress_rx) = smol::channel::bounded(1);
        let worker = cx.background_spawn({
            let target = target.clone();
            async move {
                let reader = std::fs::File::open(&file)?;
                let summary = import_games(&target, reader, "", &control, |progress| {
                    let _ = progress_tx.try_send(progress);
                })?;
                if let Ok(database) = Database::open(&target)
                    && target.file_stem().is_some_and(|stem| stem == "my_games")
                {
                    let _ = database.set_name("My games");
                }
                anyhow::Ok(summary)
            }
        });
        cx.spawn(async move |this, cx| {
            while let Ok(progress) = progress_rx.recv().await {
                let alive = this.update(cx, |this, cx| {
                    if let Some(importing) = &mut this.importing {
                        importing.progress = progress;
                    }
                    cx.notify();
                });
                if alive.is_err() {
                    return;
                }
            }
            let outcome = worker.await;
            let _ = this.update(cx, |this, cx| {
                this.importing = None;
                let message = match outcome {
                    Ok(summary) if summary.cancelled => {
                        format!("Import cancelled after {} games.", summary.games)
                    }
                    Ok(summary) => format!(
                        "Imported {} games ({} skipped).",
                        summary.games, summary.skipped
                    ),
                    Err(err) => format!("Import failed: {err:#}"),
                };
                this.selected_db = Some(target);
                this.model.update(cx, |model, cx| {
                    model.toast(message, cx);
                    model.refresh_databases();
                    model.refresh_leaks(cx);
                    cx.emit(ModelEvent::Databases);
                });
            });
        })
        .detach();
        cx.notify();
    }

    fn export_pgn(&mut self, cx: &mut Context<Self>) {
        let Some(db_path) = self.selected_db.clone() else {
            return;
        };
        let ids = self.ids.clone();
        let this = cx.entity().downgrade();
        let model = self.model.clone();
        pick_path(
            model,
            "Export PGN",
            Some("games.pgn"),
            cx,
            move |path, cx| {
                let Some(this) = this.upgrade() else { return };
                this.update(cx, |_, cx| {
                    cx.spawn(async move |this, cx| {
                        let written = cx
                            .background_spawn(async move {
                                use std::io::Write as _;
                                let database = Database::open(db_path)?;
                                let mut out = std::io::BufWriter::new(std::fs::File::create(path)?);
                                let mut count = 0usize;
                                for id in ids {
                                    if let Some(game) = database.game(id)? {
                                        writeln!(out, "{}", game.to_pgn())?;
                                        count += 1;
                                    }
                                }
                                out.flush()?;
                                anyhow::Ok(count)
                            })
                            .await;
                        let _ = this.update(cx, |this, cx| {
                            let text = match written {
                                Ok(count) => format!("Exported {count} games."),
                                Err(err) => format!("Export failed: {err:#}"),
                            };
                            this.model.toast(text, cx);
                        });
                    })
                    .detach();
                });
            },
        );
    }

    /// Batch engine review: scores each filtered game and stores its accuracy.
    fn cycle_review_engine(&mut self, cx: &mut Context<Self>) {
        let available: Vec<_> = self
            .model
            .read(cx)
            .config
            .engines
            .iter()
            .filter(|engine| engine.enabled && engine.use_review)
            .map(|engine| (engine.id, engine.name.clone()))
            .collect();
        if available.is_empty() {
            self.review_engine = None;
        } else {
            let current = available
                .iter()
                .position(|(id, _)| Some(*id) == self.review_engine)
                .unwrap_or(0);
            self.review_engine = Some(available[(current + 1) % available.len()].0);
        }
        cx.notify();
    }

    fn cycle_review_limit(&mut self, cx: &mut Context<Self>) {
        self.review_limit = (self.review_limit + 1) % ReviewLimit::ALL.len();
        cx.notify();
    }

    fn cycle_review_side(&mut self, cx: &mut Context<Self>) {
        self.review_side = (self.review_side + 1) % 4;
        cx.notify();
    }

    fn toggle_review_scope(&mut self, cx: &mut Context<Self>) {
        self.review_selected_only = !self.review_selected_only;
        cx.notify();
    }

    fn toggle_review_pause(&mut self, cx: &mut Context<Self>) {
        if let Some(reviewing) = &self.reviewing {
            let paused = !reviewing.paused.load(std::sync::atomic::Ordering::Relaxed);
            reviewing
                .paused
                .store(paused, std::sync::atomic::Ordering::Relaxed);
        }
        cx.notify();
    }

    fn analyse_all(&mut self, cx: &mut Context<Self>) {
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

    fn render_left(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let this = cx.entity();
        let entries = self.model.read(cx).databases.clone();
        let mut list = v_flex().mb(px(20.));
        for (index, entry) in entries.iter().enumerate() {
            let selected = self.selected_db.as_ref() == Some(&entry.path);
            let path = entry.path.clone();
            let this_click = this.clone();
            list = list.child(
                h_flex()
                    .id(("db", index))
                    .h(px(35.))
                    .justify_between()
                    .cursor_pointer()
                    .border_b_1()
                    .border_color(rgb(T.divider))
                    .hover(|style| style.bg(rgb(T.hover)))
                    .on_click(move |_, _, cx| {
                        let path = path.clone();
                        this_click.update(cx, |screen, cx| {
                            screen.selected_db = Some(path);
                            screen.apply_filter(cx);
                        });
                    })
                    .child(
                        div()
                            .when(selected, |name| name.font_weight(FontWeight::SEMIBOLD))
                            .child(entry.name.clone()),
                    )
                    .child(
                        mono(format_count(entry.games))
                            .text_size(px(14.))
                            .text_color(rgb(T.muted)),
                    ),
            );
        }
        if entries.is_empty() {
            list = list.child(
                div()
                    .py(px(8.))
                    .text_size(px(13.))
                    .text_color(rgb(T.muted))
                    .child(ui::i18n::t("No databases yet. Use Import to add a PGN.")),
            );
        }
        let reached = self.reached;
        let only_left = self.only_left_rep;
        let count = self.pending_count.unwrap_or(self.stats.games);
        let reached_this = this.clone();
        let only_this = this.clone();
        let show_this = this.clone();
        v_flex()
            .id("database-left")
            .w(px(281.))
            .h_full()
            .flex_none()
            .overflow_y_scroll()
            .px(px(18.))
            .pt(px(16.))
            .pb(px(16.))
            .bg(rgb(T.surface))
            .border_r_1()
            .border_color(rgb(T.border))
            .child(
                h_flex()
                    .justify_between()
                    .items_center()
                    .mb(px(5.))
                    .child(section_label(ui::i18n::t("Databases")))
                    .child({
                        let import_this = this.clone();
                        btn("import", ui::i18n::t("Import"))
                            .mini()
                            .icon("icons/plus.svg")
                            .tooltip(ui::i18n::t("Import a PGN file into the selected database"))
                            .on_click(move |window, cx| {
                                import_this
                                    .update(cx, |screen, cx| screen.prompt_import(window, cx));
                            })
                    }),
            )
            .child(list)
            .child(section_label("Filter").mb(px(8.)))
            .child(field("Player", text_input(&self.player)).mb(px(7.)))
            .child(
                div()
                    .grid()
                    .grid_cols(2)
                    .gap_x(px(10.))
                    .gap_y(px(7.))
                    .mb(px(10.))
                    .child(field("Colour", select_el(&self.colour)))
                    .child(field("ECO", text_input(&self.eco)))
                    .child(field("Opp. rating ≥", text_input(&self.opp_rating)))
                    .child(field("Time control", select_el(&self.time_control))),
            )
            .child(
                div().mb(px(10.)).child(
                    check("reached", reached, "Reached the Analyse board position")
                        .text(13.)
                        .on_change(move |value, _, cx| {
                            reached_this.update(cx, |screen, cx| screen.toggle_reached(value, cx));
                        }),
                ),
            )
            .child(
                div().mb(px(10.)).child(
                    check(
                        "left-rep",
                        only_left,
                        "Only games where I left my repertoire",
                    )
                    .on_change(move |value, _, cx| {
                        only_this.update(cx, |screen, cx| {
                            screen.only_left_rep = value;
                            screen.update_pending_count(cx);
                            cx.notify();
                        });
                    }),
                ),
            )
            .child(
                btn("show-games", format!("Show {} games", format_count(count)))
                    .primary()
                    .full()
                    .on_click(move |_, cx| {
                        show_this.update(cx, |screen, cx| screen.apply_filter(cx));
                    }),
            )
    }

    fn toggle_reached(&mut self, value: bool, cx: &mut Context<Self>) {
        self.reached = value;
        if value && let Some(workspace) = self.workspace.upgrade() {
            // Capture whatever is on the Analyse board right now.
            let analyse = workspace.read(cx).analyse.read(cx);
            let node = analyse.tree.node(analyse.current);
            let path = analyse.tree.path_to(analyse.current);
            let label = classify(path.iter().map(|&id| analyse.tree.node(id).san.as_str()))
                .map(|opening| opening.name.to_owned())
                .unwrap_or_else(|| "Analyse board position".to_owned());
            self.reached_position = Some((position_hash(&node.pos), label));
        }
        self.update_pending_count(cx);
        cx.notify();
    }

    fn render_main_header(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let this = cx.entity();
        let analyse_label: SharedString = match &self.reviewing {
            Some(reviewing) => format!(
                "Stop review {}/{}",
                reviewing
                    .finished
                    .load(std::sync::atomic::Ordering::Relaxed)
                    .min(reviewing.total),
                reviewing.total
            )
            .into(),
            None => "Analyse all".into(),
        };
        let analyse_this = this.clone();
        h_flex()
            .flex_wrap()
            .justify_between()
            .gap(px(8.))
            .min_h(px(36.))
            .child(
                h_flex()
                    .min_w_0()
                    .gap(px(12.))
                    .items_baseline()
                    .child(
                        div()
                            .text_size(px(20.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(format!("{} games", format_count(self.stats.games))),
                    )
                    .child(
                        div()
                            .text_size(px(14.))
                            .text_color(rgb(T.muted))
                            .child(self.summary.clone()),
                    ),
            )
            .child(
                h_flex()
                    .gap(px(12.))
                    .child(
                        btn("analyse-all", analyse_label)
                            .disabled(self.review_selected_only && self.selected.is_none())
                            .on_click(move |_, cx| {
                                analyse_this.update(cx, |screen, cx| screen.analyse_all(cx));
                            }),
                    )
                    .child(btn("export", "Export PGN").on_click(move |_, cx| {
                        this.update(cx, |screen, cx| screen.export_pgn(cx));
                    })),
            )
    }

    fn render_review_controls(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let engines = self.model.read(cx).config.engines.clone();
        let engine_name = engines
            .iter()
            .find(|engine| {
                Some(engine.id) == self.review_engine && engine.enabled && engine.use_review
            })
            .or_else(|| {
                engines
                    .iter()
                    .find(|engine| engine.enabled && engine.use_review)
            })
            .map_or("No review engine", |engine| engine.name.as_str());
        let this = cx.entity();
        let engine_this = this.clone();
        let limit_this = this.clone();
        let side_this = this.clone();
        let engines = self.model.read(cx).config.review_engines.max(1);
        let max_engines = max_review_engines();
        let running = self.reviewing.is_some();
        let step = |up: bool| {
            let this = this.clone();
            let next = if up {
                (engines + 1).min(max_engines)
            } else {
                engines.saturating_sub(1).max(1)
            };
            btn(
                if up {
                    "review-engines-up"
                } else {
                    "review-engines-down"
                },
                if up { "+" } else { "−" },
            )
            .w(36.)
            .pad(0.)
            .disabled(running || next == engines)
            .on_click(move |_, cx| {
                this.update(cx, |screen, cx| {
                    screen.model.update(cx, |model, cx| {
                        model.config.review_engines = next;
                        model.save_config(cx);
                    });
                    cx.notify();
                })
            })
        };
        let parallel = h_flex()
            .gap(px(4.))
            .child(
                div()
                    .text_size(px(13.))
                    .text_color(rgb(T.muted))
                    .mr(px(2.))
                    .child(ui::i18n::t("Engines at once")),
            )
            .child(step(false))
            .child(
                mono(engines.to_string())
                    .w(px(28.))
                    .text_size(px(13.))
                    .flex()
                    .justify_center()
                    .whitespace_nowrap(),
            )
            .child(step(true));
        h_flex()
            .flex_wrap()
            .gap(px(8.))
            .child(section_label("Batch review"))
            .child(
                btn("review-engine", format!("Engine: {engine_name}  ›"))
                    .tooltip(ui::i18n::t("Choose an enabled review engine"))
                    .on_click(move |_, cx| {
                        engine_this.update(cx, |screen, cx| screen.cycle_review_engine(cx))
                    }),
            )
            .child(
                btn(
                    "review-limit",
                    format!("Limit: {}  ›", ReviewLimit::ALL[self.review_limit].label()),
                )
                .tooltip(ui::i18n::t("Cycle between time, depth, and node limits"))
                .on_click(move |_, cx| {
                    limit_this.update(cx, |screen, cx| screen.cycle_review_limit(cx))
                }),
            )
            .child(
                btn(
                    "review-side",
                    format!(
                        "Side: {}  ›",
                        [
                            "Filtered player",
                            ui::i18n::t("White"),
                            ui::i18n::t("Black"),
                            "Both"
                        ][self.review_side]
                    ),
                )
                .tooltip(ui::i18n::t("Choose which player's moves to review"))
                .on_click(move |_, cx| {
                    side_this.update(cx, |screen, cx| screen.cycle_review_side(cx))
                }),
            )
            .child(parallel)
            .child(
                btn(
                    "review-scope",
                    if self.review_selected_only {
                        "Scope: Selected"
                    } else {
                        "Scope: Filtered"
                    },
                )
                .tooltip(ui::i18n::t(
                    "Review the selected game or all filtered games",
                ))
                .on_click(move |_, cx| {
                    this.update(cx, |screen, cx| screen.toggle_review_scope(cx))
                }),
            )
            .when_some(self.reviewing.as_ref(), |row, reviewing| {
                let pause_this = cx.entity();
                row.child(
                    btn(
                        "review-pause",
                        if reviewing.paused.load(std::sync::atomic::Ordering::Relaxed) {
                            ui::i18n::t("Resume")
                        } else {
                            ui::i18n::t("Pause")
                        },
                    )
                    .on_click(move |_, cx| {
                        pause_this.update(cx, |screen, cx| screen.toggle_review_pause(cx))
                    }),
                )
                .when(reviewing.engines == 1, |row| {
                    row.child(
                        mono(format!("Position {}/{}", reviewing.ply, reviewing.plies))
                            .text_size(px(12.)),
                    )
                })
            })
    }

    fn render_stats(&self) -> impl IntoElement + use<> {
        let cell = |label: &'static str, value: gpui::AnyElement| {
            v_flex()
                .flex_1()
                .px(px(16.))
                .py(px(12.))
                .gap(px(2.))
                .child(section_label(label))
                .child(value)
        };
        let leak = self.leak_text.clone().unwrap_or_else(|| "—".to_owned());
        panel()
            .flex()
            .flex_row()
            .flex_none()
            .h(px(77.))
            .child(cell(
                ui::i18n::t("Score"),
                mono(format!("{:.1}%", self.stats.score_percent))
                    .text_size(px(22.))
                    .font_weight(FontWeight::MEDIUM)
                    .into_any_element(),
            ))
            .child(div().w(px(1.)).h_full().bg(rgb(T.border)))
            .child(cell(
                "Most common leak",
                mono(leak)
                    .text_size(px(16.))
                    .mt(px(3.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(rgb(T.warn))
                    .into_any_element(),
            ))
    }

    fn render_table(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let header = h_flex()
            .h(px(35.))
            .flex_none()
            .px(px(12.))
            .bg(rgb(T.surface_2))
            .border_b_1()
            .border_color(rgb(T.border))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .pr(px(10.))
                    .child(section_label(ui::i18n::t("White"))),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .pr(px(10.))
                    .child(section_label(ui::i18n::t("Black"))),
            )
            .child(
                div()
                    .w(px(W_RESULT))
                    .child(section_label(ui::i18n::t("Result"))),
            )
            .child(div().w(px(W_LEFT)).child(section_label("Left rep.")))
            .child(
                div()
                    .w(px(W_MOVES))
                    .child(section_label(ui::i18n::t("Moves"))),
            )
            .child(
                div()
                    .w(px(W_DATE))
                    .child(section_label(ui::i18n::t("Date"))),
            )
            .child(
                div()
                    .w(px(W_SOURCE))
                    .child(section_label(ui::i18n::t("Source"))),
            )
            .child(div().w(px(W_ACC)).flex_none().child(section_label("Acc.")));
        let count = self.ids.len();
        let list = uniform_list(
            "games-table",
            count,
            cx.processor(|this, range: Range<usize>, _window, cx| {
                this.ensure_rows(range.clone(), cx);
                range
                    .map(|index| this.render_row(index, cx).into_any_element())
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(&self.scroll)
        .flex_1()
        .min_h_0();
        let rows_height = (count as f32 * ROW_H).min(450.) + 35.;
        panel()
            .flex()
            .flex_col()
            .max_h(px(rows_height + 2.))
            .h_full()
            .overflow_hidden()
            .child(header)
            .child(list)
    }

    fn render_row(&mut self, index: usize, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let id = self.ids[index];
        let selected = self.selected == Some(id);
        let this = cx.entity();
        let row_el = h_flex()
            .id(("game", index))
            .relative()
            .w_full()
            .h(px(ROW_H))
            .px(px(12.))
            .cursor_pointer()
            .border_b_1()
            .border_color(rgb(T.divider))
            .text_size(px(13.))
            .when(selected, |row| row.bg(rgb(T.selected)))
            .when(!selected, |row| row.hover(|style| style.bg(rgb(T.hover))))
            .on_click(move |_, _, cx| {
                this.update(cx, |screen, cx| screen.select_game(Some(id), cx));
            })
            .when(selected, |row| {
                row.child(
                    div()
                        .absolute()
                        .left_0()
                        .top_0()
                        .bottom_0()
                        .w(px(3.))
                        .bg(rgb(T.accent)),
                )
            });
        let Some(game) = self.rows.get(&id) else {
            return row_el.child(div().text_color(rgb(T.faint)).child("…"));
        };
        let name = |name: &str, elo: Option<i64>, winner: bool| {
            h_flex()
                .flex_1()
                .min_w_0()
                .pr(px(10.))
                .gap(px(5.))
                .overflow_hidden()
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_color(rgb(T.fg))
                        .when(winner, |text| text.font_weight(FontWeight::BOLD))
                        .child(SharedString::from(name.to_owned())),
                )
                .children(elo.map(|elo| {
                    mono(elo.to_string())
                        .flex_none()
                        .text_color(rgb(T.muted))
                        .text_size(px(14.))
                }))
        };
        let white_won = game.result == "1-0";
        let black_won = game.result == "0-1";
        let mono_cell = |text: String, width: f32, color: u32| {
            mono(text)
                .w(px(width))
                .flex_none()
                .text_size(px(13.))
                .text_color(rgb(color))
        };
        row_el
            .child(name(&game.white, game.white_elo, white_won))
            .child(name(&game.black, game.black_elo, black_won))
            .child(mono_cell(result_text(&game.result), W_RESULT, T.fg))
            .child(match &game.left_rep {
                Some(left) => mono_cell(left.clone(), W_LEFT, T.warn),
                None => mono_cell("—".to_owned(), W_LEFT, T.muted),
            })
            .child(mono_cell(game.moves_count().to_string(), W_MOVES, T.fg_2))
            .child(mono_cell(game.date.clone(), W_DATE, T.muted))
            .child(
                div()
                    .w(px(W_SOURCE))
                    .flex_none()
                    .text_color(rgb(T.fg_2))
                    .child(SharedString::from(game.source.clone())),
            )
            .child(
                mono(
                    game.accuracy
                        .map(|accuracy| format!("{accuracy:.1}"))
                        .unwrap_or_else(|| "—".to_owned()),
                )
                .w(px(W_ACC))
                .flex_none()
                .text_size(px(13.))
                .text_color(rgb(T.fg)),
            )
    }

    fn render_preview(&mut self, scale: f32, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let this = cx.entity();
        let mut column = v_flex().p(px(16.));
        column = column.child(section_label(ui::i18n::t("Preview")).mb(px(9.)));
        if let Some(preview) = &self.preview {
            let row = &preview.row;
            let mut meta = Vec::new();
            if !row.event.is_empty() && row.event != "?" {
                let round = if row.round.is_empty() || row.round == "?" {
                    String::new()
                } else {
                    format!(", Round {}", row.round)
                };
                meta.push(format!("{}{round}", row.event));
            }
            meta.push(result_text(&row.result));
            meta.push(format!("{} moves", row.moves_count()));
            column = column
                .child(
                    div()
                        .text_size(px(14.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .mb(px(6.))
                        .child(format!("{} – {}", row.white, row.black)),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(rgb(T.muted))
                        .mb(px(9.))
                        .child(meta.join(", ")),
                );
            if let Some(review) = &preview.review_summary {
                column = column.child(
                    h_flex()
                        .flex_wrap()
                        .gap(px(6.))
                        .mb(px(8.))
                        .child(section_label("Review").mr(px(4.)))
                        .child(stat(
                            ui::i18n::t("White"),
                            review
                                .white_accuracy
                                .map_or_else(|| "—".to_owned(), |value| format!("{value:.1}%")),
                        ))
                        .child(stat(
                            ui::i18n::t("Black"),
                            review
                                .black_accuracy
                                .map_or_else(|| "—".to_owned(), |value| format!("{value:.1}%")),
                        ))
                        .child(badge(format!(
                            "{} ({})",
                            review.engine, review.search_limit
                        ))),
                );
            }
            let findings = review_findings(preview);
            if !findings.is_empty() {
                let mut list = v_flex()
                    .mt(px(2.))
                    .mb(px(10.))
                    .border_t_1()
                    .border_color(rgb(T.divider));
                for finding in findings.into_iter().take(6) {
                    let this = this.clone();
                    let ply = finding.ply;
                    list = list.child(FindingRow {
                        finding,
                        on_click: std::rc::Rc::new(move |window, cx| {
                            this.update(cx, |screen, cx| {
                                screen.preview_key = ply;
                                screen.open_in_analyse(window, cx);
                            });
                        }),
                    });
                }
                column = column
                    .child(section_label("Review findings").mt(px(6.)))
                    .child(list);
            }
        }
        column
            .child(
                div()
                    .size(ui::board::snap_board_size(px(249.), scale))
                    .flex_none()
                    .child(self.preview_board.clone()),
            )
            .when_some(self.preview.as_ref(), |column, preview| {
                column.child(move_snippet(preview, self.preview_key))
            })
            .child(
                btn("open-analyse", "Open in Analyse")
                    .full()
                    .disabled(self.preview.is_none())
                    .on_click(move |window, cx| {
                        this.update(cx, |screen, cx| screen.open_in_analyse(window, cx));
                    }),
            )
    }

    fn render_import_bar(&self, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        let importing = self.importing.as_ref()?;
        let this = cx.entity();
        Some(
            panel()
                .flex()
                .flex_row()
                .flex_none()
                .items_center()
                .justify_between()
                .px(px(16.))
                .py(px(8.))
                .child(div().text_size(px(14.)).child(format!(
                    "Importing {}: {} games",
                    importing.file,
                    format_count(importing.progress.games)
                )))
                .child(btn("cancel-import", ui::i18n::t("Cancel")).h(30.).on_click(
                    move |_, cx| {
                        this.update(cx, |screen, _| {
                            if let Some(importing) = &screen.importing {
                                importing.control.cancel();
                            }
                        });
                    },
                )),
        )
    }
}

/// "13.Ne2 Ne8 14.h4?!" from a stored game and its deviation like "14.h4".
fn leak_context(left: &str, sans: &[String]) -> String {
    let Some((number, san)) = left.split_once(['.', '…']) else {
        return left.to_owned();
    };
    let Ok(number) = number.parse::<usize>() else {
        return left.to_owned();
    };
    let black = left.contains('…');
    let ply = (number - 1) * 2 + usize::from(black);
    if ply >= sans.len() {
        return left.to_owned();
    }
    let start = ply.saturating_sub(2);
    let line: Vec<String> = sans[start..ply].to_vec();
    let mut text = chess_core::format_pv(start as u32, &line);
    if !text.is_empty() {
        text.push(' ');
    }
    format!("{text}{number}{}{san}?!", if black { "…" } else { "." })
}

/// The ply to show in the preview: where the game left the repertoire, or the point
/// where the opening's named line ends. Games shorter than that show the final move.
fn key_moment(details: &GameDetails) -> usize {
    if let Some(left) = &details.row.left_rep {
        let san = left.rsplit(['.', '…']).next().unwrap_or("");
        let number = left
            .split(['.', '…'])
            .next()
            .and_then(|n| n.parse::<usize>().ok());
        if let Some(number) = number {
            let ply = (number - 1) * 2 + usize::from(left.contains('…'));
            if details
                .sans
                .get(ply)
                .is_some_and(|played| played.trim_end_matches(['+', '#']) == san)
            {
                return ply;
            }
        }
    }
    // Otherwise a middlegame moment just past the named opening.
    let mut last_book = 0;
    let mut previous = None;
    for ply in 0..details.sans.len() {
        let opening = classify(details.sans[..=ply].iter().map(String::as_str));
        if opening != previous {
            last_book = ply + 1;
        }
        previous = opening;
    }
    const SHOWN_PLIES: usize = 26;
    details.sans.len().min(SHOWN_PLIES.max(last_book))
}

/// The moves around the key moment, with the next move marked as a pill.
fn move_snippet(preview: &GameDetails, key: usize) -> impl IntoElement + use<> {
    let start = key.saturating_sub(2);
    let end = (key + 6).min(preview.sans.len());
    let mut row = div()
        .flex()
        .flex_row()
        .flex_wrap()
        .gap_x(px(6.))
        .mt(px(10.))
        .mb(px(12.))
        .font_family(MONO)
        .text_size(px(14.))
        .line_height(px(24.))
        .text_color(rgb(T.fg_2));
    for (index, san) in preview.sans[start..end].iter().enumerate() {
        let ply = start + index;
        let label = if ply.is_multiple_of(2) {
            format!("{}.{san}", ply / 2 + 1)
        } else {
            san.clone()
        };
        let is_key = ply == key;
        row = row.child(
            div()
                .when(is_key, |pill| {
                    pill.px(px(5.))
                        .rounded(px(4.))
                        .bg(rgb(T.accent))
                        .text_color(rgb(T.accent_fg))
                })
                .child(ui::i18n::t(&label).to_owned()),
        );
    }
    row
}

pub fn format_count(count: u64) -> String {
    let digits = count.to_string();
    let mut out = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

/// Lichess-style accuracy from a win-percentage loss (0..100).
pub fn accuracy_from_loss(average_loss: f64) -> f64 {
    (103.1668 * (-0.04354 * average_loss).exp() - 3.1669).clamp(0.0, 100.0)
}

/// Win percentage from a White-relative centipawn score.
fn win_percent(cp: i32) -> f64 {
    50.0 + 50.0 * (2.0 / (1.0 + (-0.00368208 * f64::from(cp)).exp()) - 1.0)
}

fn review_accuracy(evals: &[Option<i32>], white: bool, first_white: bool) -> Option<f64> {
    if evals.iter().any(Option::is_none) {
        return None;
    }
    let losses: Vec<_> = evals
        .windows(2)
        .enumerate()
        .filter(|(ply, _)| (first_white == (ply % 2 == 0)) == white)
        .map(|(_, pair)| {
            let before = win_percent(pair[0].expect("checked above"));
            let after = win_percent(pair[1].expect("checked above"));
            (if white {
                before - after
            } else {
                after - before
            })
            .max(0.0)
        })
        .collect();
    (!losses.is_empty())
        .then(|| accuracy_from_loss(losses.iter().sum::<f64>() / losses.len() as f64))
}

/// A move from the game review that lost a lot of the engine score, as one line of the
/// preview: the move, what it lost, and a longer explanation for the tooltip.
struct ReviewFinding {
    ply: usize,
    /// The move in numbered notation, such as `14.c4` or `14...Nf6`.
    label: String,
    /// What was lost, such as `−1.16` or `Mate`.
    loss: String,
    text: String,
}

fn review_findings(game: &GameDetails) -> Vec<ReviewFinding> {
    let mut findings = Vec::new();
    for ply in 0..game.sans.len() {
        let Some(before) = game
            .review_positions
            .iter()
            .find(|position| position.ply as usize == ply)
        else {
            continue;
        };
        let Some(after) = game
            .review_positions
            .iter()
            .find(|position| position.ply as usize == ply + 1)
        else {
            continue;
        };
        let Ok(before_tree) = GameTree::from_fen(&before.fen) else {
            continue;
        };
        let Ok(after_tree) = GameTree::from_fen(&after.fen) else {
            continue;
        };
        let before_pos = &before_tree.node(GameTree::ROOT).pos;
        let after_pos = &after_tree.node(GameTree::ROOT).pos;
        let white_moved = before_pos.turn().is_white();
        let loss = if white_moved {
            before.eval_cp - after.eval_cp
        } else {
            after.eval_cp - before.eval_cp
        };
        let mate_lost = before.mate_distance.is_some_and(|mate| {
            let favorable_to_mover = (white_moved && mate > 0) || (!white_moved && mate < 0);
            favorable_to_mover
                && after
                    .mate_distance
                    .is_none_or(|later| later.signum() != mate.signum() || later.abs() > mate.abs())
        });
        if loss < 75 && !mate_lost {
            continue;
        }
        let fullmove = before_pos.fullmoves().get();
        let label = if white_moved {
            format!("{fullmove}.{}", game.sans[ply])
        } else {
            format!("{fullmove}...{}", game.sans[ply])
        };
        let loss_label = if mate_lost {
            ui::i18n::t("Mate lost").to_owned()
        } else {
            format!("−{:.2}", f64::from(loss) / 100.0)
        };
        let mut text = if mate_lost {
            format!(
                "Ply {} {} gives up a forced mate in {} for {}.",
                ply + 1,
                game.sans[ply],
                before.mate_distance.unwrap_or_default().abs(),
                if white_moved {
                    ui::i18n::t("White")
                } else {
                    ui::i18n::t("Black")
                },
            )
        } else {
            format!(
                "Ply {} {}: engine score lost {:.2} pawns for {}.",
                ply + 1,
                game.sans[ply],
                f64::from(loss) / 100.0,
                if white_moved {
                    ui::i18n::t("White")
                } else {
                    ui::i18n::t("Black")
                },
            )
        };
        let material_before = chess_core::material_balance(before_pos.board()).score;
        let material_after = chess_core::material_balance(after_pos.board()).score;
        let material_loss = if white_moved {
            material_before - material_after
        } else {
            material_after - material_before
        };
        if material_loss >= 1 {
            text.push_str(&format!(
                " Board material immediately drops by {material_loss} points."
            ));
        }
        let pv: Vec<String> = before
            .pv
            .split_ascii_whitespace()
            .map(str::to_owned)
            .collect();
        let sans = chess_core::pv_to_san(before_pos, &pv);
        if !sans.is_empty() {
            text.push_str(&format!(" Engine line: {}.", sans.join(" ")));
        }
        text.push_str(" Engine estimates can change with deeper searches.");
        findings.push(ReviewFinding {
            ply,
            label,
            loss: loss_label,
            text,
        });
    }
    findings
}

/// One review finding in the preview: the move on the left and the score it lost on the
/// right, on a single line. Hovering shows the engine's explanation, and clicking or
/// pressing Enter opens the move in Analyse.
#[derive(IntoElement)]
struct FindingRow {
    finding: ReviewFinding,
    on_click: ClickHandler,
}

/// A click handler that a row shares between mouse and keyboard.
type ClickHandler = std::rc::Rc<dyn Fn(&mut Window, &mut gpui::App)>;

impl RenderOnce for FindingRow {
    fn render(self, window: &mut Window, cx: &mut gpui::App) -> impl IntoElement {
        let id = gpui::ElementId::from(("review-finding", self.finding.ply));
        let focus = window
            .use_keyed_state(id.clone(), cx, |_, cx| cx.focus_handle().tab_stop(true))
            .read(cx)
            .clone();
        let on_key = self.on_click.clone();
        let on_click = self.on_click;
        let text = SharedString::from(format!(
            "{} {}",
            self.finding.text,
            ui::i18n::t("Click to open it in Analyse.")
        ));
        h_flex()
            .id(id)
            .track_focus(&focus)
            .h(px(28.))
            .px(px(6.))
            .gap(px(8.))
            .justify_between()
            .border_b_1()
            .border_color(rgb(T.divider))
            .cursor_pointer()
            .hover(|style| style.bg(rgb(T.hover)))
            .focus_visible(|style| style.border_2().border_color(rgb(T.accent)))
            .text_size(px(13.))
            .whitespace_nowrap()
            .on_click(move |_, window, cx| on_click(window, cx))
            .on_key_down(move |event, window, cx| {
                let key = event.keystroke.key.as_str();
                if (key == "enter" || key == "space") && !event.keystroke.modifiers.modified() {
                    on_key(window, cx);
                    cx.stop_propagation();
                }
            })
            .tooltip(move |window, cx| {
                gpui_component::tooltip::Tooltip::new(text.clone()).build(window, cx)
            })
            .child(
                mono(self.finding.label)
                    .min_w_0()
                    .overflow_hidden()
                    .text_ellipsis()
                    .text_color(rgb(T.fg)),
            )
            .child(mono(self.finding.loss).flex_none().text_color(rgb(T.warn)))
    }
}

/// The most engines a batch review may run at once: one per logical CPU.
fn max_review_engines() -> u32 {
    std::thread::available_parallelism().map_or(4, |n| n.get() as u32)
}

/// Runs engine review for each game on a worker thread and stores the accuracy.
fn review_games(
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
        let Ok(start_tree) = chess_core::GameTree::from_fen(start_fen) else {
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
                                if update.depth >= depth {
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
            moves.push(chess_core::shakmaty::uci::UciMove::from_standard(mv).to_string());
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

impl Render for DatabaseScreen {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // The table needs its fixed columns, so the preview makes way on narrow windows.
        let show_preview = window.viewport_size().width >= px(1320.);
        let left = self.render_left(cx);
        let header = self.render_main_header(cx);
        let stats = self.render_stats();
        let table = self.render_table(cx);
        let preview = self.render_preview(window.scale_factor(), cx);
        let import_bar = self.render_import_bar(cx);
        h_flex().items_start().size_full().child(left).child(
            v_flex()
                .flex_1()
                .min_w_0()
                .h_full()
                .px(px(24.))
                .pt(px(24.))
                .pb(px(16.))
                .gap(px(16.))
                .child(header)
                .child(self.render_review_controls(cx))
                .children(import_bar)
                .child(stats)
                .child(
                    h_flex()
                        .items_start()
                        .flex_1()
                        .min_h_0()
                        .gap(px(16.))
                        .child(div().flex_1().min_w_0().h_full().child(table))
                        .when(show_preview, |row| {
                            row.child(panel().w(px(293.)).flex_none().child(preview))
                        }),
                ),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eco_ranges() {
        assert_eq!(parse_eco("b90–b99"), ("B90".to_owned(), "B99".to_owned()));
        assert_eq!(parse_eco("C50"), ("C50".to_owned(), "C50".to_owned()));
        assert_eq!(parse_eco("  "), (String::new(), String::new()));
    }

    #[test]
    fn counts_and_results_format() {
        assert_eq!(format_count(3214), "3,214");
        assert_eq!(format_count(1_000_000), "1,000,000");
        assert_eq!(format_count(12), "12");
        assert_eq!(result_text("1/2-1/2"), "½–½");
        assert_eq!(result_text("1-0"), "1–0");
    }

    #[test]
    fn leak_context_includes_preceding_moves() {
        let sans: Vec<String> = "e4 c5 Nf3 d6 d4 cxd4 Nxd4 Nf6 Nc3 a6 Be3 e5 Nb3 Be6 f3 Be7 Qd2 O-O O-O-O Nbd7 g4 b5 g5 b4 Ne2 Ne8 h4"
            .split(' ')
            .map(str::to_owned)
            .collect();
        assert_eq!(leak_context("14.h4", &sans), "13.Ne2 Ne8 14.h4?!");
    }

    #[test]
    fn accuracy_is_bounded_and_monotonic() {
        assert!((accuracy_from_loss(0.0) - 100.0).abs() < 0.01);
        assert!(accuracy_from_loss(5.0) > accuracy_from_loss(10.0));
        assert!(accuracy_from_loss(1000.0) >= 0.0);
        assert!((win_percent(0) - 50.0).abs() < 0.01);
        assert!(win_percent(500) > 80.0);
    }
}
