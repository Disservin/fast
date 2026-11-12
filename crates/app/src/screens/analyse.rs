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
    widgets::{
        Listen, StyleExt, btn, col_heading, h_flex, icon_btn, mono, panel, section_label,
        table_header, v_flex,
    },
};

use crate::{
    config::{GraphTab, LineSort, ScoresheetStyle},
    live::{LiveAnalysis, LiveEvent},
    model::{AppModel, ConfigExt as _, DbEntry, ModelEvent, ToastExt as _},
    shell::Workspace,
};

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

pub(crate) use self::moves_table::search_time;
use self::moves_table::{reference_column_title, wdl_bar, wdl_percentages};
pub(crate) use self::rows::rows_for;
pub use self::rows::{MoveRow, RepTag, RowKey, RowOptions, merge_rows, quality_color};

mod backward;
mod board_column;
mod database_explorer;
mod editor;
mod files;
mod line_preview;
mod moves_table;
mod online;
mod panels;
mod rows;
mod scoresheet;
mod tree_edit;

use editor::PositionEditor;
use panels::{LOG_POLL, LogSelection, PvView};

type MenuHandler = Box<dyn Fn(&mut Window, &mut gpui::App)>;

/// Width of the folded scoresheet.
const SCORESHEET_RAIL: f32 = 44.;
/// The search depth both evals around a move need before live analysis may badge it.
const MIN_BADGE_DEPTH: u32 = 10;

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
    db_findings: Vec<(NodeId, crate::review::ReviewMark)>,
    pub tree: GameTree,
    pub current: NodeId,
    board: Entity<BoardView>,
    live: Entity<LiveAnalysis>,
    rows: Vec<MoveRow>,
    /// The table row shown in Line preview.
    selected: Option<RowKey>,
    row_chosen: bool,
    reference: Vec<MoveStat>,
    /// A failed local database lookup, shown instead of a misleading empty result.
    reference_error: Option<String>,
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
    editing_comment: Option<NodeId>,
    fen_input: Entity<InputState>,
    /// The search type and editable value shared with the Infinite toggle.
    limit_controls: Entity<panels::limit::SearchLimitControls>,
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
        let limit_controls = cx.new(|cx| {
            panels::limit::SearchLimitControls::new(model.clone(), live.clone(), window, cx)
        });
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
            reference_error: None,
            mine: Vec::new(),
            evals,
            note,
            note_hash: 0,
            comment,
            editing_comment: None,
            fen_input,
            limit_controls,
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
        self.model
            .edit_config(cx, |config| config.board.flipped = flipped);
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

    /// Opens the raw-output window for the engine the panels show.
    pub fn open_engine_output(&mut self, cx: &mut Context<Self>) {
        match self.panel_engine_id(cx) {
            Some(id) => crate::model::open_debug_window(&self.model, id, cx),
            None => self
                .model
                .toast("No engine yet. Add one in Settings › Engines.", cx),
        }
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
        self.model
            .edit_config(cx, |config| change(&mut config.view));
        self.rebuild_rows(cx);
    }

    /// The badge for the move just played: its own glyph, else the game review's finding,
    /// else a judgement from the engine evals before and after it. Early search results
    /// swing too much to judge by, so live evals count only from `MIN_BADGE_DEPTH`, and
    /// only while Settings › Board allows badges from live analysis. Evals written in the
    /// game itself have no depth and always count.
    fn move_tag(&self, cx: &gpui::App) -> Option<ui::board::MoveTag> {
        use ui::board::MoveTag;
        let tag_of = |quality: Quality| match quality {
            Quality::Inaccuracy => Some(MoveTag::Inaccuracy),
            Quality::Mistake => Some(MoveTag::Mistake),
            Quality::Blunder => Some(MoveTag::Blunder),
            _ => None,
        };
        let node = self.tree.node(self.current);
        if let Some(tag) = node.nags.iter().find_map(|nag| MoveTag::from_nag(*nag)) {
            return Some(tag);
        }
        if let Some((_, mark)) = self.db_findings.iter().find(|(id, _)| *id == self.current) {
            return tag_of(mark.quality);
        }
        let parent = node.parent?;
        let live_badges = self.model.read(cx).config.board.live_move_badges;
        let (evals, depths) = self.line_evals(&[parent, self.current], cx);
        let trusted =
            |depth: Option<u32>| depth.is_none_or(|depth| live_badges && depth >= MIN_BADGE_DEPTH);
        if !depths.iter().copied().all(trusted) {
            return None;
        }
        let sign = if node.pos.turn().is_black() { 1 } else { -1 };
        tag_of(engine::classify_move(
            Score::Cp(evals[0]? * sign),
            Score::Cp(evals[1]? * sign),
        ))
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
}

impl Render for AnalyseScreen {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // The game left open last time comes back as it was.
        if let Some(recovery) = self.recovery.take() {
            self.load_text(&recovery.pgn, window, cx);
        }
        self.autosave(cx);
        let tag = if self.model.read(cx).config.board.show_move_badges
            && self.editor.is_none()
            && self.pv_view.is_none()
            && self.pv_hover.is_none()
        {
            self.move_tag(cx)
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
