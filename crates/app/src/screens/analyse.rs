//! Analyse: scoresheet, board with timeline, and the merged "Moves from here" table.

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
};

use chess_core::{
    GameTree, Move, NodeId, Position, classify, display_san, parse_pgn, position_hash, write_pgn,
    write_pgn_from,
};
use db::{Database, ImportControl, ImportOptions, MoveStat, import_games};
use engine::{EngineId, LogLine, Quality, Score, Wdl};
use gpui::{
    AppContext as _, ClipboardItem, Context, Entity, FocusHandle, FontWeight, IntoElement,
    MouseButton, Pixels, Point, Render, ScrollHandle, SharedString, Subscription, Task, TextLayout,
    UniformListScrollHandle, WeakEntity, Window, anchored, deferred, div, prelude::*, px, rgb,
};
use gpui_component::{
    input::{InputEvent, InputState, TextareaState},
    scroll::ScrollableElement as _,
};
use ui::{
    Arrow, BoardEvent, BoardView, MONO, T,
    scoresheet::{ScoresheetProps, scoresheet_body, scoresheet_inline},
    timeline::{TimelineProps, timeline},
    widgets::{btn, h_flex, icon_btn, mono, panel, section_label, v_flex},
};

use crate::{
    config::{GraphTab, LineSort, ScoresheetStyle},
    live::{LiveAnalysis, LiveEvent},
    model::{AppModel, DbEntry, ModelEvent, ToastExt as _},
    shell::Workspace,
};

/// Each position is shown at least this long, even when a search ends at once.
const BACKWARD_DWELL: std::time::Duration = std::time::Duration::from_millis(250);

#[derive(Clone, Copy, PartialEq, Eq)]
enum AnalysisPanel {
    Scoresheet,
    Board,
}

#[derive(Clone)]
struct ResizeAnalysisPanel(AnalysisPanel);

impl Render for ResizeAnalysisPanel {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size(px(1.))
    }
}

fn panel_resize_handle(id: &'static str, panel: AnalysisPanel) -> impl IntoElement {
    gpui_component::resizable::resize_handle(id, gpui::Axis::Horizontal)
        .inside(gpui_component::resizable::HandleEdge::Trailing)
        .on_drag(ResizeAnalysisPanel(panel), |drag, _, _, cx| {
            cx.new(|_| drag.clone())
        })
}

/// A game being analysed from its last move to its first, one search per position,
/// so the engine meets each position knowing what follows it.
struct BackwardRun {
    /// The node being searched; moving elsewhere ends the run.
    node: NodeId,
    started: std::time::Instant,
    _poll: Task<()>,
}

/// One candidate move: an engine's line for it, or a move only databases, online
/// sources or the repertoire know. With several engines a move can have a row each.
#[derive(Clone, Debug, Default)]
pub struct MoveRow {
    pub san: String,
    /// The score of this row's engine line.
    pub score: Option<Score>,
    pub wdl: Option<Wdl>,
    pub pv: Vec<String>,
    pub depth: u32,
    /// The engine whose line this row is.
    pub engine_id: Option<EngineId>,
    pub reference: Option<MoveStat>,
    pub mine: Option<MoveStat>,
    /// The tablebase's verdict on this move, while Online is on.
    pub tb: Option<crate::online::TbRow>,
    /// chessdb.cn's score for this move, while Online is on.
    pub cloud: Option<crate::online::CloudMove>,
    pub rep: RepTag,
    /// How this line compares with the best one, for its arrow and label colour.
    pub quality: Option<Quality>,
}

/// Which table row is picked: the move, and the engine whose line it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RowKey {
    pub san: String,
    pub engine_id: Option<EngineId>,
}

impl RowKey {
    pub fn of(row: &MoveRow) -> Self {
        Self {
            san: row.san.clone(),
            engine_id: row.engine_id,
        }
    }

    pub fn matches(&self, row: &MoveRow) -> bool {
        row.san == self.san && row.engine_id == self.engine_id
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RepTag {
    Main,
    Side,
    Deviate,
    #[default]
    None,
}

/// Where rows come from and how to order them.
#[derive(Clone, Copy, Debug)]
pub struct RowOptions {
    pub white_to_move: bool,
    pub user_to_move: bool,
    pub sort: LineSort,
}

/// Merges candidate sources into table rows, sorted by engine eval for the side to
/// move (or by depth, deepest first); moves with no eval follow, by popularity, then
/// any remaining legal moves in `legal`, so every candidate is listed.
pub fn merge_rows(
    engine_lines: Vec<MoveRow>,
    reference: Vec<MoveStat>,
    mine: Vec<MoveStat>,
    rep_moves: &[(String, bool)],
    legal: &[String],
    options: RowOptions,
) -> Vec<MoveRow> {
    let RowOptions {
        white_to_move,
        user_to_move,
        sort,
    } = options;
    let mut rows = engine_lines;
    for stat in reference {
        for row in rows_for(&mut rows, &stat.san, true) {
            row.reference = Some(stat.clone());
        }
    }
    for stat in mine {
        for row in rows_for(&mut rows, &stat.san, true) {
            row.mine = Some(stat.clone());
        }
    }
    for (san, main) in rep_moves {
        for row in rows_for(&mut rows, san, true) {
            row.rep = if *main { RepTag::Main } else { RepTag::Side };
        }
    }
    for san in legal {
        // Adding the row is the point; there is nothing to set on it.
        let _ = rows_for(&mut rows, san, true);
    }
    // The user played something the repertoire does not cover, when it has an answer.
    if user_to_move && !rep_moves.is_empty() {
        for row in &mut rows {
            if row.rep == RepTag::None && row.mine.is_some() {
                row.rep = RepTag::Deviate;
            }
        }
    }
    let popularity = |row: &MoveRow| {
        row.reference.as_ref().map_or(0, |stat| stat.games)
            + row.mine.as_ref().map_or(0, |stat| stat.games)
    };
    // Without an engine eval, online verdicts order what popularity leaves tied: the
    // tablebase's result first, then the cloud's score, both for the side to move.
    let tb_rank = |row: &MoveRow| row.tb.as_ref().map(|tb| online::outcome_rank(tb.outcome));
    let cloud_score = |row: &MoveRow| row.cloud.as_ref().map(|cloud| cloud.score);
    let key = |row: &MoveRow| {
        row.score
            .map(|score| score.sort_key() * if white_to_move { 1 } else { -1 })
    };
    rows.sort_by(|a, b| {
        let by_eval = match (key(a), key(b)) {
            (Some(x), Some(y)) => y.cmp(&x),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => popularity(b)
                .cmp(&popularity(a))
                .then(tb_rank(b).cmp(&tb_rank(a)))
                .then(cloud_score(b).cmp(&cloud_score(a))),
        };
        match sort {
            LineSort::Eval => by_eval,
            LineSort::Depth => b.depth.cmp(&a.depth).then(by_eval),
        }
    });
    // Grade each line against the best one, from the side to move's view.
    let relative = |score: Score| {
        if white_to_move { score } else { score.negate() }
    };
    let best = rows
        .iter()
        .filter_map(|row| row.score.map(relative))
        .max_by_key(|score| score.sort_key());
    if let Some(best) = best {
        for row in &mut rows {
            row.quality = row
                .score
                .map(|score| engine::classify_move(best, relative(score)));
        }
    }
    rows
}

/// Every row for `san` (one per engine that found it), ignoring check marks. When
/// there is none and `add` is set, a new row for the move is added and returned.
pub(crate) fn rows_for<'a>(
    rows: &'a mut Vec<MoveRow>,
    san: &str,
    add: bool,
) -> impl Iterator<Item = &'a mut MoveRow> {
    fn clean(san: &str) -> &str {
        san.trim_end_matches(['+', '#'])
    }
    if add && !rows.iter().any(|row| clean(&row.san) == clean(san)) {
        rows.push(MoveRow {
            san: san.to_owned(),
            ..Default::default()
        });
    }
    rows.iter_mut()
        .filter(move |row| clean(&row.san) == clean(san))
}

/// The colour for a move's quality.
pub fn quality_color(quality: Quality) -> u32 {
    match quality {
        Quality::Best => T.q_best,
        Quality::Good => T.q_good,
        Quality::Inaccuracy => T.q_inaccuracy,
        Quality::Mistake => T.q_mistake,
        Quality::Blunder => T.q_blunder,
    }
}

mod editor;
mod online;
mod panels;

use editor::PositionEditor;
use panels::{LOG_POLL, LogSelection, PvView};

type MenuHandler = Box<dyn Fn(&mut Window, &mut gpui::App)>;

/// Width of the folded scoresheet.
const SCORESHEET_RAIL: f32 = 44.;
/// Height of the Line preview's move box: four lines of moves. Longer lines scroll.
const PV_INSET_H: f32 = 4. * 26. + 14.;

/// The stored game this board was opened from, so saving writes the changes back to it.
#[derive(Clone, Debug)]
pub struct DbGame {
    pub path: std::path::PathBuf,
    pub id: i64,
    /// The database's name, for messages.
    pub name: SharedString,
}

pub struct AnalyseScreen {
    recovery: Option<crate::recovery::Recovery>,
    recovery_path: std::path::PathBuf,
    /// What the autosave file last received.
    recovery_pgn: String,
    /// Autosave is failing; reported once until it works again.
    autosave_failed: bool,
    model: Entity<AppModel>,
    workspace: WeakEntity<Workspace>,
    /// Set while the board holds a game opened from a database.
    db_game: Option<DbGame>,
    /// The game as last loaded from or saved to its database, to tell whether it has
    /// unsaved changes.
    db_saved_pgn: String,
    /// The stored review's findings for that game, with the move each one is on.
    db_findings: Vec<(NodeId, crate::screens::database::ReviewMark)>,
    pub tree: GameTree,
    pub current: NodeId,
    board: Entity<BoardView>,
    live: Entity<LiveAnalysis>,
    rows: Vec<MoveRow>,
    /// The table row shown in Line preview.
    selected: Option<RowKey>,
    row_chosen: bool,
    reference: Vec<MoveStat>,
    mine: Vec<MoveStat>,
    evals: HashMap<NodeId, i32>,
    /// Search depth behind each eval, for the depth graph.
    depths: HashMap<NodeId, u32>,
    /// White-relative mate distance behind an eval at ±`MATE_CP`, where known.
    mates: HashMap<NodeId, i32>,
    /// The child last visited from each node, so stepping forward returns into the
    /// line you came back from rather than always the main line.
    last_child: HashMap<NodeId, NodeId>,
    note: Entity<TextareaState>,
    note_hash: u64,
    comment: Entity<TextareaState>,
    editing_comment: bool,
    fen_input: Entity<InputState>,
    /// Edit position is open, with the position being set up.
    editor: Option<PositionEditor>,
    context_menu: Option<(NodeId, Point<Pixels>)>,
    /// The board toolbar's copy menu is open.
    copy_menu: bool,
    scoresheet_scroll: ScrollHandle,
    /// Where the current move was drawn in the scoresheet.
    scoresheet_current: Rc<Cell<Option<gpui::Bounds<Pixels>>>>,
    /// The current move changed, so the scoresheet should scroll it into view once.
    scoresheet_follow: Rc<Cell<bool>>,
    db_task: Option<Task<()>>,
    /// The database this tab's opening tree counts, when the user picked one here.
    explorer_db: Option<std::path::PathBuf>,
    /// The database the last opening-tree query used, to notice when it disappears.
    explorer_used: Option<std::path::PathBuf>,
    /// The opening tree's database list is open, with its highlighted entry.
    explorer_menu: Option<usize>,
    explorer_focus: FocusHandle,
    /// Recent opening-tree answers, so stepping through a game does not query again.
    move_cache: crate::explorer::MoveCache,
    visible: bool,
    /// Show WDL when an engine requests or supplies it, regardless of window width.
    show_wdl: bool,
    /// Whether the Engine panel lists every legal move to restrict the search to.
    search_moves_open: bool,
    /// Which tab of settings the Engine panel shows under its engines.
    engine_tab: panels::EngineTab,
    spin_edit: Option<(EngineId, &'static str, Entity<InputState>, Subscription)>,
    log_panel_open: bool,
    /// The Online panel and its tablebase and cloud lookups.
    online: online::OnlineState,
    /// The engine the Engine and Log panels are showing.
    panel_engine: Option<EngineId>,
    log_show_info: bool,
    log_follow: bool,
    log_lines: Vec<LogLine>,
    /// What `log_lines` was taken from: engine, log revision and the Info filter.
    log_key: Option<(EngineId, u64, bool)>,
    log_selection: Option<LogSelection>,
    log_selecting: bool,
    /// Text layouts of the visible log rows, for finding the character under the pointer.
    log_layouts: Rc<RefCell<HashMap<usize, TextLayout>>>,
    /// Redraws the Log panel when its engine writes, since not every line is an event.
    _log_poll: Task<()>,
    log_scroll: UniformListScrollHandle,
    /// The highlighted part of the selected row's line, with the line held still.
    pv_view: Option<PvView>,
    pv_dragging: bool,
    pv_toggle_off: bool,
    pv_played: Option<usize>,
    /// The preview is analysed by the kibitz engine; the main analysis, the table and
    /// everything else stay on the game position.
    pub(super) pv_kibitz: bool,
    pub(super) kibitz: Entity<crate::kibitz::Kibitz>,
    pv_hover: Option<usize>,
    /// The small board that follows the cursor over a hovered PV move.
    pv_peek: Entity<BoardView>,
    pv_peek_at: Option<Point<Pixels>>,
    pv_focus: FocusHandle,
    backward: Option<BackwardRun>,
    /// The window's scale factor at the last render, for snapping fixed-size boards.
    scale: f32,
    _subscriptions: Vec<Subscription>,
}

impl AnalyseScreen {
    pub fn new(
        model: Entity<AppModel>,
        workspace: WeakEntity<Workspace>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let path = model.read(cx).paths.data_dir.join("game-recovery.json");
        Self::new_with_recovery(model, workspace, window, cx, path)
    }

