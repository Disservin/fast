//! Database: database list + filter form, virtualized games table, preview panel.

use std::{collections::HashMap, ops::Range, path::PathBuf};

use chess_core::{Chess, GameTree, Position, classify, position_hash};
use db::{
    Database, Filter, FilterStats, GameDetails, GameRow, ImportControl, ImportProgress, SideFilter,
    TimeClass, import_games,
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
    widgets::{btn, check, h_flex, mono, panel, section_label, v_flex},
};

use crate::{
    controls::{TextSelect, field, new_input, new_select, select_el, text_input},
    model::{AppModel, DbEntry, ModelEvent, pick_path},
    shell::Workspace,
};

const COLOURS: [&str; 3] = ["Any", "White", "Black"];
const TIME_CONTROLS: [&str; 5] = ["Any", "Rapid + Classical", "Classical", "Rapid", "Blitz"];
/// Rows fetched per page as the table scrolls.
const PAGE: usize = 200;
const ROW_H: f32 = 37.5;

// Column widths for the games table.
const W_WHITE: f32 = 94.;
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
    done: usize,
    total: usize,
    cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
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
                .map(|text| SharedString::from(*text))
                .collect(),
            usize::from(demo),
            window,
            cx,
        );
        let eco = new_input(if demo { "B90–B99" } else { "" }, "A00–E99", window, cx);
        let opp_rating = new_input(if demo { "1900" } else { "" }, "Any", window, cx);
        let time_control = new_select(
            TIME_CONTROLS
                .iter()
                .map(|text| SharedString::from(*text))
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
                    Err(err) => this.toast(format!("Query failed: {err}"), cx),
                }
                cx.notify();
            });
        }));
    }

    fn toast(&self, text: impl Into<SharedString>, cx: &mut Context<Self>) {
        let text = text.into();
        self.model.update(cx, |model, cx| model.toast(text, cx));
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
                            this.toast(text, cx);
                        });
                    })
                    .detach();
                });
            },
        );
    }

    /// Batch engine review: scores each filtered game and stores its accuracy.
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
            .find(|engine| engine.enabled && engine.use_review)
            .cloned()
        else {
            self.toast(
                "Choose an engine for batch review in Settings › Engines.",
                cx,
            );
            return;
        };
        if model.demo {
            self.toast(
                "Batch review needs a real engine; demo mode only has scripted lines.",
                cx,
            );
            return;
        }
        let Some(path) = self.selected_db.clone() else {
            return;
        };
        let ids = self.ids.clone();
        let player = self.applied.player.trim().to_owned();
        let cancelled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        self.reviewing = Some(Reviewing {
            done: 0,
            total: ids.len(),
            cancelled: cancelled.clone(),
        });
        let (tx, rx) = smol::channel::bounded(1);
        let worker = cx.background_spawn(async move {
            review_games(&engine, &path, &ids, &player, &cancelled, |done| {
                let _ = tx.try_send(done);
            })
        });
        cx.spawn(async move |this, cx| {
            while let Ok(done) = rx.recv().await {
                let _ = this.update(cx, |this, cx| {
                    if let Some(reviewing) = &mut this.reviewing {
                        reviewing.done = done;
                    }
                    cx.notify();
                });
            }
            let result = worker.await;
            let _ = this.update(cx, |this, cx| {
                this.reviewing = None;
                if let Err(err) = result {
                    this.toast(format!("Review stopped: {err:#}"), cx);
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
                    .child("No databases yet. Use Import to add a PGN."),
            );
        }
        let reached = self.reached;
        let only_left = self.only_left_rep;
        let count = self.pending_count.unwrap_or(self.stats.games);
        let reached_this = this.clone();
        let only_this = this.clone();
        let show_this = this.clone();
        v_flex()
            .w(px(281.))
            .h_full()
            .flex_none()
            .px(px(18.))
            .pt(px(16.))
            .bg(rgb(T.surface))
            .border_r_1()
            .border_color(rgb(T.border))
            .child(section_label("Databases").mb(px(5.)))
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
            Some(reviewing) => format!("Stop review {}/{}", reviewing.done, reviewing.total).into(),
            None => "Analyse all".into(),
        };
        let analyse_this = this.clone();
        h_flex()
            .justify_between()
            .h(px(36.))
            .child(
                h_flex()
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
                    .child(btn("analyse-all", analyse_label).on_click(move |_, cx| {
                        analyse_this.update(cx, |screen, cx| screen.analyse_all(cx));
                    }))
                    .child(btn("export", "Export PGN").on_click(move |_, cx| {
                        this.update(cx, |screen, cx| screen.export_pgn(cx));
                    })),
            )
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
                "Score",
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
            .child(div().w(px(W_WHITE)).child(section_label("White")))
            .child(div().flex_1().min_w_0().child(section_label("Black")))
            .child(div().w(px(W_RESULT)).child(section_label("Result")))
            .child(div().w(px(W_LEFT)).child(section_label("Left rep.")))
            .child(div().w(px(W_MOVES)).child(section_label("Moves")))
            .child(div().w(px(W_DATE)).child(section_label("Date")))
            .child(div().w(px(W_SOURCE)).child(section_label("Source")))
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
                .gap(px(5.))
                .overflow_hidden()
                .child(
                    div()
                        .text_color(rgb(T.fg))
                        .when(winner, |text| text.font_weight(FontWeight::BOLD))
                        .child(SharedString::from(name.to_owned())),
                )
                .children(elo.map(|elo| {
                    mono(elo.to_string())
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
            .child(
                name(&game.white, game.white_elo, white_won)
                    .w(px(W_WHITE))
                    .flex_none(),
            )
            .child(
                name(&game.black, game.black_elo, black_won)
                    .flex_1()
                    .min_w_0(),
            )
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
        column = column.child(section_label("Preview").mb(px(9.)));
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
                .child(
                    btn("cancel-import", "Cancel")
                        .h(30.)
                        .on_click(move |_, cx| {
                            this.update(cx, |screen, _| {
                                if let Some(importing) = &screen.importing {
                                    importing.control.cancel();
                                }
                            });
                        }),
                ),
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
                .child(label),
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

/// Runs engine review for each game on a worker thread and stores the accuracy.
pub fn review_games(
    engine: &engine::EngineConfig,
    path: &std::path::Path,
    ids: &[i64],
    player: &str,
    cancelled: &std::sync::atomic::AtomicBool,
    mut on_progress: impl FnMut(usize),
) -> anyhow::Result<()> {
    use engine::{EngineEvent, EngineHandle, Limits, Score, SearchRequest};
    use std::sync::atomic::Ordering;
    const MS_PER_MOVE: u64 = 40;
    const START: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

    let database = Database::open(path)?;
    let options = engine
        .options
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let handle = EngineHandle::spawn(engine.path.clone(), Vec::new(), options);
    let events = handle.events();
    let mut search_id = 0;
    for (done, &id) in ids.iter().enumerate() {
        if cancelled.load(Ordering::Relaxed) {
            break;
        }
        let Some(game) = database.game(id)? else {
            continue;
        };
        let mut moves: Vec<String> = Vec::new();
        let mut pos = Chess::default();
        let mut evals: Vec<i32> = Vec::with_capacity(game.sans.len() + 1);
        for san_index in 0..=game.sans.len() {
            if cancelled.load(Ordering::Relaxed) {
                break;
            }
            search_id += 1;
            handle.search(SearchRequest {
                fen: START.to_owned(),
                moves: moves.clone(),
                limits: Some(Limits {
                    movetime_ms: Some(MS_PER_MOVE),
                    ..Default::default()
                }),
                search_id,
                game: id as u64,
            });
            let mut eval = 0;
            loop {
                match smol::block_on(events.recv()) {
                    Ok(EngineEvent::Analysis {
                        search_id: from,
                        update,
                    }) if from == search_id => {
                        if let Some(score) = update.score {
                            let white = if pos.turn().is_white() {
                                score
                            } else {
                                score.negate()
                            };
                            eval = match white {
                                Score::Cp(cp) => cp,
                                Score::Mate(mate) if mate > 0 => 3000,
                                Score::Mate(_) => -3000,
                            };
                        }
                    }
                    Ok(EngineEvent::BestMove {
                        search_id: from, ..
                    }) if from == search_id => break,
                    Ok(EngineEvent::Crashed(reason)) => anyhow::bail!(reason),
                    Ok(_) => {}
                    Err(_) => anyhow::bail!("engine channel closed"),
                }
            }
            evals.push(eval);
            let Some(san) = game.sans.get(san_index) else {
                break;
            };
            let parsed: chess_core::shakmaty::san::San = san.parse()?;
            let mv = parsed.to_move(&pos)?;
            moves.push(chess_core::shakmaty::uci::UciMove::from_standard(mv).to_string());
            pos.play_unchecked(mv);
        }
        let reviewed_white = player.is_empty() || game.row.white.eq_ignore_ascii_case(player);
        let losses: Vec<f64> = evals
            .windows(2)
            .enumerate()
            .filter(|(ply, _)| (ply % 2 == 0) == reviewed_white)
            .map(|(_, pair)| {
                let (before, after) = (win_percent(pair[0]), win_percent(pair[1]));
                if reviewed_white {
                    before - after
                } else {
                    after - before
                }
                .max(0.0)
            })
            .collect();
        if !losses.is_empty() {
            let average = losses.iter().sum::<f64>() / losses.len() as f64;
            database.set_accuracy(id, accuracy_from_loss(average))?;
        }
        on_progress(done + 1);
    }
    handle.quit();
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