    pub fn new_with_recovery(
        model: Entity<AppModel>,
        workspace: WeakEntity<Workspace>,
        window: &mut Window,
        cx: &mut Context<Self>,
        recovery_path: std::path::PathBuf,
    ) -> Self {
        let board = cx.new(|_| BoardView::new("analyse-board"));
        let live = cx.new(|cx| LiveAnalysis::new(model.clone(), cx));
        let kibitz = cx.new(|_| crate::kibitz::Kibitz::default());
        let note = cx.new(|cx| {
            TextareaState::new(window, cx).placeholder(ui::i18n::t("Add a note for this position"))
        });
        let comment = cx.new(|cx| TextareaState::new(window, cx));
        let fen_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder(ui::i18n::t("Paste a FEN and press Enter"))
        });
        let subscriptions = vec![
            cx.subscribe_in(&board, window, |this, _, event, window, cx| match event {
                BoardEvent::Moved(mv) => this.play_move(*mv, window, cx),
                BoardEvent::Premove(_) => {}
                BoardEvent::Square { square, erase } => {
                    let (square, erase) = (*square, *erase);
                    this.edit_position(cx, |editor| editor.paint(square, erase));
                }
            }),
            cx.subscribe_in(&live, window, |this, _, event, _, cx| match event {
                LiveEvent::Updated => this.rebuild_rows(cx),
                LiveEvent::Eval(node, score, depth) => {
                    // Keep the deeper of what is known: a game's annotations or the engine.
                    if this.depths.get(node).is_none_or(|known| *known <= *depth)
                        || !this.evals.contains_key(node)
                    {
                        this.evals.insert(*node, crate::live::score_cp(*score));
                        this.depths.insert(*node, *depth);
                        match score {
                            Score::Mate(mate) => this.mates.insert(*node, *mate),
                            Score::Cp(_) => this.mates.remove(node),
                        };
                    }
                    cx.notify();
                }
            }),
            cx.subscribe_in(&note, window, |this, state, event, _, cx| {
                if matches!(event, InputEvent::Change) {
                    let text = state.read(cx).value().to_string();
                    let hash = this.note_hash;
                    this.model.read(cx).set_note(hash, &text);
                }
            }),
            cx.observe(&kibitz, |this, _, cx| {
                if this.pv_kibitz {
                    this.update_arrows(cx);
                    cx.notify();
                }
            }),
            cx.subscribe_in(&fen_input, window, |this, state, event, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    let fen = state.read(cx).value().to_string();
                    this.load_fen(fen.trim(), window, cx);
                }
            }),
            cx.subscribe_in(&model, window, |this, model, event, _, cx| match event {
                ModelEvent::BoardPrefs => {
                    let prefs = model.read(cx).config.board.prefs();
                    this.board
                        .update(cx, |board, cx| board.set_prefs(prefs, cx));
                }
                ModelEvent::Databases => this.databases_changed(cx),
                ModelEvent::Repertoire => this.query_databases(cx),
                ModelEvent::Engines | ModelEvent::Restored => {}
            }),
        ];
        let demo = model.read(cx).demo;
        let tree = if demo {
            chess_core::sample::najdorf()
        } else {
            GameTree::new()
        };
        let mut evals = HashMap::new();
        let current = if demo {
            let main = tree.mainline();
            for (id, cp) in main.iter().zip(crate::demo::TIMELINE) {
                evals.insert(*id, cp);
            }
            main.last().copied().unwrap_or(GameTree::ROOT)
        } else {
            GameTree::ROOT
        };
        let prefs = model.read(cx).config.board.prefs();
        let flipped = model.read(cx).config.board.flipped;
        board.update(cx, |board, cx| {
            board.set_prefs(prefs, cx);
            board.set_flipped(flipped, cx);
        });
        let mut this = Self {
            recovery: crate::recovery::Recovery::load(&recovery_path),
            recovery_path,
            recovery_pgn: String::new(),
            autosave_failed: false,
            model,
            workspace,
            db_game: None,
            db_saved_pgn: String::new(),
            db_findings: Vec::new(),
            tree,
            current,
            board,
            live,
            rows: Vec::new(),
            depths: HashMap::new(),
            mates: HashMap::new(),
            last_child: HashMap::new(),
            selected: None,
            row_chosen: false,
            reference: Vec::new(),
            mine: Vec::new(),
            evals,
            note,
            note_hash: 0,
            comment,
            editing_comment: false,
            fen_input,
            editor: None,
            context_menu: None,
            copy_menu: false,
            scoresheet_scroll: ScrollHandle::new(),
            scoresheet_current: Rc::default(),
            scoresheet_follow: Rc::new(Cell::new(true)),
            db_task: None,
            explorer_db: None,
            explorer_used: None,
            explorer_menu: None,
            explorer_focus: cx.focus_handle().tab_stop(true),
            move_cache: crate::explorer::MoveCache::default(),
            visible: false,
            show_wdl: true,
            search_moves_open: false,
            engine_tab: panels::EngineTab::Engines,
            spin_edit: None,
            log_panel_open: false,
            online: online::OnlineState::default(),
            panel_engine: None,
            log_show_info: true,
            log_follow: true,
            log_lines: Vec::new(),
            log_key: None,
            log_selection: None,
            log_selecting: false,
            log_layouts: Rc::default(),
            _log_poll: cx.spawn(async move |this: WeakEntity<Self>, cx| {
                loop {
                    cx.background_executor().timer(LOG_POLL).await;
                    let alive = this.update(cx, |this, cx| {
                        if this.log_panel_open && this.log_key != this.current_log_key(cx) {
                            cx.notify();
                        }
                    });
                    if alive.is_err() {
                        return;
                    }
                }
            }),
            log_scroll: UniformListScrollHandle::new(),
            pv_view: None,
            pv_dragging: false,
            pv_toggle_off: false,
            pv_played: None,
            pv_kibitz: false,
            kibitz,
            pv_hover: None,
            pv_peek: cx.new(|_| BoardView::new("pv-peek-board").read_only()),
            pv_peek_at: None,
            pv_focus: cx.focus_handle(),
            scale: window.scale_factor(),
            backward: None,
            _subscriptions: subscriptions,
        };
        this.position_changed(false, window, cx);
        this
    }

    pub fn breadcrumb(&self) -> SharedString {
        if self.db_game.is_some() {
            let player = |side: &'static str| {
                self.tree
                    .header(side)
                    .filter(|name| !name.is_empty() && *name != "?")
                    .unwrap_or(side)
                    .to_owned()
            };
            return format!("{} – {}", player("White"), player("Black")).into();
        }
        match self.opening() {
            Some(opening) => opening
                .name
                .strip_prefix("Sicilian Najdorf, ")
                .map(|variation| format!("Najdorf — {variation}"))
                .unwrap_or_else(|| opening.name.to_owned())
                .into(),
            None => ui::i18n::t("New game").into(),
        }
    }

    pub fn live_on(&self, cx: &gpui::App) -> bool {
        self.live.read(cx).on
    }

    /// The engines are searching this tab's position.
    pub fn live_searching(&self, cx: &gpui::App) -> bool {
        self.live.read(cx).searching()
    }

    /// The engines are on but their search is paused, by hand or on leaving the tab.
    pub fn live_paused(&self, cx: &gpui::App) -> bool {
        let live = self.live.read(cx);
        live.on && live.paused
    }

    pub fn set_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        self.visible = visible;
        let pause = !visible && self.model.read(cx).config.pause_in_background;
        self.live.update(cx, |live, cx| {
            if pause && live.on {
                live.set_paused(true, cx);
            }
            live.set_active(visible, cx)
        });
        if !visible {
            self.stop_backward(None, cx);
            self.kibitz.update(cx, |kibitz, cx| kibitz.stop(cx));
        } else if self.pv_kibitz {
            self.pv_played = None;
            self.show_pv_preview(cx);
        }
    }

    /// The file this tab keeps its game in, so the tab can be reopened on the next start.
    pub fn recovery_path(&self) -> &std::path::Path {
        &self.recovery_path
    }

    pub fn checkpoint(&self) -> anyhow::Result<()> {
        let pgn = write_pgn(&self.tree);
        if pgn == write_pgn(&GameTree::new()) {
            crate::recovery::Recovery::discard(&self.recovery_path)
        } else {
            crate::recovery::Recovery::save(&self.recovery_path, pgn)
        }
    }

    /// Keeps the open game on disk after every change, so it is never lost and comes
    /// back on the next start. An empty board leaves nothing to restore.
    fn autosave(&mut self, cx: &mut Context<Self>) {
        let pgn = write_pgn(&self.tree);
        if pgn == self.recovery_pgn {
            return;
        }
        match self.checkpoint() {
            Ok(()) => {
                self.recovery_pgn = pgn;
                self.autosave_failed = false;
            }
            Err(err) => {
                log::error!("autosave failed: {err:#}");
                if !std::mem::replace(&mut self.autosave_failed, true) {
                    self.model.toast(
                        format!("Could not autosave this game: {err}. Check storage permissions."),
                        cx,
                    );
                }
            }
        }
    }

    fn opening(&self) -> Option<chess_core::Opening> {
        let path = self.tree.path_to(self.current);
        let line = if path.is_empty() {
            self.tree.mainline()
        } else {
            self.tree.line_through(self.current)
        };
        classify(line.iter().map(|&id| self.tree.node(id).san.as_str()))
    }

    /// SAN path from the root, for repertoire lookups.
    fn base_san_path(&self) -> Vec<String> {
        self.tree
            .path_to(self.current)
            .iter()
            .map(|&id| self.tree.node(id).san.clone())
            .collect()
    }

    fn position_changed(&mut self, animate: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.close_editor(cx);
        self.pv_view = None;
        self.pv_dragging = false;
        self.pv_hover = None;
        self.pv_peek_at = None;
        self.pv_played = None;
        if std::mem::take(&mut self.pv_kibitz) {
            self.kibitz.update(cx, |kibitz, cx| kibitz.stop(cx));
        }
        self.board
            .update(cx, |board, cx| board.set_interactive(true, cx));
        let node = self.tree.node(self.current);
        let pos = node.pos.clone();
        let last = node.mv.as_ref().and_then(|mv| Some((mv.from()?, mv.to())));
        // A move made, or one step forward: the same moments the board animates.
        if animate && let Some(mv) = node.mv {
            self.model.read(cx).config.board.play_move_sound(
                &mv,
                &pos,
                self.tree.game_end(self.current),
            );
        }
        self.board
            .update(cx, |board, cx| board.set_position(pos, last, animate, cx));
        self.selected = None;
        self.row_chosen = false;
        self.board
            .update(cx, |board, cx| board.set_arrows(Vec::new(), cx));
        let tree = self.tree.clone();
        let current = self.current;
        self.live
            .update(cx, |live, cx| live.set_position(&tree, current, cx));
        self.note_hash = position_hash(&self.tree.node(self.current).pos);
        let text = self.model.read(cx).note(self.note_hash);
        self.note
            .update(cx, |note, cx| note.set_value(text, window, cx));
        self.query_databases(cx);
        self.scoresheet_follow.set(true);
        cx.notify();
    }

    /// The database the Reference column counts: this tab's choice, the open game's
    /// database, the last database picked anywhere, then the first reference database.
    pub fn explorer_database(&self, cx: &gpui::App) -> Option<DbEntry> {
        let model = self.model.read(cx);
        let paths: Vec<std::path::PathBuf> = model
            .databases
            .iter()
            .map(|entry| entry.path.clone())
            .collect();
        let pick = crate::explorer::pick_database(
            self.explorer_db.as_deref(),
            self.db_game.as_ref().map(|game| game.path.as_path()),
            model.config.explorer_default.as_deref(),
            model.reference_db().map(|entry| entry.path.as_path()),
            &paths,
        )?;
        model
            .databases
            .iter()
            .find(|entry| entry.path == pick)
            .cloned()
    }

    /// The databases the opening tree can use, the open game's database first.
    fn explorer_choices(&self, cx: &gpui::App) -> Vec<DbEntry> {
        let mut entries = self.model.read(cx).databases.clone();
        if let Some(game) = &self.db_game
            && let Some(index) = entries.iter().position(|entry| entry.path == game.path)
        {
            let entry = entries.remove(index);
            entries.insert(0, entry);
        }
        entries
    }

    /// The database this tab chose for its opening tree, for saving the session.
    pub fn explorer_choice(&self) -> Option<&std::path::Path> {
        self.explorer_db.as_deref()
    }

    /// Restores a saved choice for the opening tree, ignoring a database that no
    /// longer exists.
    pub fn restore_explorer_choice(
        &mut self,
        path: Option<std::path::PathBuf>,
        cx: &mut Context<Self>,
    ) {
        self.explorer_db = path.filter(|path| {
            self.model
                .read(cx)
                .databases
                .iter()
                .any(|entry| &entry.path == path)
        });
        self.query_databases(cx);
    }

    /// Makes the opening tree count `path`, remembering it for this tab and as the
    /// default for tabs that have not chosen.
    fn choose_explorer_db(&mut self, path: std::path::PathBuf, cx: &mut Context<Self>) {
        self.explorer_menu = None;
        self.explorer_db = Some(path.clone());
        self.model.update(cx, |model, cx| {
            if model.config.explorer_default.as_ref() != Some(&path) {
                model.config.explorer_default = Some(path);
                model.save_config(cx);
            }
        });
        self.query_databases(cx);
        cx.notify();
    }

    /// Steps the opening tree to the next database in the picker's order.
    pub fn next_explorer_db(&mut self, cx: &mut Context<Self>) {
        let choices = self.explorer_choices(cx);
        if choices.is_empty() {
            return;
        }
        let current = self.explorer_database(cx).map(|entry| entry.path);
        let index = choices
            .iter()
            .position(|entry| Some(&entry.path) == current.as_ref())
            .map_or(0, |index| (index + 1) % choices.len());
        let path = choices[index].path.clone();
        self.choose_explorer_db(path, cx);
    }

    /// A database was added, changed or removed: cached counts are dropped, and a
    /// removed opening-tree database falls back with a note saying which one is used.
    fn databases_changed(&mut self, cx: &mut Context<Self>) {
        self.move_cache.clear();
        let known: Vec<std::path::PathBuf> = self
            .model
            .read(cx)
            .databases
            .iter()
            .map(|entry| entry.path.clone())
            .collect();
        if self
            .explorer_db
            .as_ref()
            .is_some_and(|path| !known.contains(path))
        {
            self.explorer_db = None;
        }
        let removed = self
            .explorer_used
            .as_ref()
            .is_some_and(|path| !known.contains(path));
        if removed && let Some(entry) = self.explorer_database(cx) {
            let message = ui::i18n::tf("Opening tree now uses {}", &[&entry.name]);
            self.model.toast(message, cx);
        }
        self.query_databases(cx);
    }

    fn query_databases(&mut self, cx: &mut Context<Self>) {
        let hash = position_hash(&self.analysed_pos());
        let reference = self.explorer_database(cx).map(|entry| entry.path);
        self.explorer_used = reference.clone();
        let model = self.model.read(cx);
        let mine = model.my_games().map(|entry| entry.path.clone());
        // All the user's names, one per line, as the cache key and the query's names.
        let player = model.config.my_names().join("\n");
        // Answers already known come from the cache; only the rest are queried.
        let cached = |path: &Option<std::path::PathBuf>, player: &str| match path {
            None => Some(Vec::new()),
            Some(path) => self.move_cache.get(path, hash, player).cloned(),
        };
        let (cached_reference, cached_mine) = (cached(&reference, ""), cached(&mine, &player));
        if let (Some(reference), Some(mine)) = (&cached_reference, &cached_mine) {
            self.db_task = None;
            self.reference = reference.clone();
            self.mine = mine.clone();
            self.rebuild_rows(cx);
            return;
        }
        let (query_reference, query_mine) = (
            reference.clone().filter(|_| cached_reference.is_none()),
            mine.clone().filter(|_| cached_mine.is_none()),
        );
        let query_player = player.clone();
        let task = cx.background_spawn(async move {
            let query = |path: Option<std::path::PathBuf>, player: &str| {
                let path = path?;
                Some(
                    Database::open(path)
                        .and_then(|db| {
                            let names: Vec<String> = player
                                .split('\n')
                                .filter(|name| !name.is_empty())
                                .map(str::to_owned)
                                .collect();
                            if names.is_empty() {
                                db.moves_from(hash)
                            } else {
                                db.moves_from_players(hash, &names)
                            }
                        })
                        .unwrap_or_else(|err| {
                            log::warn!("position query failed: {err}");
                            Vec::new()
                        }),
                )
            };
            (query(query_reference, ""), query(query_mine, &query_player))
        });
        // Dropping the previous task cancels a stale query.
        self.db_task = Some(cx.spawn(async move |this, cx| {
            let (found_reference, found_mine) = task.await;
            let _ = this.update(cx, |this, cx| {
                if let (Some(path), Some(moves)) = (&reference, &found_reference) {
                    this.move_cache
                        .insert(path.clone(), hash, String::new(), moves.clone());
                }
                if let (Some(path), Some(moves)) = (&mine, &found_mine) {
                    this.move_cache
                        .insert(path.clone(), hash, player.clone(), moves.clone());
                }
                this.reference = found_reference.or(cached_reference).unwrap_or_default();
                this.mine = found_mine.or(cached_mine).unwrap_or_default();
                this.rebuild_rows(cx);
            });
        }));
    }

    fn rebuild_rows(&mut self, cx: &mut Context<Self>) {
        let white_to_move = self.analysed_pos().turn().is_white();
        let model = self.model.read(cx);
        let live = self.live.read(cx);
        let mut lines: Vec<MoveRow> = Vec::new();
        // A row per engine line, so engines can be compared move by move in one
        // column. The primary engine goes first, so on equal scores its row leads.
        let primary_id = live.primary().map(|engine| engine.config.id);
        let mut engines: Vec<&crate::live::LiveEngine> = live.engines.iter().collect();
        engines.sort_by_key(|engine| Some(engine.config.id) != primary_id);
        for engine in engines {
            let show_wdl = crate::live::wdl_visible(&engine.config, true);
            for line in engine.lines.values() {
                let Some(san) = line.sans.first() else {
                    continue;
                };
                lines.push(MoveRow {
                    san: san.clone(),
                    score: line.score,
                    wdl: line.wdl.filter(|_| show_wdl),
                    pv: line.sans.clone(),
                    depth: line.update.depth,
                    engine_id: Some(engine.config.id),
                    ..Default::default()
                });
            }
        }
        let path = self.analysed_san_path();
        // Repertoires are stored as lines from the initial position.
        let from_start = self.tree.header("FEN").is_none();
        let rep_moves: Vec<(String, bool)> = model
            .repertoire
            .moves_after(&path)
            .filter(|_| from_start && model.config.view.my_moves)
            .map(|moves| {
                moves
                    .iter()
                    .enumerate()
                    .map(|(rank, node)| (node.san.clone(), rank == 0))
                    .collect()
            })
            .unwrap_or_default();
        // Online moves join the rows of the same move rather than listing them twice.
        self.merge_online(&mut lines, model.config.view.all_moves);
        let view = &model.config.view;
        let legal: Vec<String> = if view.all_moves {
            let pos = self.analysed_pos();
            pos.legal_moves()
                .iter()
                .map(|mv| {
                    chess_core::shakmaty::san::SanPlus::from_move(pos.clone(), *mv).to_string()
                })
                .collect()
        } else {
            Vec::new()
        };
        self.rows = merge_rows(
            lines,
            self.reference.clone(),
            // With My moves off, the user's own games add no rows and no figures.
            if view.my_moves {
                self.mine.clone()
            } else {
                Vec::new()
            },
            &rep_moves,
            &legal,
            RowOptions {
                white_to_move,
                user_to_move: white_to_move == model.repertoire.white,
                sort: view.sort,
            },
        );
        // The selection follows the best line until the user picks a row.
        // A kibitz preview holds the row whose line is being looked at.
        if !self.row_chosen
            && !self.pv_kibitz
            && let Some(first) = self.rows.iter().find(|row| !row.pv.is_empty())
        {
            self.selected = Some(RowKey::of(first));
        }
        self.update_arrows(cx);
        cx.notify();
    }

    fn update_arrows(&mut self, cx: &mut Context<Self>) {
        // A hovered move in Line preview shows just that move. The line can change under
        // a still mouse, so it is drawn again from the line shown now; a move that is no
        // longer there ends the hover.
        if let Some(index) = self.pv_hover {
            if let Some(arrow) = self.hovered_arrow(index) {
                self.board
                    .update(cx, |board, cx| board.set_arrows(vec![arrow], cx));
                return;
            }
            self.pv_hover = None;
        }
        // A highlighted range is already drawn on the board.
        if self.pv_view.as_ref().is_some_and(PvView::is_range) {
            return;
        }
        // The board shows the previewed position, which only the kibitz analyses.
        if self.pv_kibitz {
            let arrows: Vec<Arrow> = self
                .kibitz
                .read(cx)
                .line
                .as_ref()
                .and_then(|line| {
                    let (_, moves) =
                        panels::replay(&self.shown_pos(), &line.sans[..line.sans.len().min(1)]);
                    let mv = moves.first()?;
                    Some(Arrow::new(mv.from()?, mv.to()))
                })
                .into_iter()
                .collect();
            self.board
                .update(cx, |board, cx| board.set_arrows(arrows, cx));
            return;
        }
        let pos = self.analysed_pos();
        let arrow_for = |san: &str| -> Option<Arrow> {
            let parsed: chess_core::shakmaty::san::San = san.parse().ok()?;
            let mv = parsed.to_move(&pos).ok()?;
            Some(Arrow::new(mv.from()?, mv.to()))
        };
        let quality_arrows = self.model.read(cx).config.view.quality_arrows;
        let relative = |score: Score| {
            if pos.turn().is_white() {
                score
            } else {
                score.negate()
            }
        };
        let best = self
            .rows
            .iter()
            .filter_map(|row| row.score.map(relative))
            .max_by_key(|score| score.sort_key());
        let engine_color = |row: &MoveRow| {
            row.engine_id
                .and_then(|id| self.model.read(cx).engine(id)?.arrow_color)
        };
        let mut arrows: Vec<Arrow> = Vec::new();
        if quality_arrows {
            // Every engine line gets an arrow coloured by quality, worst drawn first
            // so the best end up on top; the selected one is drawn last of all.
            let mut graded: Vec<(&MoveRow, Quality)> = self
                .rows
                .iter()
                .filter(|row| !row.pv.is_empty())
                .filter_map(|row| Some((row, row.quality?)))
                .collect();
            // Several engines on one move share its arrow: the best-ranked row's.
            let mut seen: Vec<&str> = Vec::new();
            graded.retain(|(row, _)| {
                let first = !seen.contains(&row.san.as_str());
                seen.push(&row.san);
                first
            });
            graded.sort_by_key(|(_, quality)| std::cmp::Reverse(*quality));
            let selected_san = self.selected.as_ref().map(|key| key.san.as_str());
            for (row, quality) in graded {
                if selected_san == Some(row.san.as_str()) {
                    continue;
                }
                if let Some(arrow) = arrow_for(&row.san) {
                    let scale = best
                        .zip(row.score.map(relative))
                        .map_or(1.0, |(best, score)| {
                            engine::quality::secondary_arrow_scale(best, score)
                        });
                    let arrow = arrow.with_scale(scale);
                    arrows.push(
                        arrow.with_color(
                            engine_color(row).unwrap_or_else(|| quality_color(quality)),
                        ),
                    );
                }
            }
        }
        if let Some(key) = &self.selected
            && let Some(arrow) = arrow_for(&key.san)
        {
            let quality = self
                .selected_row()
                .and_then(|row| row.quality)
                .filter(|_| quality_arrows);
            let arrow = match quality {
                Some(quality) => arrow.with_color(quality_color(quality)),
                None => arrow,
            };
            arrows.push(match self.selected_row().and_then(engine_color) {
                Some(color) => arrow.with_color(color),
                None => arrow,
            });
        }
        self.board
            .update(cx, |board, cx| board.set_arrows(arrows, cx));
    }

    pub fn play_move(&mut self, mv: Move, window: &mut Window, cx: &mut Context<Self>) {
        self.current = self.tree.add_move(self.current, mv);
        self.position_changed(true, window, cx);
    }

    pub fn jump(&mut self, id: NodeId, window: &mut Window, cx: &mut Context<Self>) {
        let adjacent = self.tree.node(id).parent == Some(self.current);
        // Remember the way here, so forward steps retrace it after stepping back.
        let path = self.tree.path_to(id);
        for &node in &path {
            if let Some(parent) = self.tree.node(node).parent {
                self.last_child.insert(parent, node);
            }
        }
        self.current = id;
        self.context_menu = None;
        self.position_changed(adjacent, window, cx);
    }

    pub fn step(&mut self, delta: i32, window: &mut Window, cx: &mut Context<Self>) {
        let node = self.tree.node(self.current);
        let target = if delta < 0 {
            node.parent
        } else {
            self.next_child(self.current)
        };
        if let Some(target) = target {
            self.jump(target, window, cx);
        }
    }

    /// The child to step forward into: the one last visited, else the main line.
    fn next_child(&self, id: NodeId) -> Option<NodeId> {
        let children = &self.tree.node(id).children;
        self.last_child
            .get(&id)
            .copied()
            .filter(|child| children.contains(child))
            .or_else(|| children.first().copied())
    }

    pub fn go_first(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.jump(GameTree::ROOT, window, cx);
    }

    pub fn go_last(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut end = self.current;
        while let Some(next) = self.next_child(end) {
            end = next;
        }
        self.jump(end, window, cx);
    }

    /// Turns the board around and remembers it for the next game and launch.
    pub fn flip(&mut self, cx: &mut Context<Self>) {
        let flipped = !self.board.read(cx).is_flipped();
        self.board
            .update(cx, |board, cx| board.set_flipped(flipped, cx));
        self.model.update(cx, |model, cx| {
            model.config.board.flipped = flipped;
            model.save_config(cx);
        });
        cx.notify();
    }

    pub fn toggle_engine(&mut self, cx: &mut Context<Self>) {
        self.live.update(cx, |live, cx| live.toggle(cx));
    }

    /// Holds or resumes the engine search without unloading the engines.
    pub fn toggle_pause(&mut self, cx: &mut Context<Self>) {
        self.live.update(cx, |live, cx| {
            let paused = live.paused;
            live.set_paused(!paused, cx);
        });
    }

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
            let (games, errors) = read_game_files(&paths);
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

    /// Starts analysing the shown line from its end back to the first move, or stops.
    pub fn toggle_backward(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.backward.is_some() {
            self.stop_backward(Some(ui::i18n::t("Backwards analysis stopped.")), cx);
            return;
        }
        if self.model.read(cx).analysis_engines().is_empty() {
            self.model.toast(
                "Add an engine for analysis first, in Settings › Engines.",
                cx,
            );
            return;
        }
        if self.model.read(cx).demo {
            self.model.toast(
                "Backwards analysis needs a real engine; demo mode only has scripted lines.",
                cx,
            );
            return;
        }
        // A fixed node count, set in Settings › Analysis, so every position gets the same
        // search whatever the limit for ordinary analysis.
        let nodes = self.model.read(cx).config.backward_nodes.max(1);
        self.live.update(cx, |live, cx| {
            live.limit_override = Some(crate::config::AnalysisLimit::Nodes(nodes));
            if !live.on {
                live.toggle(cx);
            }
            live.set_paused(false, cx);
        });
        let end = self
            .tree
            .line_through(self.current)
            .last()
            .copied()
            .unwrap_or(self.current);
        // The jump searches the end position under the new limit.
        self.jump(end, window, cx);
        let poll = cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(100))
                    .await;
                let alive = this.update_in(cx, |this, window, cx| this.backward_tick(window, cx));
                if !matches!(alive, Ok(true)) {
                    return;
                }
            }
        });
        self.backward = Some(BackwardRun {
            node: end,
            started: std::time::Instant::now(),
            _poll: poll,
        });
        cx.notify();
    }

    /// Moves one position back once every engine has finished; false ends the poll.
    fn backward_tick(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(run) = &self.backward else {
            return false;
        };
        if run.node != self.current {
            self.stop_backward(None, cx);
            return false;
        }
        if run.started.elapsed() < BACKWARD_DWELL {
            return true;
        }
        let (loaded, idle) = {
            let live = self.live.read(cx);
            (live.loaded(), live.idle())
        };
        if loaded == 0 {
            self.stop_backward(
                Some(ui::i18n::t(
                    "Backwards analysis stopped: no engine is running.",
                )),
                cx,
            );
            return false;
        }
        if !idle {
            return true;
        }
        let Some(parent) = self.tree.node(self.current).parent else {
            self.stop_backward(Some(ui::i18n::t("Backwards analysis finished.")), cx);
            return false;
        };
        // Recorded before the jump, so the run sees its own move and carries on.
        if let Some(run) = &mut self.backward {
            run.node = parent;
            run.started = std::time::Instant::now();
        }
        self.jump(parent, window, cx);
        true
    }

    fn stop_backward(&mut self, message: Option<&str>, cx: &mut Context<Self>) {
        // Dropping the run drops its poll task.
        if self.backward.take().is_none() {
            return;
        }
        self.live.update(cx, |live, cx| {
            live.limit_override = None;
            live.search(cx);
        });
        if let Some(message) = message {
            self.model.toast(message.to_owned(), cx);
        }
        cx.notify();
    }

    /// Opens the raw-output window for the engine the panels show.
    pub fn open_engine_output(&mut self, cx: &mut Context<Self>) {
        match self.panel_engine_id(cx) {
            Some(id) => crate::model::open_debug_window(&self.model, id, cx),
            None => self
                .model
                .toast("No engine yet. Add one in Settings › Engines.", cx),
        }
    }

    pub fn copy_fen_action(&mut self, cx: &mut Context<Self>) {
        self.copy_fen(cx);
    }

    pub fn copy_pgn_action(&mut self, cx: &mut Context<Self>) {
        self.copy_pgn(cx);
    }

    pub fn toggle_all_moves(&mut self, cx: &mut Context<Self>) {
        self.set_view(cx, |view| view.all_moves = !view.all_moves);
    }

    pub fn toggle_quality_arrows(&mut self, cx: &mut Context<Self>) {
        self.set_view(cx, |view| view.quality_arrows = !view.quality_arrows);
    }

    pub fn toggle_sort(&mut self, cx: &mut Context<Self>) {
        self.set_view(cx, |view| {
            view.sort = match view.sort {
                LineSort::Eval => LineSort::Depth,
                LineSort::Depth => LineSort::Eval,
            };
        });
    }

    /// The next graph tab under the board.
    pub fn next_graph(&mut self, cx: &mut Context<Self>) {
        self.set_view(cx, |view| {
            let index = GraphTab::ALL
                .iter()
                .position(|tab| *tab == view.graph)
                .unwrap_or(0);
            view.graph = GraphTab::ALL[(index + 1) % GraphTab::ALL.len()];
        });
    }

    pub fn toggle_scoresheet_style(&mut self, cx: &mut Context<Self>) {
        self.set_view(cx, |view| {
            view.scoresheet = match view.scoresheet {
                ScoresheetStyle::Table => ScoresheetStyle::Inline,
                ScoresheetStyle::Inline => ScoresheetStyle::Table,
            };
        });
    }

    /// One more or one fewer MultiPV line on every analysis engine.
    pub fn change_lines(&mut self, up: bool, cx: &mut Context<Self>) {
        self.step_lines(up, cx);
    }

    /// Changes one of the Analyse display options and redraws.
    pub(super) fn set_view(
        &mut self,
        cx: &mut Context<Self>,
        change: impl FnOnce(&mut crate::config::AnalysisView),
    ) {
        self.model.update(cx, |model, cx| {
            change(&mut model.config.view);
            model.save_config(cx);
        });
        self.rebuild_rows(cx);
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
        let findings = crate::screens::database::review_marks(details)
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
    fn copy_fen(&mut self, cx: &mut Context<Self>) {
        let fen = GameTree::fen_of(&self.analysed_pos());
        cx.write_to_clipboard(ClipboardItem::new_string(fen));
        self.copy_menu = false;
        self.model.toast("FEN copied.", cx);
    }

    /// The game with each analysed move's evaluation written into its comment as a
    /// `[%eval]` command, so saved and copied PGN keeps the analysis. An eval the
    /// game already carries stays unless the engine searched deeper.
    fn annotated_tree(&self) -> GameTree {
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
    fn copy_pgn(&mut self, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(write_pgn(&self.annotated_tree())));
        self.copy_menu = false;
        self.model.toast("PGN copied.", cx);
    }

    /// The copy button and, when open, its menu above it.
    fn render_copy_menu(&self, width: f32, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let this = cx.entity();
        let toggle_this = this.clone();
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
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(rgb(T.muted))
                        .child(hint),
                )
        };
        let fen_this = this.clone();
        let pgn_this = this;
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
                    .border_1()
                    .border_color(rgb(T.border_strong))
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
                    .on_click(move |_, cx| {
                        toggle_this.update(cx, |screen, cx| {
                            screen.copy_menu = !screen.copy_menu;
                            cx.notify();
                        });
                    }),
            )
            .children(menu)
    }

    /// Copies the board as an image and saves it under Pictures.
    fn snapshot_board(&mut self, cx: &mut Context<Self>) {
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
                model.refresh_databases();
                cx.emit(ModelEvent::Databases);
            });
        })
        .detach();
    }

    /// Whether the game opened from a database has changes it has not saved back.
    fn db_game_dirty(&self) -> bool {
        self.db_game.is_some() && write_pgn(&self.tree) != self.db_saved_pgn
    }

    /// Writes the board's game, comments and variations included, over the stored game
    /// it was opened from.
    fn save_to_db_game(&mut self, origin: DbGame, cx: &mut Context<Self>) {
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
                model.refresh_databases();
                model.refresh_leaks(cx);
                cx.emit(ModelEvent::Databases);
            });
        })
        .detach();
    }

    fn annotate(&mut self, nag: u8, cx: &mut Context<Self>) {
        if self.current != GameTree::ROOT {
            self.tree.toggle_nag(self.current, nag);
            cx.notify();
        }
    }

    fn promote(&mut self, id: NodeId, cx: &mut Context<Self>) {
        self.tree.promote(id);
        self.context_menu = None;
        cx.notify();
    }

    fn delete_from(&mut self, id: NodeId, window: &mut Window, cx: &mut Context<Self>) {
        let on_deleted_line = self.tree.path_to(self.current).contains(&id);
        let parent = self.tree.delete_from(id);
        self.context_menu = None;
        if on_deleted_line {
            self.current = parent;
            self.position_changed(false, window, cx);
        }
        cx.notify();
    }

    fn selected_row(&self) -> Option<&MoveRow> {
        let key = self.selected.as_ref()?;
        self.rows
            .iter()
            .find(|row| key.matches(row))
            // The chosen engine may have dropped the move; another row for it will do.
            .or_else(|| self.rows.iter().find(|row| row.san == key.san))
    }

    fn select_row(&mut self, key: RowKey, cx: &mut Context<Self>) {
        if self.continue_pv_with(&key, cx) {
            return;
        }
        self.row_chosen = true;
        self.selected = Some(key);
        self.update_arrows(cx);
        cx.notify();
    }

    /// Adds SAN moves as a line from the current node, returning each new node.
    fn add_line(&mut self, sans: &[String]) -> Vec<NodeId> {
        let mut nodes = Vec::with_capacity(sans.len());
        let mut node = self.current;
        for san in sans {
            match self.tree.add_san(node, san) {
                Ok(next) => {
                    nodes.push(next);
                    node = next;
                }
                Err(err) => {
                    log::warn!("could not add {san}: {err}");
                    break;
                }
            }
        }
        nodes
    }

    /// The line the Line preview buttons act on, from the game position.
    fn action_line(&self) -> Option<Vec<String>> {
        if let Some(view) = &self.pv_view {
            return Some(view.sans.clone());
        }
        let row = self.selected_row()?;
        Some(if row.pv.is_empty() {
            vec![row.san.clone()]
        } else {
            row.pv.clone()
        })
    }

    fn play_line(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(line) = self.action_line() else {
            return;
        };
        // From a preview, stop at the move being looked at; otherwise play it all.
        let target = self.pv_view.as_ref().map(PvView::played);
        let nodes = self.add_line(&line);
        let index = target.unwrap_or(nodes.len()).saturating_sub(1);
        if let Some(&node) = nodes.get(index) {
            self.jump(node, window, cx);
        }
    }

    fn add_variation(&mut self, cx: &mut Context<Self>) {
        if let Some(line) = self.action_line() {
            self.add_line(&line);
            cx.notify();
        }
    }

    fn add_to_repertoire(&mut self, cx: &mut Context<Self>) {
        let Some(line) = self.action_line() else {
            return;
        };
        let mut path = self.base_san_path();
        path.extend(line);
        let refs: Vec<&str> = path.iter().map(String::as_str).collect();
        self.model
            .update(cx, |model, cx| match model.repertoire.add_line(&refs) {
                Ok(()) => {
                    model.save_repertoire(cx);
                    model.refresh_leaks(cx);
                    model.toast("Added to the repertoire.", cx);
                }
                Err(err) => model.toast(format!("Could not add the line: {err}"), cx),
            });
        self.rebuild_rows(cx);
    }

    fn timeline_ids(&self) -> Vec<NodeId> {
        self.tree.line_through(self.current)
    }

    /// A side's player name and rating from the game headers, with the colour as the name and
    /// an em dash as the rating when the game does not say.
    fn player(&self, side: chess_core::Color) -> (String, String) {
        let (tag, fallback) = if side.is_white() {
            ("White", ui::i18n::t("White"))
        } else {
            ("Black", ui::i18n::t("Black"))
        };
        let known = |tag: &str| {
            self.tree
                .header(tag)
                .map(str::trim)
                .filter(|value| !value.is_empty() && *value != "?")
                .map(str::to_owned)
        };
        (
            known(tag).unwrap_or_else(|| fallback.to_owned()),
            known(&format!("{tag}Elo")).unwrap_or_else(|| "—".to_owned()),
        )
    }

    /// Folds the scoresheet to a narrow strip or opens it again, and remembers it.
    fn set_scoresheet_collapsed(&mut self, collapsed: bool, cx: &mut Context<Self>) {
        self.model.update(cx, |model, cx| {
            model.config.panels.scoresheet_collapsed = collapsed;
            model.schedule_config_save(cx);
        });
        cx.notify();
    }

    /// The folded scoresheet: a button to open it again and, for a game from a
    /// database, a Save button that shows when there are unsaved changes.
    fn render_scoresheet_rail(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let this = cx.entity();
        let expand = this.clone();
        v_flex()
            .w_full()
            .h_full()
            .items_center()
            .pt(px(12.))
            .gap(px(6.))
            .bg(rgb(T.surface))
            .border_r_1()
            .border_color(rgb(T.border))
            .child(
                icon_btn("sheet-expand", "icons/chevron-right.svg")
                    .compact()
                    .tooltip(ui::i18n::t("Show the scoresheet"))
                    .on_click(move |_, cx| {
                        expand.update(cx, |screen, cx| screen.set_scoresheet_collapsed(false, cx))
                    }),
            )
            .when_some(self.db_game.as_ref(), |rail, origin| {
                let dirty = self.db_game_dirty();
                let tip = if dirty {
                    format!("{}: {}", origin.name, ui::i18n::t("Unsaved changes"))
                } else {
                    format!("{}: {}", origin.name, ui::i18n::t("Saved in this database"))
                };
                rail.child(
                    icon_btn("sheet-rail-save", "icons/database.svg")
                        .compact()
                        .when(dirty, |button| button.dot(T.warn))
                        .disabled(!dirty)
                        .tooltip(tip)
                        .on_click(move |_, cx| this.update(cx, |screen, cx| screen.save_game(cx))),
                )
            })
    }

    /// Under the scoresheet header while the game came from a database: which database,
    /// whether there are unsaved changes, a Save button, and the review findings.
    fn render_db_game_strip(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let origin = self.db_game.as_ref()?;
        let dirty = self.db_game_dirty();
        let this = cx.entity();
        let save_this = this.clone();
        let mut strip = v_flex()
            .flex_none()
            .px(px(18.))
            .py(px(10.))
            .gap(px(8.))
            .bg(rgb(T.surface_2))
            .border_b_1()
            .border_color(rgb(T.border))
            .child(
                h_flex()
                    .gap(px(8.))
                    .child(ui::widgets::icon("icons/database.svg", px(16.), T.accent))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .truncate()
                                    .child(origin.name.clone()),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(rgb(if dirty { T.warn } else { T.muted }))
                                    .child(if dirty {
                                        ui::i18n::t("Unsaved changes")
                                    } else {
                                        ui::i18n::t("Saved in this database")
                                    }),
                            ),
                    )
                    .child(
                        btn("db-game-save", "Save")
                            .compact()
                            .when(dirty, |button| button.primary())
                            .disabled(!dirty)
                            .tooltip(ui::i18n::t(
                                "Save moves, comments and variations to the database (Ctrl+S)",
                            ))
                            .on_click(move |_, cx| {
                                save_this.update(cx, |screen, cx| screen.save_game(cx))
                            }),
                    ),
            );
        if !self.db_findings.is_empty() {
            let mut chips = h_flex().flex_wrap().gap(px(4.));
            for (index, (node, mark)) in self.db_findings.iter().enumerate() {
                let node = *node;
                let this = this.clone();
                let tip = if mark.best.is_empty() {
                    mark.loss.clone()
                } else {
                    format!(
                        "{}, {} {}",
                        mark.loss,
                        ui::i18n::t("engine line"),
                        mark.best
                    )
                };
                chips = chips.child(
                    btn(
                        ("db-finding", index),
                        format!("{} {}", mark.label, mark.loss),
                    )
                    .mini()
                    .mono()
                    .pad(6.)
                    .dot(quality_color(mark.quality))
                    .selected(node == self.current)
                    .tooltip(tip)
                    .on_click(move |window, cx| {
                        this.update(cx, |screen, cx| screen.jump(node, window, cx))
                    }),
                );
            }
            strip = strip.child(section_label("Review findings")).child(chips);
        }
        Some(strip.into_any_element())
    }

    fn render_scoresheet(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let this = cx.entity();
        let opening = self
            .tree
            .header("Opening")
            .map(str::to_owned)
            .or_else(|| self.opening().map(|o| format!("{} {}", o.eco, o.name)))
            .unwrap_or_else(|| ui::i18n::t("Starting position").to_owned());
        let notation = self.model.read(cx).config.board.notation;
        let select_this = this.clone();
        let context_this = this.clone();
        // Only a move drawn in this frame counts; the start position has none.
        self.scoresheet_current.set(None);
        let evals = self
            .evals
            .iter()
            .filter(|(id, _)| **id != GameTree::ROOT)
            .map(|(&id, &cp)| (id, eval_label(cp, self.mates.get(&id).copied()).into()))
            .collect();
        let props = ScoresheetProps {
            current: self.current,
            current_bounds: self.scoresheet_current.clone(),
            evals: Rc::new(evals),
            notation,
            on_select: Rc::new(move |id, window, cx| {
                select_this.update(cx, |screen, cx| screen.jump(id, window, cx));
            }),
            on_context: Rc::new(move |id, event, _, cx| {
                let position = event.position;
                context_this.update(cx, |screen, cx| {
                    screen.context_menu = Some((id, position));
                    cx.notify();
                });
            }),
        };
        let style = self.model.read(cx).config.view.scoresheet;
        let sheet = match style {
            ScoresheetStyle::Table => scoresheet_body(&self.tree, &props).into_any_element(),
            ScoresheetStyle::Inline => scoresheet_inline(&self.tree, &props).into_any_element(),
        };
        let style_toggle = {
            let this = this.clone();
            let (label, tip) = match style {
                ScoresheetStyle::Table => (
                    ui::i18n::t("Table"),
                    "Lichess style. Click for running text.",
                ),
                ScoresheetStyle::Inline => (
                    ui::i18n::t("Inline"),
                    "Nibbler style. Click for the move table.",
                ),
            };
            btn("sheet-style", label)
                .mini()
                .tooltip(tip)
                .on_click(move |_, cx| {
                    this.update(cx, |screen, cx| {
                        screen.set_view(cx, |view| {
                            view.scoresheet = match view.scoresheet {
                                ScoresheetStyle::Table => ScoresheetStyle::Inline,
                                ScoresheetStyle::Inline => ScoresheetStyle::Table,
                            };
                        });
                    });
                })
        };
        let nag_button = |id: &'static str, label: &'static str, nag: u8, this: Entity<Self>| {
            btn(id, label)
                .w(36.)
                .h(30.)
                .pad(0.)
                .on_click(move |_, cx| this.update(cx, |screen, cx| screen.annotate(nag, cx)))
        };
        v_flex()
            .w_full()
            .h_full()
            .flex_none()
            .bg(rgb(T.surface))
            .border_r_1()
            .border_color(rgb(T.border))
            .child(
                v_flex()
                    .flex_none()
                    .px(px(18.))
                    .pt(px(16.))
                    .pb(px(10.))
                    .gap(px(1.))
                    .border_b_1()
                    .border_color(rgb(T.border))
                    .child(
                        h_flex()
                            .justify_between()
                            .mb(px(9.))
                            .child(section_label("Scoresheet"))
                            .when(self.tree.variant == chess_core::Variant::Chess960, |row| {
                                row.child(ui::widgets::badge("Chess960"))
                            })
                            .child(h_flex().gap(px(4.)).child(style_toggle).child({
                                let this = this.clone();
                                icon_btn("sheet-collapse", "icons/chevron-left.svg")
                                    .mini()
                                    .tooltip(ui::i18n::t("Hide the scoresheet"))
                                    .on_click(move |_, cx| {
                                        this.update(cx, |screen, cx| {
                                            screen.set_scoresheet_collapsed(true, cx)
                                        })
                                    })
                            })),
                    )
                    .child(
                        div()
                            .text_size(px(13.))
                            .text_color(rgb(T.muted))
                            .child(opening),
                    ),
            )
            .children(self.render_db_game_strip(cx))
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .child(
                        div()
                            .id("scoresheet-scroll")
                            .size_full()
                            .overflow_y_scroll()
                            .track_scroll(&self.scoresheet_scroll)
                            .py(px(4.))
                            .child(sheet),
                    )
                    .child(self.scoresheet_follower())
                    .vertical_scrollbar(&self.scoresheet_scroll),
            )
            .when(self.editing_comment, |column| {
                let comment = self.comment.clone();
                let save = this.clone();
                column.child(
                    v_flex()
                        .flex_none()
                        .gap(px(6.))
                        .p(px(12.))
                        .border_t_1()
                        .border_color(rgb(T.border))
                        .child(gpui_component::input::Textarea::new(&comment).h(px(80.)))
                        .child(
                            btn("comment-save", "Save comment")
                                .primary()
                                .h(30.)
                                .on_click(move |_, cx| {
                                    save.update(cx, |screen, cx| {
                                        let text = screen.comment.read(cx).value().to_string();
                                        let current = screen.current;
                                        screen.tree.node_mut(current).comment = text;
                                        screen.editing_comment = false;
                                        cx.notify();
                                    });
                                }),
                        ),
                )
            })
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_wrap()
                    .flex_none()
                    .gap(px(6.))
                    .px(px(18.))
                    .py(px(12.))
                    .border_t_1()
                    .border_color(rgb(T.border))
                    .child(nag_button("nag-good", "!", 1, this.clone()))
                    .child(nag_button("nag-bad", "?", 2, this.clone()))
                    .child(nag_button("nag-brilliant", "!!", 3, this.clone()))
                    .child(nag_button("nag-blunder", "??", 4, this.clone()))
                    .child(nag_button("nag-inaccuracy", "?!", 6, this.clone()))
                    .child(nag_button("nag-interesting", "!?", 5, this.clone()))
                    .child(
                        btn("comment", ui::i18n::t("Comment"))
                            .h(30.)
                            .pad(13.)
                            .on_click({
                                let this = this.clone();
                                move |window, cx| {
                                    this.update(cx, |screen, cx| {
                                        screen.editing_comment = !screen.editing_comment;
                                        let text = screen.tree.node(screen.current).comment.clone();
                                        screen.comment.update(cx, |state, cx| {
                                            state.set_value(text, window, cx);
                                        });
                                        cx.notify();
                                    });
                                }
                            }),
                    )
                    .child(btn("promote", "Promote").h(30.).pad(13.).on_click({
                        let this = this.clone();
                        move |_, cx| {
                            this.update(cx, |screen, cx| {
                                let current = screen.current;
                                screen.promote(current, cx);
                            });
                        }
                    })),
            )
    }

    fn render_board_column(
        &mut self,
        board_size: Pixels,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let this = cx.entity();
        let compact = board_size < px(440.);
        // Eight buttons share the row under the board, so they narrow with it.
        let nav_width = if board_size < px(340.) {
            32.
        } else if compact {
            34.
        } else {
            42.
        };
        let shown = self.shown_pos();
        let turn = if shown.turn().is_white() {
            ui::i18n::t("White")
        } else {
            ui::i18n::t("Black")
        };
        let end = self.live.read(cx).end;
        let status = if self.editor.is_some() {
            ui::i18n::t("Edit position").to_owned()
        } else if let Some(end) = end {
            end.describe().to_owned()
        } else {
            format!("{turn} to move")
        };
        let after = (self.current != GameTree::ROOT && self.editor.is_none())
            .then(|| format!("after {}", self.tree.move_label(self.current)));
        let after = self
            .pv_preview_label(self.tree.node(self.current).ply)
            .or(after);
        let live = self.live.read(cx);
        let eval_text = live
            .primary()
            .and_then(|engine| engine.lines.get(&1))
            .and_then(|line| line.score)
            .map(Score::display)
            .or_else(|| {
                self.evals
                    .get(&self.current)
                    .filter(|_| self.pv_view.is_none())
                    .map(|cp| Score::Cp(*cp).display())
            })
            .filter(|_| self.editor.is_none())
            .unwrap_or_else(|| "—".to_owned());
        // A finished game shows its result, not an evaluation.
        let eval_text = match end.filter(|_| self.editor.is_none()) {
            Some(end) => end.result().replace("1/2", "½"),
            None => eval_text,
        };
        let graph = self.render_graph(cx);
        // What each side is up on: Black's pieces taken by White, then White's by Black.
        let material = {
            let balance = chess_core::material_balance(shown.board());
            let set = self.model.read(cx).config.board.prefs().pieces;
            let strip = move |side: chess_core::Color| {
                (!balance.up(side).is_empty()).then(|| {
                    ui::board::material_strip(
                        balance.up(side),
                        side.other(),
                        balance.lead(side),
                        set,
                        17.,
                    )
                })
            };
            let editing = self.editor.is_some();
            move |side| strip(side).filter(|_| !editing)
        };
        let flipped = self.board.read(cx).is_flipped();
        let to_move = shown.turn();
        let player_strip = |side: chess_core::Color| {
            let (name, rating) = self.player(side);
            h_flex()
                .h(px(26.))
                .gap(px(8.))
                .justify_between()
                .child(
                    h_flex()
                        .gap(px(8.))
                        .min_w_0()
                        .child(
                            // The side to move is marked beside its name.
                            div()
                                .flex_none()
                                .size(px(6.))
                                .rounded_full()
                                .when(to_move == side && end.is_none(), |dot| {
                                    dot.bg(rgb(T.accent))
                                }),
                        )
                        .child(
                            div()
                                .min_w_0()
                                .overflow_hidden()
                                .text_ellipsis()
                                .whitespace_nowrap()
                                .text_size(px(13.))
                                .font_weight(FontWeight::MEDIUM)
                                .child(name),
                        )
                        .child(
                            mono(rating)
                                .flex_none()
                                .text_size(px(12.))
                                .text_color(rgb(T.muted)),
                        ),
                )
                .children(material(side))
        };
        let (top_side, bottom_side) = if flipped {
            (chess_core::Color::White, chess_core::Color::Black)
        } else {
            (chess_core::Color::Black, chess_core::Color::White)
        };
        let top_strip = player_strip(top_side);
        let bottom_strip = player_strip(bottom_side);
        // Where the shown position sits in the line, in full moves.
        let move_counter = {
            let ply = self.tree.node(self.current).ply;
            let last = self
                .timeline_ids()
                .last()
                .map_or(ply, |id| self.tree.node(*id).ply)
                .max(ply);
            mono(format!(
                "{} {} / {}",
                ui::i18n::t("Move"),
                ply.div_ceil(2),
                last.div_ceil(2)
            ))
            .flex_none()
            .whitespace_nowrap()
            .text_size(px(12.))
            .text_color(rgb(T.muted))
        };
        let nav = |id: &'static str,
                   icon: &'static str,
                   tip: &'static str,
                   this: Entity<Self>,
                   action: fn(&mut Self, &mut Window, &mut Context<Self>)| {
            icon_btn(id, icon)
                .w(nav_width)
                .tooltip(tip)
                .on_click(move |window, cx| {
                    this.update(cx, |screen, cx| action(screen, window, cx));
                })
        };
        let copy_menu = self.render_copy_menu(nav_width, cx);
        let flip_tip = match self.model.read(cx).config.shortcuts.get("flip_board") {
            Some(key) => format!("Flip board ({})", key.to_uppercase()),
            None => ui::i18n::t("Flip board").to_owned(),
        };
        v_flex()
            .flex_none()
            .w(board_size)
            .min_h_full()
            .pb(px(16.))
            .child(
                h_flex()
                    .h(px(39.))
                    .mt(px(12.))
                    .justify_between()
                    .child(
                        h_flex()
                            .gap(px(12.))
                            .items_baseline()
                            .child(
                                div()
                                    .text_size(px(20.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(status),
                            )
                            .children(after.map(|text| {
                                div()
                                    .text_size(px(14.))
                                    .text_color(rgb(T.muted))
                                    .child(text)
                            })),
                    )
                    .child(
                        mono(eval_text)
                            .text_size(px(18.))
                            .font_weight(FontWeight::SEMIBOLD),
                    ),
            )
            .child(top_strip)
            .child({
                let (evals, _) = self.line_evals(&[self.current], cx);
                let share = evals[0].map_or(0.5, |cp| {
                    ((engine::quality::winning_chances(Score::Cp(cp)) + 1.) / 2.) as f32
                });
                div()
                    .relative()
                    .size(board_size)
                    .flex_none()
                    .child(
                        div().absolute().left(px(-14.)).top(px(0.)).child(
                            ui::eval_bar::EvalBar::new("analysis-eval", share, board_size)
                                .flipped(self.board.read(cx).is_flipped()),
                        ),
                    )
                    .child(self.board.clone())
            })
            .child(bottom_strip)
            .child(
                h_flex()
                    .mt(px(6.))
                    .justify_between()
                    .child(
                        h_flex()
                            .gap(px(6.))
                            .child(nav(
                                "first",
                                "icons/skip-back.svg",
                                ui::i18n::t("First move"),
                                this.clone(),
                                Self::go_first,
                            ))
                            .child(nav(
                                "prev",
                                "icons/chevron-left.svg",
                                ui::i18n::t("Previous move"),
                                this.clone(),
                                |s, w, c| s.step(-1, w, c),
                            ))
                            .child(nav(
                                "next",
                                "icons/chevron-right.svg",
                                ui::i18n::t("Next move"),
                                this.clone(),
                                |s, w, c| s.step(1, w, c),
                            ))
                            .child(nav(
                                "last",
                                "icons/skip-forward.svg",
                                ui::i18n::t("Last move"),
                                this.clone(),
                                Self::go_last,
                            ))
                            .when(!compact, |row| row.child(move_counter.ml(px(6.)))),
                    )
                    .child(
                        h_flex()
                            .gap(px(6.))
                            .child(
                                icon_btn("flip", "icons/arrow-down-up.svg")
                                    .w(nav_width)
                                    .selected(flipped)
                                    .tooltip(flip_tip)
                                    .on_click({
                                        let this = this.clone();
                                        move |_, cx| {
                                            this.update(cx, |screen, cx| screen.flip(cx));
                                        }
                                    }),
                            )
                            .child(
                                icon_btn("edit-position", "icons/pencil.svg")
                                    .w(nav_width)
                                    .selected(self.editor.is_some())
                                    .tooltip(ui::i18n::t("Edit position"))
                                    .on_click({
                                        let this = this.clone();
                                        move |_, cx| {
                                            this.update(cx, |screen, cx| screen.toggle_editor(cx));
                                        }
                                    }),
                            )
                            .child(copy_menu)
                            .child(
                                icon_btn("snapshot", "icons/camera.svg")
                                    .w(nav_width)
                                    .tooltip(ui::i18n::t("Copy the board as an image and save it"))
                                    .on_click({
                                        let this = this.clone();
                                        move |_, cx| {
                                            this.update(cx, |screen, cx| screen.snapshot_board(cx));
                                        }
                                    }),
                            ),
                    ),
            )
            .child(graph)
    }

    /// Evals along the shown line: the game's own, else what any engine found for that
    /// position, including by another move order.
    fn line_evals(&self, ids: &[NodeId], cx: &gpui::App) -> (Vec<Option<i32>>, Vec<Option<u32>>) {
        let live = self.live.read(cx);
        let engines = live.engine_ids();
        ids.iter()
            .map(|id| {
                let cached = || {
                    live.cache.eval(
                        &engines,
                        position_hash(&self.tree.node(*id).pos),
                        &crate::analysis_store::tree_position_key(&self.tree, *id),
                    )
                };
                match self.evals.get(id) {
                    Some(cp) => (Some(*cp), self.depths.get(id).copied().filter(|d| *d > 0)),
                    None => match cached() {
                        Some((cp, depth)) => (Some(cp), Some(depth)),
                        None => (None, None),
                    },
                }
            })
            .unzip()
    }

    /// The graph under the board, with tabs for eval, time, depth and the bar timeline.
    fn render_graph(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let this = cx.entity();
        let tab = self.model.read(cx).config.view.graph;
        let ids = self.timeline_ids();
        let (evals, depths) = self.line_evals(&ids, cx);
        let current = ids.iter().position(|&id| id == self.current);
        let scrub_ids = ids.clone();
        let scrub_this = this.clone();
        let on_scrub: ui::timeline::ScrubHandler = Rc::new(move |index, window, cx| {
            if let Some(&id) = scrub_ids.get(index) {
                scrub_this.update(cx, |screen, cx| {
                    if screen.current != id {
                        screen.jump(id, window, cx);
                    }
                });
            }
        });
        let mut tabs = h_flex().gap(px(2.));
        for (index, option) in GraphTab::ALL.into_iter().enumerate() {
            let this = this.clone();
            let active = option == tab;
            tabs = tabs.child(
                div()
                    .id(("graph-tab", index))
                    .px(px(8.))
                    .py(px(2.))
                    .rounded(px(4.))
                    .cursor_pointer()
                    .text_size(px(11.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(if active { T.fg } else { T.muted }))
                    .when(active, |el| el.bg(rgb(T.selected)))
                    .when(!active, |el| el.hover(|style| style.bg(rgb(T.hover))))
                    .on_click(move |_, _, cx| {
                        this.update(cx, |screen, cx| {
                            screen.set_view(cx, |view| view.graph = option);
                        });
                    })
                    .child(option.label()),
            );
        }
        let count = ids.len();
        let body = match tab {
            GraphTab::Timeline => timeline(TimelineProps {
                evals,
                current,
                on_scrub,
            })
            .into_any_element(),
            GraphTab::Eval => ui::graph::graph(ui::graph::GraphProps {
                series: vec![ui::graph::Series {
                    values: evals.iter().map(|cp| cp.map(|cp| cp as f32)).collect(),
                    color: T.fg,
                    fill: true,
                    connect: false,
                }],
                count,
                current,
                scale: ui::graph::Scale::SymLog {
                    max: chess_core::annotation::MATE_CP as f32,
                    unit: 100.0,
                },
                ticks: ui::graph::eval_ticks(),
                height: None,
                on_scrub,
                empty: "No evaluations yet. Turn the engine on or load an annotated game.",
            })
            .into_any_element(),
            GraphTab::Time => {
                let times = chess_core::move_times(&self.tree, &ids);
                let max = times.iter().flatten().fold(0.0f64, |a, b| a.max(*b)) as f32;
                let side = |white: bool| ui::graph::Series {
                    values: ids
                        .iter()
                        .zip(&times)
                        .map(|(id, time)| {
                            let is_white = self.tree.node(*id).ply % 2 == 1;
                            time.filter(|_| is_white == white).map(|t| t as f32)
                        })
                        .collect(),
                    color: if white { T.fg } else { T.warn },
                    fill: false,
                    connect: true,
                };
                ui::graph::graph(ui::graph::GraphProps {
                    series: vec![side(true), side(false)],
                    count,
                    current,
                    scale: ui::graph::Scale::Linear {
                        max: (max * 1.1).max(1.0),
                    },
                    ticks: ui::graph::linear_ticks(max, "s"),
                    height: None,
                    on_scrub,
                    empty: "No clock data. Games with [%clk] or [%emt] comments show time per move.",
                })
                .into_any_element()
            }
            GraphTab::Depth => {
                let max = depths.iter().flatten().copied().max().unwrap_or(0) as f32;
                ui::graph::graph(ui::graph::GraphProps {
                    series: vec![ui::graph::Series {
                        values: depths.iter().map(|d| d.map(|d| d as f32)).collect(),
                        color: T.accent,
                        fill: false,
                        connect: true,
                    }],
                    count,
                    current,
                    scale: ui::graph::Scale::Linear {
                        max: (max * 1.1).max(1.0),
                    },
                    ticks: ui::graph::linear_ticks(max, ""),
                    height: None,
                    on_scrub,
                    empty: "No search depths yet.",
                })
                .into_any_element()
            }
        };
        v_flex()
            .flex_1()
            .child(
                h_flex()
                    .flex_none()
                    .flex_wrap()
                    .gap(px(6.))
                    .mt(px(14.))
                    .mb(px(6.))
                    .justify_between()
                    .child(
                        h_flex()
                            .gap(px(8.))
                            .child(section_label("Game graph"))
                            .child({
                                let this = this.clone();
                                let running = self.backward.is_some();
                                let nodes = self.model.read(cx).config.backward_nodes;
                                btn(
                                    "analyse-backward",
                                    if running { ui::i18n::t("Stop") } else { ui::i18n::t("Analyse backwards") },
                                )
                                .mini()
                                .selected(running)
                                .tooltip(format!(
                                    "Search every position from the last move to the first, {} each (Settings › Analysis)",
                                    engine::format_nodes(nodes)
                                ))
                                .on_click(move |window, cx| {
                                    this.update(cx, |screen, cx| screen.toggle_backward(window, cx));
                                })
                            }),
                    )
                    .child(tabs),
            )
            .child(body)
            .into_any_element()
    }

    /// Opens or closes the opening tree's database list, keeping keyboard focus on
    /// the picker so the arrow keys move through it.
    fn toggle_explorer_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.explorer_menu = match self.explorer_menu {
            Some(_) => None,
            None => Some(self.explorer_current_index(cx)),
        };
        self.explorer_focus.focus(window, cx);
        cx.notify();
    }

    /// Where the database in use sits in the picker's list.
    fn explorer_current_index(&self, cx: &gpui::App) -> usize {
        let current = self.explorer_database(cx).map(|entry| entry.path);
        self.explorer_choices(cx)
            .iter()
            .position(|entry| Some(&entry.path) == current.as_ref())
            .unwrap_or(0)
    }

    /// Keys on the focused picker: Enter, Space or Down open it; the arrows move,
    /// Enter picks and Escape closes. Returns whether the key was used.
    fn explorer_key(&mut self, key: &str, cx: &mut Context<Self>) -> bool {
        let count = self.explorer_choices(cx).len();
        match (self.explorer_menu, key) {
            (None, "enter" | "space" | "down") if count > 0 => {
                self.explorer_menu = Some(self.explorer_current_index(cx));
            }
            (Some(index), "down") => self.explorer_menu = Some((index + 1) % count.max(1)),
            (Some(index), "up") => {
                self.explorer_menu = Some((index + count.max(1) - 1) % count.max(1))
            }
            (Some(index), "enter" | "space") => {
                if let Some(entry) = self.explorer_choices(cx).get(index) {
                    let path = entry.path.clone();
                    self.choose_explorer_db(path, cx);
                }
            }
            (Some(_), "escape" | "tab") => {
                self.explorer_menu = None;
                cx.notify();
                return key == "escape";
            }
            _ => return false,
        }
        cx.notify();
        true
    }

    /// The Reference column's heading: the opening tree's database, which opens a
    /// list of every database to count instead. It keeps the column's width.
    fn render_explorer_picker(&self, width: f32, cx: &mut Context<Self>) -> gpui::AnyElement {
        let this = cx.entity();
        let name = self.explorer_database(cx).map_or_else(
            || ui::i18n::t("Reference").to_owned(),
            |entry| explorer_label(&entry.name),
        );
        let open = self.explorer_menu;
        let (click, keys) = (this.clone(), this.clone());
        let button = h_flex()
            .id("explorer-picker")
            .key_context("FastButton")
            .track_focus(&self.explorer_focus)
            .w_full()
            .h(px(24.))
            .pl(px(4.))
            .pr(px(2.))
            .gap(px(4.))
            .rounded(px(4.))
            .border_1()
            .border_color(gpui::transparent_black())
            .cursor_pointer()
            .hover(|style| style.bg(rgb(T.hover)))
            .focus_visible(|style| style.border_2().border_color(rgb(T.accent)))
            .when(open.is_none(), |button| {
                button.tooltip(|window, cx| {
                    gpui_component::tooltip::Tooltip::new(ui::i18n::t(
                        "Choose the database for the opening tree",
                    ))
                    .build(window, cx)
                })
            })
            .on_click(move |event, window, cx| {
                // Enter and Space are handled with the arrow keys in `explorer_key`.
                if !event.is_keyboard() {
                    click.update(cx, |screen, cx| screen.toggle_explorer_menu(window, cx));
                }
            })
            .on_key_down(move |event: &gpui::KeyDownEvent, _, cx| {
                if event.keystroke.modifiers.modified() {
                    return;
                }
                let key = event.keystroke.key.clone();
                if keys.update(cx, |screen, cx| screen.explorer_key(&key, cx)) {
                    cx.stop_propagation();
                }
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(11.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .font_family(ui::SANS)
                    .text_color(rgb(if open.is_some() { T.fg } else { T.muted }))
                    .child(name.to_uppercase()),
            )
            .child(ui::widgets::icon(
                "icons/chevron-down.svg",
                px(12.),
                T.muted,
            ));
        let menu = open.map(|highlight| {
            let game_db = self.db_game.as_ref().map(|game| game.path.clone());
            let mut list = v_flex()
                .id("explorer-menu")
                .w(px(260.))
                .py(px(4.))
                .rounded(px(6.))
                .bg(rgb(T.surface))
                .border_1()
                .border_color(rgb(T.border_strong))
                .on_mouse_down_out({
                    let this = this.clone();
                    move |_, _, cx| {
                        this.update(cx, |screen, cx| {
                            screen.explorer_menu = None;
                            cx.notify();
                        })
                    }
                });
            for (index, entry) in self.explorer_choices(cx).into_iter().enumerate() {
                let this = this.clone();
                let highlighted = highlight == index;
                let is_game_db = game_db.as_ref() == Some(&entry.path);
                let path = entry.path.clone();
                list = list.child(
                    h_flex()
                        .id(("explorer-choice", index))
                        .min_h(px(30.))
                        .px(px(10.))
                        .gap(px(8.))
                        .justify_between()
                        .cursor_pointer()
                        .font_family(ui::SANS)
                        .text_size(px(13.))
                        .text_color(rgb(T.fg))
                        .when(highlighted, |row| row.bg(rgb(T.selected)))
                        .when(!highlighted, |row| {
                            row.hover(|style| style.bg(rgb(T.hover)))
                        })
                        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                            cx.stop_propagation();
                            let path = path.clone();
                            this.update(cx, |screen, cx| screen.choose_explorer_db(path, cx));
                        })
                        .child(
                            v_flex()
                                .min_w_0()
                                .py(px(4.))
                                .child(div().truncate().child(entry.name.clone()))
                                .when(is_game_db, |cell| {
                                    cell.child(
                                        div()
                                            .text_size(px(11.))
                                            .text_color(rgb(T.muted))
                                            .child(ui::i18n::t("This game's database")),
                                    )
                                }),
                        )
                        .child(
                            mono(crate::screens::database::format_count(entry.games))
                                .flex_none()
                                .text_size(px(12.))
                                .text_color(rgb(T.muted)),
                        ),
                );
            }
            gpui::deferred(gpui::anchored().child(div().mt(px(2.)).occlude().child(list)))
                .with_priority(1)
        });
        div()
            .w(px(width))
            .flex_none()
            .child(button)
            .children(menu)
            .into_any_element()
    }

    fn render_moves_table(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let this = cx.entity();
        const COL_REF: f32 = 84.;
        let explorer = self.explorer_database(cx);
        let picker = self.render_explorer_picker(COL_REF, cx);
        let model = self.model.read(cx);
        let notation = model.config.board.notation;
        let live = self.live.read(cx);
        // With several engines each row names the engine whose line it is.
        let engine_badges = live.engines.len() > 1;
        let engine_configs: HashMap<EngineId, engine::EngineConfig> = live
            .engines
            .iter()
            .map(|engine| (engine.config.id, engine.config.clone()))
            .collect();
        let show_wdl = self.show_wdl;
        let quality_arrows = model.config.view.quality_arrows;
        let my_moves = model.config.view.my_moves;
        const COL_BADGE: f32 = 96.;
        const COL_MOVE: f32 = 64.;
        const COL_SF: f32 = 66.;
        let col_lead = if engine_badges { COL_BADGE } else { 0. };
        const COL_WDL: f32 = 112.;
        const COL_MY: f32 = 80.;
        const COL_TB: f32 = 96.;
        const COL_CLOUD: f32 = 70.;
        let online = self.online.open;
        let header = h_flex()
            .h(px(32.))
            .px(px(16.))
            .bg(rgb(T.surface_2))
            .border_b_1()
            .border_color(rgb(T.border))
            .when(engine_badges, |header| {
                header.child(
                    div()
                        .w(px(COL_BADGE))
                        .flex_none()
                        .child(section_label(ui::i18n::t("Engine"))),
                )
            })
            .child(
                div()
                    .w(px(COL_MOVE))
                    .flex_none()
                    .child(section_label(ui::i18n::t("Move"))),
            )
            .child(
                div()
                    .w(px(COL_SF))
                    .flex_none()
                    .child(section_label(ui::i18n::t("Eval"))),
            )
            .when(show_wdl, |header| {
                header.child(
                    div()
                        .w(px(COL_WDL))
                        .flex_none()
                        .child(section_label("W / D / L")),
                )
            })
            .when(online, |header| {
                header
                    .child(
                        div()
                            .w(px(COL_TB))
                            .flex_none()
                            .child(section_label("Tablebase")),
                    )
                    .child(
                        div()
                            .w(px(COL_CLOUD))
                            .flex_none()
                            .child(section_label("Cloud")),
                    )
            })
            .child(picker)
            .when(my_moves, |header| {
                header
                    .child(
                        div()
                            .w(px(COL_MY))
                            .flex_none()
                            .child(section_label(ui::i18n::t("My games"))),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(section_label(ui::i18n::t("Repertoire"))),
                    )
            });
        let ply = self.analysed_ply();
        let pos = self.analysed_pos();
        let white_to_move = pos.turn().is_white();
        // Room for every requested line is kept while the engine runs, so lines
        // arriving one by one early in a search do not move what is below.
        // A position never has more lines than legal moves, however many are asked for.
        let legal = pos.legal_moves().len().max(1) as i64;
        let reserved = self
            .shown_lines(cx)
            .filter(|_| self.live.read(cx).on && self.live.read(cx).end.is_none())
            .map_or(0., |lines| lines.clamp(1, legal) as f32 * MOVE_ROW_H);
        let mut rows = v_flex().w_full().min_h(px(reserved));
        for (index, row) in self.rows.iter().enumerate() {
            let selected = self.selected.as_ref().is_some_and(|key| key.matches(row));
            let key = RowKey::of(row);
            let mv = row
                .san
                .parse::<chess_core::shakmaty::san::San>()
                .ok()
                .and_then(|parsed| parsed.to_move(&pos).ok());
            let shown = display_san(&row.san, mv.as_ref(), notation);
            let label = chess_core::notation::numbered(ply + 1, &shown);
            let label_color = row
                .quality
                .filter(|_| quality_arrows)
                .map_or(T.fg, quality_color);
            let select_this = this.clone();
            let score = row
                .score
                .map(Score::display)
                .unwrap_or_else(|| "—".to_owned());
            let badge = engine_badges.then(|| {
                let config = row.engine_id.and_then(|id| engine_configs.get(&id));
                engine_badge(config, index, COL_BADGE)
            });
            let stat_text = |stat: &Option<MoveStat>| {
                stat.as_ref()
                    .map(|stat| {
                        format!("{} ({:.0}%)", stat.games, stat.score_percent(white_to_move))
                    })
                    .unwrap_or_else(|| "—".to_owned())
            };
            let (rep_text, rep_color) = match row.rep {
                RepTag::Main => ("Main", T.good),
                RepTag::Side => ("Side line", T.fg_2),
                RepTag::Deviate => ("You deviate", T.warn),
                RepTag::None => ("—", T.muted),
            };
            let dash = || ("—".to_owned(), T.muted);
            let (tb_text, tb_color) = row.tb.as_ref().map_or_else(dash, online::tb_cell);
            let (cloud_text, cloud_color) =
                row.cloud.as_ref().map_or_else(dash, online::cloud_cell);
            // A count opens the Database tab on the games reaching the position after it.
            let count_target = explorer
                .as_ref()
                .filter(|_| row.reference.is_some())
                .zip(mv)
                .map(|(entry, mv)| {
                    let mut after = pos.clone();
                    after.play_unchecked(mv);
                    (entry.path.clone(), position_hash(&after), label.clone())
                });
            let workspace = self.workspace.clone();
            rows = rows.child(
                h_flex()
                    .id(("move-row", index))
                    .relative()
                    .h(px(MOVE_ROW_H))
                    .flex_none()
                    .px(px(16.))
                    .cursor_pointer()
                    .border_b_1()
                    .border_color(rgb(T.divider))
                    .text_size(px(14.))
                    .font_family(MONO)
                    .whitespace_nowrap()
                    .when(selected, |row_el| row_el.bg(rgb(T.selected)))
                    .when(!selected, |row_el| {
                        row_el.hover(|style| style.bg(rgb(T.hover)))
                    })
                    .on_click(move |_, _, cx| {
                        let key = key.clone();
                        select_this.update(cx, |screen, cx| screen.select_row(key, cx));
                    })
                    .when(selected, |row_el| {
                        row_el.child(
                            div()
                                .absolute()
                                .left_0()
                                .top_0()
                                .bottom_0()
                                .w(px(3.))
                                .bg(rgb(T.accent)),
                        )
                    })
                    .children(badge)
                    .child(
                        div()
                            .w(px(COL_MOVE))
                            .flex_none()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(label_color))
                            .child(ui::i18n::t(&label).to_owned()),
                    )
                    .child(
                        div()
                            .w(px(COL_SF))
                            .flex_none()
                            .text_size(px(13.))
                            .text_color(rgb(T.fg_2))
                            .child(score),
                    )
                    .when(show_wdl, |row_el| {
                        row_el.child(
                            div().w(px(COL_WDL)).flex_none().pr(px(12.)).child(
                                match wdl_percentages(row.wdl) {
                                    Some([w, d, l]) => {
                                        v_flex().gap(px(2.)).child(wdl_bar(row.wdl)).child(
                                            mono(format!("{w:.0} / {d:.0} / {l:.0}%"))
                                                .text_size(px(10.))
                                                .text_color(rgb(T.fg_2)),
                                        )
                                    }
                                    None => mono("—").text_size(px(13.)).text_color(rgb(T.fg_2)),
                                },
                            ),
                        )
                    })
                    .when(online, |row_el| {
                        row_el
                            .child(
                                div()
                                    .w(px(COL_TB))
                                    .flex_none()
                                    .pr(px(8.))
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .font_family(ui::SANS)
                                    .text_size(px(12.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(rgb(tb_color))
                                    .child(tb_text),
                            )
                            .child(
                                div()
                                    .w(px(COL_CLOUD))
                                    .flex_none()
                                    .text_size(px(13.))
                                    .text_color(rgb(cloud_color))
                                    .child(cloud_text),
                            )
                    })
                    .child(
                        div()
                            .id(("reference-count", index))
                            .w(px(COL_REF))
                            .flex_none()
                            .pl(px(4.))
                            .text_size(px(13.))
                            .text_color(rgb(T.fg_2))
                            .when_some(count_target, |cell, (path, hash, label)| {
                                cell.hover(|style| style.text_color(rgb(T.accent)).underline())
                                    .tooltip(|window, cx| {
                                        gpui_component::tooltip::Tooltip::new(ui::i18n::t(
                                            "Show these games in the Database tab",
                                        ))
                                        .build(window, cx)
                                    })
                                    .on_click(move |_, window, cx| {
                                        cx.stop_propagation();
                                        let (path, label) = (path.clone(), label.clone());
                                        if let Some(workspace) = workspace.upgrade() {
                                            workspace.update(cx, |workspace, cx| {
                                                workspace.show_position_in_database(
                                                    path,
                                                    hash,
                                                    ui::i18n::tf("After {}", &[&label]),
                                                    window,
                                                    cx,
                                                );
                                            });
                                        }
                                    })
                            })
                            .child(stat_text(&row.reference)),
                    )
                    .when(my_moves, |row_el| {
                        row_el
                            .child(
                                div()
                                    .w(px(COL_MY))
                                    .flex_none()
                                    .text_size(px(13.))
                                    .text_color(rgb(T.fg_2))
                                    .child(stat_text(&row.mine)),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .font_family(ui::SANS)
                                    .text_size(px(12.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(rgb(rep_color))
                                    .child(rep_text),
                            )
                    }),
            );
        }
        if !self.rows.is_empty() && self.reference.is_empty() && explorer.is_some() {
            rows = rows.child(
                div()
                    .px(px(16.))
                    .py(px(8.))
                    .text_size(px(12.))
                    .text_color(rgb(T.muted))
                    .child(ui::i18n::t("No games reach this position")),
            );
        }
        if self.rows.is_empty() {
            rows = rows.child(
                div()
                    .px(px(16.))
                    .py(px(20.))
                    .text_size(px(13.))
                    .text_color(rgb(T.muted))
                    .child(if model.analysis_engines().is_empty() {
                        "No candidate moves yet. Add an engine in Settings › Engines, or import a database."
                    } else {
                        "Waiting for engine lines…"
                    }),
            );
        }
        let (toggles, settings) = self.render_table_tools(cx);
        panel()
            .rounded_none()
            .flex_none()
            .overflow_hidden()
            .child(
                v_flex().px(px(16.)).py(px(10.)).gap(px(8.)).child(
                    h_flex()
                        .flex_wrap()
                        .gap(px(8.))
                        .child(div().flex_1().child(settings))
                        .child(toggles),
                ),
            )
            .children(self.render_online_status())
            .child(
                div()
                    .id("candidate-table-scroll")
                    .overflow_x_scroll()
                    .child(
                        v_flex()
                            .min_w(px(col_lead
                                + COL_MOVE
                                + COL_SF
                                + if show_wdl { COL_WDL } else { 0. }
                                + if online { COL_TB + COL_CLOUD } else { 0. }
                                + COL_REF
                                + COL_MY
                                + 120.))
                            .child(header)
                            .child(rows),
                    ),
            )
    }

    /// After the scoresheet is laid out, scrolls just far enough to show the current move
    /// when it has changed, and leaves the scroll position alone otherwise.
    fn scoresheet_follower(&self) -> impl IntoElement + use<> {
        let scroll = self.scoresheet_scroll.clone();
        let current = self.scoresheet_current.clone();
        let follow = self.scoresheet_follow.clone();
        gpui::canvas(
            move |_, window, _| {
                if !follow.replace(false) {
                    return;
                }
                let mut offset = scroll.offset();
                let Some(pill) = current.get() else {
                    // The start position: show the top of the sheet.
                    if offset.y != px(0.) {
                        scroll.set_offset(gpui::point(offset.x, px(0.)));
                        window.refresh();
                    }
                    return;
                };
                let view = scroll.bounds();
                // Keep a row of context above and below the current move.
                let margin = px(28.);
                let top = view.top() + margin;
                let bottom = view.bottom() - margin;
                let shift = if pill.top() < top {
                    top - pill.top()
                } else if pill.bottom() > bottom {
                    bottom - pill.bottom()
                } else {
                    return;
                };
                let max = scroll.max_offset().y;
                offset.y = (offset.y + shift).clamp(-max, px(0.));
                scroll.set_offset(offset);
                window.refresh();
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_0()
    }

    /// Who found the selected line and how deep: the engine's icon and name, then the
    /// line's depth.
    fn render_line_source(&self, row: &MoveRow, cx: &gpui::App) -> gpui::AnyElement {
        let live = self.live.read(cx);
        let engine = row
            .engine_id
            .and_then(|id| live.engines.iter().find(|engine| engine.config.id == id));
        let stats = engine.and_then(|engine| engine.stats());
        let depth = match stats.and_then(|stats| stats.seldepth) {
            Some(sel) if sel > row.depth => format!("{}/{sel}", row.depth),
            _ => row.depth.to_string(),
        };
        // The Engine panel shows the search's other figures.
        let figures: Vec<(&'static str, String)> = vec![("Depth", depth)];
        let name = engine.map(|engine| {
            let config = &engine.config;
            format!("{} {}", config.name, config.version)
                .trim()
                .to_owned()
        });
        // Each figure is a tiny caption over its value, like the Engine panel's columns.
        let figure = |(label, value): (&'static str, String)| {
            v_flex()
                .flex_none()
                .child(
                    div()
                        .text_size(px(10.))
                        .text_color(rgb(T.faint))
                        .child(ui::i18n::t(label).to_uppercase()),
                )
                .child(
                    mono(value)
                        .whitespace_nowrap()
                        .text_size(px(12.))
                        .text_color(rgb(T.fg_2)),
                )
        };
        h_flex()
            .flex_wrap()
            .gap_x(px(14.))
            .gap_y(px(4.))
            .items_center()
            .text_size(px(12.))
            .children(engine.map(|engine| {
                h_flex()
                    .flex_none()
                    .gap(px(6.))
                    .child(crate::controls::engine_icon(&engine.config, 14.))
                    .children(
                        name.clone()
                            .map(|name| div().text_color(rgb(T.fg_2)).child(name)),
                    )
            }))
            .children(figures.into_iter().map(figure))
            .into_any_element()
    }

    fn render_line_preview(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let this = cx.entity();
        let explorer = self.explorer_database(cx);
        let model = self.model.read(cx);
        let demo = model.demo;
        // Database names are often lowercase; this label is not uppercased like the table's.
        let reference_title = explorer
            .map(|entry| {
                let title = reference_column_title(&entry.name);
                let mut chars = title.chars();
                chars
                    .next()
                    .map(|first| first.to_uppercase().chain(chars).collect())
                    .unwrap_or(title)
            })
            .unwrap_or_else(|| ui::i18n::t("Reference").to_owned());
        let ply = self.analysed_ply();
        let white_to_move = self.analysed_pos().turn().is_white();
        let selected = self.selected_row().cloned();
        let meta = selected
            .as_ref()
            .filter(|row| row.depth > 0)
            .map(|row| self.render_line_source(row, cx));
        // The move itself, large, with its score and how it compares with the best line.
        let headline = selected.as_ref().map(|row| {
            let score = row.score.map(|score| {
                h_flex()
                    .flex_none()
                    .h(px(26.))
                    .px(px(8.))
                    .rounded(px(4.))
                    .bg(rgb(T.fg))
                    .text_color(rgb(T.bg))
                    .child(
                        mono(score.display())
                            .text_size(px(13.))
                            .font_weight(FontWeight::SEMIBOLD),
                    )
            });
            let verdict = row.quality.map(|quality| {
                let text = match quality {
                    Quality::Best => "Best move",
                    Quality::Good => "Good",
                    Quality::Inaccuracy => "Inaccuracy",
                    Quality::Mistake => "Mistake",
                    Quality::Blunder => "Blunder",
                };
                div()
                    .flex_none()
                    .text_size(px(12.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(rgb(if quality == Quality::Best {
                        T.muted
                    } else {
                        quality_color(quality)
                    }))
                    .child(ui::i18n::t(text).to_owned())
            });
            h_flex()
                .mt(px(14.))
                .gap(px(10.))
                .children(score)
                .child(
                    mono(chess_core::notation::numbered(ply + 1, &row.san))
                        .flex_1()
                        .min_w_0()
                        .text_size(px(18.))
                        .font_weight(FontWeight::SEMIBOLD),
                )
                .children(verdict)
        });
        let wdl = self.live.read(cx).wdl_engine().map(|_| {
            let wdl = selected.as_ref().and_then(|row| row.wdl);
            let labels = match wdl_percentages(wdl) {
                Some([w, d, l]) => h_flex()
                    .justify_between()
                    .child(format!("{} {w:.1}%", ui::i18n::t("White")))
                    .child(format!("{} {d:.1}%", ui::i18n::t("Draw")))
                    .child(format!("{} {l:.1}%", ui::i18n::t("Black"))),
                None => h_flex().child(ui::i18n::t("Waiting for WDL data")),
            };
            let bar = if wdl_percentages(wdl).is_some() {
                wdl_bar(wdl)
            } else {
                div()
                    .h(px(10.))
                    .w_full()
                    .rounded(px(1.))
                    .bg(rgb(T.divider))
                    .into_any_element()
            };
            v_flex()
                .gap(px(6.))
                .mt(px(12.))
                .child(bar)
                .child(labels.text_size(px(12.)).text_color(rgb(T.muted)))
        });
        // Hook for future move explanations; only demo mode has text today.
        let explanation = selected
            .as_ref()
            .filter(|_| demo)
            .and_then(|row| crate::demo::explanation(&row.san))
            .map(|text| {
                div()
                    .mt(px(12.))
                    .text_size(px(13.))
                    .line_height(px(19.))
                    .text_color(rgb(T.fg_2))
                    .child(text)
            });
        // What the databases and the repertoire say about the selected move.
        // A game count and the score, as two figures side by side.
        let stat_text = |stat: Option<&MoveStat>| -> gpui::AnyElement {
            match stat {
                None => div()
                    .child(ui::i18n::t("No games").to_owned())
                    .into_any_element(),
                Some(stat) => h_flex()
                    .gap(px(12.))
                    .child(format!("{} {}", stat.games, ui::i18n::t("games")))
                    .child(mono(format!("{:.0}%", stat.score_percent(white_to_move))))
                    .into_any_element(),
            }
        };
        let (rep_text, rep_color) = match selected.as_ref().map_or(RepTag::None, |row| row.rep) {
            RepTag::Main => ("Main", T.good),
            RepTag::Side => ("Side line", T.fg_2),
            RepTag::Deviate => ("You deviate", T.warn),
            RepTag::None => ("Not in repertoire", T.fg_2),
        };
        let fact = |title: String, value: gpui::AnyElement, color: u32| {
            v_flex()
                .flex_1()
                .min_w_0()
                .gap(px(3.))
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(rgb(T.muted))
                        .overflow_hidden()
                        .text_ellipsis()
                        .whitespace_nowrap()
                        .child(title),
                )
                .child(
                    div()
                        .text_size(px(13.))
                        .text_color(rgb(color))
                        .whitespace_nowrap()
                        .child(value),
                )
        };
        let my_moves = self.model.read(cx).config.view.my_moves;
        let facts = selected.as_ref().map(|row| {
            h_flex()
                .mt(px(16.))
                .pt(px(12.))
                .gap(px(12.))
                .border_t_1()
                .border_color(rgb(T.divider))
                .child(fact(
                    reference_title.clone(),
                    stat_text(row.reference.as_ref()),
                    T.fg_2,
                ))
                .when(my_moves, |facts| {
                    facts
                        .child(fact(
                            ui::i18n::t("My games").to_owned(),
                            stat_text(row.mine.as_ref()),
                            T.fg_2,
                        ))
                        .child(fact(
                            ui::i18n::t("Repertoire").to_owned(),
                            div()
                                .child(ui::i18n::t(rep_text).to_owned())
                                .into_any_element(),
                            rep_color,
                        ))
                })
        });
        // A previewed line counts even where the table is empty, such as after the
        // mating move, so the whole line can still be played.
        let has_row = self.action_line().is_some();
        let pv = (selected.is_some() || self.pv_view.is_some())
            .then(|| self.render_pv(selected.as_ref(), cx));
        panel()
            .rounded_none()
            .flex_none()
            .p(px(16.))
            .child(
                h_flex()
                    .flex_wrap()
                    .gap(px(8.))
                    .justify_between()
                    .child(section_label("Line preview"))
                    .children(meta),
            )
            .children(headline)
            .children(wdl)
            .child(
                div()
                    .mt(px(12.))
                    .h(px(PV_INSET_H))
                    .flex_none()
                    .px(px(10.))
                    .py(px(6.))
                    .bg(rgb(T.bg))
                    .border_1()
                    .border_color(rgb(T.divider))
                    .when(pv.is_none(), |inset| {
                        inset.child(
                            div()
                                .py(px(4.))
                                .text_size(px(13.))
                                .text_color(rgb(T.muted))
                                .child(ui::i18n::t(
                                    "Click a move in the table to preview its line",
                                )),
                        )
                    })
                    .children(pv),
            )
            .children(self.render_kibitz_row(cx))
            .children(explanation)
            .child(
                h_flex()
                    .mt(px(12.))
                    .gap(px(6.))
                    .child(
                        btn("play-line", "Play on board")
                            .flex_1()
                            .pad(9.)
                            .disabled(!has_row)
                            .tooltip(ui::i18n::t("Play line on board"))
                            .on_click({
                                let this = this.clone();
                                move |window, cx| {
                                    this.update(cx, |screen, cx| screen.play_line(window, cx))
                                }
                            }),
                    )
                    .child(
                        btn("add-variation", "Add variation")
                            .flex_1()
                            .pad(9.)
                            .disabled(!has_row)
                            .tooltip(ui::i18n::t("Add as variation"))
                            .on_click({
                                let this = this.clone();
                                move |_, cx| this.update(cx, |screen, cx| screen.add_variation(cx))
                            }),
                    )
                    .child(
                        btn("add-rep", "To repertoire")
                            .flex_1()
                            .pad(9.)
                            .disabled(!has_row)
                            .tooltip(ui::i18n::t("Add to repertoire"))
                            .on_click({
                                let this = this.clone();
                                move |_, cx| {
                                    this.update(cx, |screen, cx| screen.add_to_repertoire(cx))
                                }
                            }),
                    )
                    .child(
                        icon_btn("copy-line", "icons/copy.svg")
                            .w(36.)
                            .pad(0.)
                            .disabled(!has_row)
                            .tooltip(ui::i18n::t("Copy line"))
                            .on_click({
                                let this = this.clone();
                                move |_, cx| this.update(cx, |screen, cx| screen.copy_pv(cx))
                            }),
                    ),
            )
            .children(facts)
    }

    fn render_context_menu(&self, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        let (id, position) = self.context_menu?;
        let this = cx.entity();
        let item = |index: u32, label: &'static str, handler: MenuHandler| {
            div()
                .id(("context-item", index))
                .px(px(12.))
                .py(px(7.))
                .text_size(px(14.))
                .cursor_pointer()
                .hover(|style| style.bg(rgb(T.hover)))
                .on_click(move |_, window, cx| handler(window, cx))
                .child(ui::i18n::t(label).to_owned())
        };
        let delete_this = this.clone();
        let delete_sidelines_this = this.clone();
        let promote_this = this.clone();
        let copy_this = this.clone();
        Some(deferred(
            anchored().position(position).child(
                v_flex()
                    .id("scoresheet-context")
                    // Keep the parent dismissal handler from removing the menu
                    // on mouse-down before its items receive their click.
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .w(px(190.))
                    .py(px(4.))
                    .bg(rgb(T.surface))
                    .border_1()
                    .border_color(rgb(T.border_strong))
                    .rounded(px(6.))
                    .on_mouse_down_out(cx.listener(|screen, _, _, cx| {
                        screen.context_menu = None;
                        cx.notify();
                    }))
                    .child(item(
                        0,
                        ui::i18n::t("Delete from here"),
                        Box::new(move |window, cx| {
                            delete_this.update(cx, |screen, cx| screen.delete_from(id, window, cx));
                        }),
                    ))
                    .child(item(
                        3,
                        ui::i18n::t("Delete all sidelines"),
                        Box::new(move |window, cx| {
                            delete_sidelines_this.update(cx, |screen, cx| {
                                let selected = screen.tree.delete_all_sidelines(screen.current);
                                screen.context_menu = None;
                                if selected != screen.current {
                                    screen.current = selected;
                                    screen.position_changed(false, window, cx);
                                }
                                cx.notify();
                            });
                        }),
                    ))
                    .child(item(
                        1,
                        ui::i18n::t("Promote variation"),
                        Box::new(move |_, cx| {
                            promote_this.update(cx, |screen, cx| screen.promote(id, cx));
                        }),
                    ))
                    .child(item(
                        2,
                        ui::i18n::t("Copy PGN from here"),
                        Box::new(move |_, cx| {
                            copy_this.update(cx, |screen, cx| {
                                let text = write_pgn_from(&screen.annotated_tree(), id);
                                cx.write_to_clipboard(ClipboardItem::new_string(text));
                                screen.context_menu = None;
                                screen.model.toast("PGN copied.", cx);
                            });
                        }),
                    )),
            ),
        ))
    }
}

/// Read every selected file without losing successful games when another file fails.
fn read_game_files(paths: &[std::path::PathBuf]) -> (Vec<GameTree>, Vec<String>) {
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

/// Height of one row in Moves from here.
const MOVE_ROW_H: f32 = 46.;

/// Search time as "850 ms", "12.4 s" or "3:05".
fn search_time(ms: u64) -> String {
    match ms {
        0..1_000 => format!("{ms} ms"),
        1_000..60_000 => format!("{:.1} s", ms as f64 / 1000.),
        _ => format!("{}:{:02}", ms / 60_000, (ms / 1000) % 60),
    }
}

/// The engine a table row's line comes from: its icon and short name, cut to fit
/// `width`, with the full name on hover. Rows without an engine line keep the space.
fn engine_badge(
    config: Option<&engine::EngineConfig>,
    index: usize,
    width: f32,
) -> gpui::AnyElement {
    let Some(config) = config else {
        return div().w(px(width)).flex_none().into_any_element();
    };
    let full = format!("{} {}", config.name, config.version)
        .trim()
        .to_owned();
    h_flex()
        .id(("move-engine", index))
        .w(px(width))
        .flex_none()
        .pr(px(8.))
        .gap(px(6.))
        .tooltip(move |window, cx| {
            gpui_component::tooltip::Tooltip::new(full.clone()).build(window, cx)
        })
        .child(crate::controls::engine_icon(config, 14.))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .text_ellipsis()
                .font_family(ui::SANS)
                .text_size(px(12.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(rgb(T.fg_2))
                .child(short_engine(&config.name)),
        )
        .into_any_element()
}

fn short_engine(name: &str) -> String {
    if name.eq_ignore_ascii_case("stockfish") {
        "SF".to_owned()
    } else {
        name.to_owned()
    }
}

/// A database name short enough for the Reference column's heading: "Reference: OTB
/// masters" becomes "masters", other names stay whole and are truncated if needed.
fn explorer_label(name: &str) -> String {
    if name.contains(':') {
        reference_column_title(name)
    } else {
        name.to_owned()
    }
}

fn reference_column_title(name: &str) -> String {
    name.rsplit(' ')
        .next()
        .filter(|word| !word.is_empty())
        .unwrap_or("Reference")
        .to_owned()
}

/// Engine WDL is stored White-relative. Normalize by its reported total rather
/// than assuming every engine uses the UCI convention of 1000.
fn wdl_percentages(wdl: Option<Wdl>) -> Option<[f64; 3]> {
    let wdl = wdl?;
    let total = u64::from(wdl.win) + u64::from(wdl.draw) + u64::from(wdl.loss);
    (total > 0)
        .then(|| [wdl.win, wdl.draw, wdl.loss].map(|value| f64::from(value) * 100.0 / total as f64))
}

/// White win / draw / Black win. Missing data is shown explicitly.
fn wdl_bar(wdl: Option<Wdl>) -> gpui::AnyElement {
    let Some(wdl) = wdl.filter(|wdl| wdl_percentages(Some(*wdl)).is_some()) else {
        return div()
            .text_size(px(11.))
            .text_color(rgb(T.muted))
            .child("—")
            .into_any_element();
    };
    h_flex()
        .h(px(10.))
        .w_full()
        .overflow_hidden()
        .rounded(px(1.))
        .children(
            [
                (wdl.win, 0xE8EAED),
                (wdl.draw, 0x7F8893),
                (wdl.loss, 0x262B33),
            ]
            .into_iter()
            .filter(|(value, _)| *value > 0)
            .map(|(value, color)| {
                div()
                    .h_full()
                    .flex_grow(value as f32)
                    .flex_basis(px(0.))
                    .min_w(px(0.))
                    .bg(rgb(color))
            }),
        )
        .into_any_element()
}

impl Render for AnalyseScreen {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // The game left open last time comes back as it was.
        if let Some(recovery) = self.recovery.take() {
            self.load_text(&recovery.pgn, window, cx);
        }
        self.autosave(cx);
        let node = self.tree.node(self.current);
        let explicit = node
            .nags
            .iter()
            .find_map(|nag| ui::board::MoveTag::from_nag(*nag));
        let inferred = node.parent.and_then(|parent| {
            let (evals, _) = self.line_evals(&[parent, self.current], cx);
            let sign = if node.pos.turn().is_black() { 1 } else { -1 };
            match engine::classify_move(Score::Cp(evals[0]? * sign), Score::Cp(evals[1]? * sign)) {
                Quality::Inaccuracy => Some(ui::board::MoveTag::Inaccuracy),
                Quality::Mistake => Some(ui::board::MoveTag::Mistake),
                Quality::Blunder => Some(ui::board::MoveTag::Blunder),
                _ => None,
            }
        });
        let tag = if self.model.read(cx).config.board.show_move_badges
            && self.editor.is_none()
            && self.pv_view.is_none()
            && self.pv_hover.is_none()
        {
            explicit.or(inferred)
        } else {
            None
        };
        self.board
            .update(cx, |board, cx| board.set_move_tag(tag, cx));
        let viewport = window.viewport_size();
        let panels = self.model.read(cx).config.panels.clone();
        let collapsed = panels.scoresheet_collapsed;
        let scoresheet_width = if collapsed {
            SCORESHEET_RAIL
        } else {
            (panels.scoresheet_width as f32)
                .clamp(180., 480.)
                .min((f32::from(viewport.width) - 720.).max(180.))
        };
        // The side column keeps room for its table, so the board gives way first.
        self.scale = window.scale_factor();
        let board_size = ui::board::snap_board_size(
            (viewport.height - px(416.))
                .min(viewport.width - px(scoresheet_width + 420.))
                .min(px(panels.board_size.unwrap_or(u32::MAX) as f32))
                .max(px(240.)),
            self.scale,
        );
        self.show_wdl = self.live.read(cx).wdl_engine().is_some();
        let scoresheet = if collapsed {
            self.render_scoresheet_rail(cx).into_any_element()
        } else {
            v_flex()
                .h_full()
                .child(self.render_scoresheet(cx))
                .into_any_element()
        };
        let board_column = self.render_board_column(board_size, cx);
        let table = self.render_moves_table(cx);
        let preview = self.render_line_preview(cx);
        let editor = self.editor.is_some().then(|| self.render_editor(cx));
        let engine_panel = self.render_engine_panel(cx);
        let log_panel = self.log_panel_open.then(|| self.render_log_panel(cx));
        self.sync_online(cx);
        let note = self.note.clone();
        let menu = self.render_context_menu(cx);
        h_flex()
            .items_start()
            .size_full()
            .child(
                div()
                    .id("scoresheet-panel")
                    .relative()
                    .w(px(scoresheet_width))
                    .h_full()
                    .flex_none()
                    .on_drag_move(cx.listener(
                        |screen, event: &gpui::DragMoveEvent<ResizeAnalysisPanel>, _, cx| {
                            if event.drag(cx).0 != AnalysisPanel::Scoresheet {
                                return;
                            }
                            let width = f32::from(event.event.position.x - event.bounds.origin.x)
                                .clamp(180., 480.)
                                .round() as u32;
                            screen.model.update(cx, |model, cx| {
                                model.config.panels.scoresheet_width = width;
                                model.schedule_config_save(cx);
                            });
                            cx.notify();
                        },
                    ))
                    .child(scoresheet)
                    .when(!collapsed, |panel| {
                        panel.child(panel_resize_handle(
                            "scoresheet-resize",
                            AnalysisPanel::Scoresheet,
                        ))
                    }),
            )
            .child(
                div()
                    .id("board-column")
                    .relative()
                    .on_drag_move(cx.listener(
                        |screen, event: &gpui::DragMoveEvent<ResizeAnalysisPanel>, _, cx| {
                            if event.drag(cx).0 != AnalysisPanel::Board {
                                return;
                            }
                            let width = (f32::from(event.event.position.x - event.bounds.origin.x)
                                - 56.)
                                .clamp(240., 1600.)
                                .round() as u32;
                            screen.model.update(cx, |model, cx| {
                                model.config.panels.board_size = Some(width);
                                model.schedule_config_save(cx);
                            });
                            cx.notify();
                        },
                    ))
                    .px(px(28.))
                    .h_full()
                    .flex_none()
                    .overflow_y_scroll()
                    .child(board_column)
                    .child(panel_resize_handle(
                        "board-panel-resize",
                        AnalysisPanel::Board,
                    )),
            )
            .child(
                div()
                    .id("moves-column")
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .overflow_y_scroll()
                    .pr(px(24.))
                    .pb(px(16.))
                    .child(
                        v_flex()
                            .gap(px(18.))
                            // The search changes the height of everything below the table, so the
                            // Engine panel goes first to keep its buttons still.
                            .children(editor)
                            .child(engine_panel)
                            .child(table)
                            .child(preview)
                            .children(log_panel)
                            .child(
                                panel()
                                    .rounded_none()
                                    .flex_none()
                                    .p(px(16.))
                                    .child(section_label("Position note").mb(px(10.)))
                                    .child(
                                        div().bg(rgb(T.bg)).rounded(px(4.)).child(
                                            gpui_component::input::Textarea::new(&note)
                                                .h(px(64.))
                                                .text_size(px(13.)),
                                        ),
                                    ),
                            ),
                    ),
            )
            .children(menu)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|screen, _, _, cx| {
                    let menus = screen.context_menu.take().is_some()
                        || std::mem::take(&mut screen.copy_menu);
                    if menus {
                        cx.notify();
                    }
                }),
            )
            .into_any_element()
    }
}

/// A White-relative evaluation as the scoresheet shows it beside a move: pawns
/// ("+0.35"), or a mate ("#3", "#-2", and "#" or "-#" when its distance
/// is not known).
fn eval_label(cp: i32, mate: Option<i32>) -> String {
    match mate {
        Some(mate) => format!("#{mate}"),
        None if cp >= chess_core::annotation::MATE_CP => "#".to_owned(),
        None if cp <= -chess_core::annotation::MATE_CP => "-#".to_owned(),
        None => Score::Cp(cp).display(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scoresheet_evals_are_short() {
        assert_eq!(eval_label(35, None), "+0.35");
        assert_eq!(eval_label(-120, None), "-1.20");
        assert_eq!(eval_label(2000, Some(3)), "#3");
        assert_eq!(eval_label(-2000, None), "-#");
    }

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

    fn stat(san: &str, games: u64) -> MoveStat {
        MoveStat {
            san: san.to_owned(),
            games,
            white_wins: games,
            draws: 0,
            black_wins: 0,
        }
    }

    fn opts(white_to_move: bool, user_to_move: bool) -> RowOptions {
        RowOptions {
            white_to_move,
            user_to_move,
            sort: LineSort::Eval,
        }
    }

    fn line(san: &str, cp: i32) -> MoveRow {
        MoveRow {
            san: san.to_owned(),
            score: Some(Score::Cp(cp)),
            ..Default::default()
        }
    }

    #[test]
    fn wdl_values_have_clear_white_relative_percentages() {
        let value = Wdl {
            win: 600,
            draw: 300,
            loss: 100,
        };
        assert_eq!(wdl_percentages(Some(value)), Some([60.0, 30.0, 10.0]));
        assert_eq!(
            wdl_percentages(Some(value.flip())),
            Some([10.0, 30.0, 60.0])
        );
        assert_eq!(
            wdl_percentages(Some(Wdl {
                win: 0,
                draw: 0,
                loss: 0
            })),
            None
        );
        assert_eq!(wdl_percentages(None), None);
        assert_eq!(
            wdl_percentages(Some(Wdl {
                win: 1,
                draw: 1,
                loss: 2
            })),
            Some([25.0, 25.0, 50.0])
        );
    }

    #[test]
    fn rows_without_an_eval_follow_the_tablebase_then_the_cloud() {
        use crate::online::{CloudMove, Outcome, TbRow};
        let tb = |san: &str, outcome| MoveRow {
            san: san.to_owned(),
            tb: Some(TbRow {
                san: san.to_owned(),
                outcome,
                dtz: None,
                dtm: None,
                is_mate: false,
                draw_reason: None,
            }),
            ..Default::default()
        };
        let cloud = |san: &str, score| MoveRow {
            san: san.to_owned(),
            cloud: Some(CloudMove {
                san: san.to_owned(),
                score,
                rank: 0,
                note: String::new(),
                winrate: None,
            }),
            ..Default::default()
        };
        let rows = merge_rows(
            vec![
                cloud("a3", -40),
                tb("Kd2", Outcome::Draw),
                cloud("e4", 35),
                tb("Rd8", Outcome::Win),
                line("Nf3", 20),
            ],
            vec![],
            vec![],
            &[],
            &[],
            opts(true, false),
        );
        let order: Vec<&str> = rows.iter().map(|row| row.san.as_str()).collect();
        assert_eq!(order, vec!["Nf3", "Rd8", "Kd2", "e4", "a3"]);
    }

    #[test]
    fn rows_are_the_union_sorted_by_eval_then_popularity() {
        let rows = merge_rows(
            vec![line("h4", 31), line("f4", 42)],
            vec![stat("f4", 412), stat("Kb1", 131), stat("Ng3", 47)],
            vec![stat("h4", 9)],
            &[("f4".to_owned(), true), ("Ng3".to_owned(), false)],
            &[],
            opts(true, true),
        );
        let order: Vec<&str> = rows.iter().map(|row| row.san.as_str()).collect();
        assert_eq!(order, vec!["f4", "h4", "Kb1", "Ng3"]);
        assert_eq!(rows[0].rep, RepTag::Main);
        assert_eq!(rows[1].rep, RepTag::Deviate);
        assert_eq!(rows[2].rep, RepTag::None);
        assert_eq!(rows[3].rep, RepTag::Side);
        assert_eq!(rows[0].reference.as_ref().unwrap().games, 412);
    }

    #[test]
    fn each_engine_line_keeps_its_own_row_and_shares_the_move_data() {
        let engine_line = |san: &str, cp: i32, id: EngineId| MoveRow {
            engine_id: Some(id),
            ..line(san, cp)
        };
        let rows = merge_rows(
            vec![
                engine_line("d4", 24, 1),
                engine_line("d4", 31, 2),
                engine_line("e4", 19, 2),
            ],
            vec![stat("d4", 95), stat("c4", 7)],
            vec![],
            &[],
            &[],
            opts(true, true),
        );
        let order: Vec<(&str, Option<EngineId>)> = rows
            .iter()
            .map(|row| (row.san.as_str(), row.engine_id))
            .collect();
        assert_eq!(
            order,
            vec![
                ("d4", Some(2)),
                ("d4", Some(1)),
                ("e4", Some(2)),
                ("c4", None),
            ]
        );
        assert_eq!(rows[0].reference.as_ref().unwrap().games, 95);
        assert_eq!(rows[1].reference.as_ref().unwrap().games, 95);
    }

    #[test]
    fn black_prefers_lower_white_relative_scores() {
        let rows = merge_rows(
            vec![line("a", 50), line("b", -50)],
            vec![],
            vec![],
            &[],
            &[],
            opts(false, false),
        );
        assert_eq!(rows[0].san, "b");
    }

    #[test]
    fn check_suffixes_merge_with_database_moves() {
        let rows = merge_rows(
            vec![line("Qh5+", 10)],
            vec![stat("Qh5", 3)],
            vec![],
            &[],
            &[],
            opts(true, true),
        );
        assert_eq!(rows.len(), 1);
    }

    #[test]
    fn no_repertoire_means_no_deviation_tag() {
        let rows = merge_rows(
            vec![],
            vec![],
            vec![stat("e4", 3)],
            &[],
            &[],
            opts(true, true),
        );
        assert_eq!(rows[0].rep, RepTag::None);
        // Nor on the opponent's turn, where the moves are not the user's choices.
        let rows = merge_rows(
            vec![],
            vec![],
            vec![stat("e5", 3)],
            &[("c5".to_owned(), true)],
            &[],
            opts(false, false),
        );
        assert_eq!(
            rows.iter().find(|row| row.san == "e5").unwrap().rep,
            RepTag::None
        );
    }

    #[test]
    fn search_times_read_naturally() {
        assert_eq!(search_time(850), "850 ms");
        assert_eq!(search_time(12_400), "12.4 s");
        assert_eq!(search_time(185_000), "3:05");
    }

    #[test]
    fn depth_sort_puts_deeper_lines_first() {
        let mut shallow = line("a", 90);
        shallow.depth = 10;
        let mut deep = line("b", 20);
        deep.depth = 30;
        let rows = merge_rows(
            vec![shallow, deep],
            vec![],
            vec![],
            &[],
            &[],
            RowOptions {
                sort: LineSort::Depth,
                ..opts(true, true)
            },
        );
        assert_eq!(rows[0].san, "b");
    }

    #[test]
    fn all_legal_moves_are_listed_once_after_analysed_ones() {
        let rows = merge_rows(
            vec![line("e4", 30)],
            vec![],
            vec![],
            &[],
            &["e4".to_owned(), "d4".to_owned(), "Nf3".to_owned()],
            opts(true, true),
        );
        let order: Vec<&str> = rows.iter().map(|row| row.san.as_str()).collect();
        assert_eq!(order, vec!["e4", "d4", "Nf3"]);
        assert!(rows[1].score.is_none());
    }

    #[test]
    fn lines_are_graded_against_the_best_for_the_side_to_move() {
        let rows = merge_rows(
            vec![line("a", -40), line("b", 300), line("c", 0)],
            vec![],
            vec![],
            &[],
            &[],
            opts(false, false),
        );
        // Black to move: -0.40 is best for Black, +3.00 throws the game away.
        assert_eq!(rows[0].san, "a");
        assert_eq!(rows[0].quality, Some(Quality::Best));
        assert_eq!(rows[2].san, "b");
        assert_eq!(rows[2].quality, Some(Quality::Blunder));
        assert!(rows[1].quality > Some(Quality::Best));
    }
}
