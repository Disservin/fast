//! Analyse: scoresheet, board with timeline, and the merged "Moves from here" table.

use std::{cell::RefCell, collections::HashMap, rc::Rc};

use chess_core::{
    GameTree, Move, NodeId, Position, classify, display_san, parse_pgn, position_hash, write_pgn,
    write_pgn_from,
};
use db::{Database, ImportControl, MoveStat, import_games};
use engine::{EngineId, LogLine, Quality, Score, Wdl};
use gpui::{
    AppContext as _, ClipboardItem, Context, Entity, FocusHandle, FontWeight, IntoElement,
    MouseButton, Pixels, Point, Render, ScrollHandle, SharedString, Subscription, Task, TextLayout,
    UniformListScrollHandle, WeakEntity, Window, anchored, deferred, div, prelude::*, px, rgb,
};
use gpui_component::input::{InputEvent, InputState, TextareaState};
use ui::{
    Arrow, BoardEvent, BoardView, MONO, T,
    scoresheet::{ScoresheetProps, scoresheet_body, scoresheet_inline},
    timeline::{TimelineProps, timeline},
    widgets::{btn, divider, h_flex, icon_btn, mono, panel, section_label, stat, v_flex},
};

use crate::{
    config::{GraphTab, LineSort, ScoresheetStyle},
    live::{LiveAnalysis, LiveEvent},
    model::{AppModel, ModelEvent},
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

/// One candidate move merged from engines, databases and the repertoire.
#[derive(Clone, Debug, Default)]
pub struct MoveRow {
    pub san: String,
    /// The score the table sorts by: the primary engine's, else another engine's.
    pub score: Option<Score>,
    /// Each analysis engine's score for this move, in `LiveAnalysis::engines` order.
    pub scores: Vec<Option<Score>>,
    pub wdl: Option<Wdl>,
    pub pv: Vec<String>,
    pub depth: u32,
    pub nodes: Option<u64>,
    pub nps: Option<u64>,
    /// The engine whose line this row previews.
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
    fn clean(san: &str) -> &str {
        san.trim_end_matches(['+', '#'])
    }
    let mut rows = engine_lines;
    let upsert = |rows: &mut Vec<MoveRow>, san: &str| -> usize {
        match rows.iter().position(|row| clean(&row.san) == clean(san)) {
            Some(index) => index,
            None => {
                rows.push(MoveRow {
                    san: san.to_owned(),
                    ..Default::default()
                });
                rows.len() - 1
            }
        }
    };
    for stat in reference {
        let index = upsert(&mut rows, &stat.san);
        rows[index].reference = Some(stat);
    }
    for stat in mine {
        let index = upsert(&mut rows, &stat.san);
        rows[index].mine = Some(stat);
    }
    for (san, main) in rep_moves {
        let index = upsert(&mut rows, san);
        rows[index].rep = if *main { RepTag::Main } else { RepTag::Side };
    }
    for san in legal {
        upsert(&mut rows, san);
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

pub struct AnalyseScreen {
    recovery: Option<crate::recovery::Recovery>,
    recovery_path: std::path::PathBuf,
    /// What the autosave file last received.
    recovery_pgn: String,
    /// Autosave is failing; reported once until it works again.
    autosave_failed: bool,
    model: Entity<AppModel>,
    workspace: WeakEntity<Workspace>,
    pub tree: GameTree,
    pub current: NodeId,
    board: Entity<BoardView>,
    live: Entity<LiveAnalysis>,
    rows: Vec<MoveRow>,
    selected_san: Option<String>,
    row_chosen: bool,
    reference: Vec<MoveStat>,
    mine: Vec<MoveStat>,
    evals: HashMap<NodeId, i32>,
    /// Search depth behind each eval, for the depth graph.
    depths: HashMap<NodeId, u32>,
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
    db_task: Option<Task<()>>,
    visible: bool,
    /// Show WDL when an engine requests or supplies it, regardless of window width.
    show_wdl: bool,
    engine_panel_open: bool,
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
        let note = cx.new(|cx| TextareaState::new(window, cx));
        let comment = cx.new(|cx| TextareaState::new(window, cx));
        let fen_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("Paste a FEN and press Enter"));
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
                LiveEvent::Eval(node, cp, depth) => {
                    // Keep the deeper of what is known: a game's annotations or the engine.
                    if this.depths.get(node).is_none_or(|known| *known <= *depth)
                        || !this.evals.contains_key(node)
                    {
                        this.evals.insert(*node, *cp);
                        this.depths.insert(*node, *depth);
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
                ModelEvent::Databases | ModelEvent::Repertoire => {
                    this.query_databases(cx);
                }
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
            tree,
            current,
            board,
            live,
            rows: Vec::new(),
            depths: HashMap::new(),
            last_child: HashMap::new(),
            selected_san: None,
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
            db_task: None,
            visible: false,
            show_wdl: true,
            engine_panel_open: false,
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

    pub fn set_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        self.visible = visible;
        self.live
            .update(cx, |live, cx| live.set_active(visible, cx));
        if !visible {
            self.stop_backward(None, cx);
        }
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
                    self.toast(
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
        self.board
            .update(cx, |board, cx| board.set_interactive(true, cx));
        let node = self.tree.node(self.current);
        let pos = node.pos.clone();
        let last = node.mv.as_ref().and_then(|mv| Some((mv.from()?, mv.to())));
        // A move made, or one step forward: the same moments the board animates.
        if animate && let Some(mv) = node.mv {
            let volume = self.model.read(cx).config.board.sound_level();
            crate::sound::play_move(
                self.model.read(cx).config.board.sound_profile,
                volume,
                &mv,
                &pos,
                self.tree.game_end(self.current),
            );
        }
        self.board
            .update(cx, |board, cx| board.set_position(pos, last, animate, cx));
        self.selected_san = None;
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
        self.scoresheet_scroll.scroll_to_item(0);
        cx.notify();
    }

    fn query_databases(&mut self, cx: &mut Context<Self>) {
        self.selected_san = self.selected_san.take();
        let hash = position_hash(&self.analysed_pos());
        let model = self.model.read(cx);
        let reference = model.reference_db().map(|entry| entry.path.clone());
        let mine = model.my_games().map(|entry| entry.path.clone());
        let player = model.config.player_name.clone();
        let task = cx.background_spawn(async move {
            let query = |path: Option<std::path::PathBuf>, player: &str| -> Vec<MoveStat> {
                let Some(path) = path else { return Vec::new() };
                Database::open(path)
                    .and_then(|db| {
                        if player.is_empty() {
                            db.moves_from(hash)
                        } else {
                            db.moves_from_player(hash, player)
                        }
                    })
                    .unwrap_or_else(|err| {
                        log::warn!("position query failed: {err}");
                        Vec::new()
                    })
            };
            (query(reference, ""), query(mine, &player))
        });
        // Dropping the previous task cancels a stale query.
        self.db_task = Some(cx.spawn(async move |this, cx| {
            let (reference, mine) = task.await;
            let _ = this.update(cx, |this, cx| {
                this.reference = reference;
                this.mine = mine;
                this.rebuild_rows(cx);
            });
        }));
    }

    fn rebuild_rows(&mut self, cx: &mut Context<Self>) {
        let white_to_move = self.analysed_pos().turn().is_white();
        let model = self.model.read(cx);
        let live = self.live.read(cx);
        let mut lines: Vec<MoveRow> = Vec::new();
        // Every engine's lines, merged by move. The primary engine goes first, so its
        // line is the one a row previews and its score the one the table sorts by.
        let count = live.engines.len();
        let primary_id = live.primary().map(|engine| engine.config.id);
        let mut order: Vec<usize> = (0..count).collect();
        order.sort_by_key(|&index| Some(live.engines[index].config.id) != primary_id);
        for index in order {
            let engine = &live.engines[index];
            for line in engine.lines.values() {
                let Some(san) = line.sans.first() else {
                    continue;
                };
                let row = match lines.iter().position(|row| &row.san == san) {
                    Some(found) => &mut lines[found],
                    None => {
                        lines.push(MoveRow {
                            san: san.clone(),
                            scores: vec![None; count],
                            wdl: None,
                            pv: line.sans.clone(),
                            depth: line.update.depth,
                            nodes: line.update.nodes,
                            nps: line.update.nps,
                            engine_id: Some(engine.config.id),
                            ..Default::default()
                        });
                        lines.last_mut().expect("just pushed")
                    }
                };
                row.scores[index] = line.score;
                row.score = row.score.or(line.score);
            }
        }
        if let Some(wdl_engine) = live.wdl_engine() {
            for line in wdl_engine.lines.values() {
                let Some(san) = line.sans.first() else {
                    continue;
                };
                if let Some(row) = lines.iter_mut().find(|row| &row.san == san) {
                    row.wdl = line.wdl;
                }
            }
        }
        let path = self.analysed_san_path();
        // Repertoires are stored as lines from the initial position.
        let from_start = self.tree.header("FEN").is_none();
        let rep_moves: Vec<(String, bool)> = model
            .repertoire
            .moves_after(&path)
            .filter(|_| from_start)
            .map(|moves| {
                moves
                    .iter()
                    .enumerate()
                    .map(|(rank, node)| (node.san.clone(), rank == 0))
                    .collect()
            })
            .unwrap_or_default();
        // Online moves join the rows of the same move rather than listing them twice.
        self.merge_online(&mut lines);
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
            self.mine.clone(),
            &rep_moves,
            &legal,
            RowOptions {
                white_to_move,
                user_to_move: white_to_move == model.repertoire.white,
                sort: view.sort,
            },
        );
        // The selection follows the best line until the user picks a row.
        if !self.row_chosen
            && let Some(first) = self.rows.iter().find(|row| !row.pv.is_empty())
        {
            self.selected_san = Some(first.san.clone());
        }
        self.update_arrows(cx);
        cx.notify();
    }

    fn update_arrows(&mut self, cx: &mut Context<Self>) {
        // A hovered move or a highlighted range is already drawn on the board.
        if self.pv_hover.is_some() || self.pv_view.as_ref().is_some_and(PvView::is_range) {
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
            graded.sort_by_key(|(_, quality)| std::cmp::Reverse(*quality));
            for (row, quality) in graded {
                if self.selected_san.as_deref() == Some(row.san.as_str()) {
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
        if let Some(san) = &self.selected_san
            && let Some(arrow) = arrow_for(san)
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
        // A different game: engines hear `ucinewgame` before its first search.
        self.live.update(cx, |live, _| live.game += 1);
        self.current = GameTree::ROOT;
        self.evals.clear();
        self.depths.clear();
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
                    this.update(cx, |screen, cx| screen.toast(errors.join("; "), cx));
                }
            });
        });
    }

    /// Loads text that is either a FEN or PGN, e.g. from a file or the clipboard.
    pub fn load_text(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        let text = text.trim();
        if text.is_empty() {
            self.toast("Nothing to load.", cx);
            return;
        }
        if GameTree::from_fen(text).is_ok() {
            self.load_fen(text, window, cx);
            return;
        }
        let count = parse_pgn(text).map(|games| games.len()).unwrap_or(0);
        self.load_pgn_at(text, 0, window, cx);
        if count > 1 {
            self.toast(
                format!("Loaded the first of {count} games. Import the file in Database to browse them all."),
                cx,
            );
        }
    }

    /// Loads a FEN or PGN from the clipboard.
    pub fn paste_game(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match cx.read_from_clipboard().and_then(|item| item.text()) {
            Some(text) => self.load_text(&text, window, cx),
            None => self.toast("The clipboard has no text.", cx),
        }
    }

    /// Writes the game, with variations and comments, to a PGN file.
    pub fn save_pgn_as(&mut self, cx: &mut Context<Self>) {
        let pgn = write_pgn(&self.tree);
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
            self.stop_backward(Some("Backwards analysis stopped."), cx);
            return;
        }
        if self.model.read(cx).analysis_engines().is_empty() {
            self.toast(
                "Add an engine for analysis first, in Settings › Engines.",
                cx,
            );
            return;
        }
        if self.model.read(cx).demo {
            self.toast(
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
                Some("Backwards analysis stopped: no engine is running."),
                cx,
            );
            return false;
        }
        if !idle {
            return true;
        }
        let Some(parent) = self.tree.node(self.current).parent else {
            self.stop_backward(Some("Backwards analysis finished."), cx);
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
            self.toast(message.to_owned(), cx);
        }
        cx.notify();
    }

    /// Opens the raw-output window for the engine the panels show.
    pub fn open_engine_output(&mut self, cx: &mut Context<Self>) {
        match self.panel_engine_id(cx) {
            Some(id) => crate::model::open_debug_window(&self.model, id, cx),
            None => self.toast("No engine yet. Add one in Settings › Engines.", cx),
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
            Ok(_) => self.toast("That PGN contains no games.", cx),
            Err(err) => self.toast(format!("Could not read the game: {err}"), cx),
        }
    }

    pub fn load_fen(&mut self, fen: &str, window: &mut Window, cx: &mut Context<Self>) {
        match GameTree::from_fen(fen) {
            Ok(tree) => self.load_tree(tree, window, cx),
            Err(err) => self.toast(format!("Invalid FEN: {err}"), cx),
        }
    }

    fn toast(&self, text: impl Into<SharedString>, cx: &mut Context<Self>) {
        let text = text.into();
        self.model.update(cx, |model, cx| model.toast(text, cx));
    }

    /// Copies the position on the board as FEN, a previewed line's included.
    fn copy_fen(&mut self, cx: &mut Context<Self>) {
        let fen = GameTree::fen_of(&self.analysed_pos());
        cx.write_to_clipboard(ClipboardItem::new_string(fen));
        self.copy_menu = false;
        self.toast("FEN copied.", cx);
    }

    /// Copies the whole game, headers and variations included.
    fn copy_pgn(&mut self, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(write_pgn(&self.tree)));
        self.copy_menu = false;
        self.toast("PGN copied.", cx);
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
                .child(label)
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
                    .tooltip("Copy FEN or PGN")
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
                    screen.toast(
                        format!("Board image copied and saved to {}", path.display()),
                        cx,
                    );
                }
                Err(err) => screen.toast(format!("Could not save the board image: {err}"), cx),
            });
        })
        .detach();
    }

    /// Saves the game to "My games".
    pub fn save_game(&mut self, cx: &mut Context<Self>) {
        let mut tree = self.tree.clone();
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
                    let summary =
                        import_games(&path, pgn.as_bytes(), "", &ImportControl::default(), |_| {})?;
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
        self.selected_san
            .as_ref()
            .and_then(|san| self.rows.iter().find(|row| &row.san == san))
    }

    fn select_row(&mut self, san: String, cx: &mut Context<Self>) {
        if self.continue_pv_with(&san, cx) {
            return;
        }
        self.row_chosen = true;
        self.selected_san = Some(san);
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

    fn render_scoresheet(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let this = cx.entity();
        let white = self
            .tree
            .header("White")
            .unwrap_or(ui::i18n::t("White"))
            .to_owned();
        let black = self
            .tree
            .header("Black")
            .unwrap_or(ui::i18n::t("Black"))
            .to_owned();
        let elo = |tag: &str| {
            self.tree
                .header(tag)
                .filter(|value| !value.is_empty() && *value != "?")
                .unwrap_or("—")
                .to_owned()
        };
        let white_elo = elo("WhiteElo");
        let black_elo = elo("BlackElo");
        let opening = self
            .tree
            .header("Opening")
            .map(str::to_owned)
            .or_else(|| self.opening().map(|o| format!("{} {}", o.eco, o.name)))
            .unwrap_or_else(|| "Starting position".to_owned());
        let name_row = |name: String, rating: String| {
            h_flex()
                .justify_between()
                .h(px(23.))
                .child(div().font_weight(FontWeight::SEMIBOLD).child(name))
                .child(mono(rating).text_color(rgb(T.muted)).text_size(px(14.)))
        };
        let notation = self.model.read(cx).config.board.notation;
        let select_this = this.clone();
        let context_this = this.clone();
        let props = ScoresheetProps {
            current: self.current,
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
                .h(22.)
                .pad(7.)
                .text(11.)
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
                            .child(style_toggle),
                    )
                    .child(name_row(white, white_elo))
                    .child(name_row(black, black_elo))
                    .child(
                        div()
                            .mt(px(1.))
                            .text_size(px(13.))
                            .text_color(rgb(T.muted))
                            .child(opening),
                    ),
            )
            .child(
                div()
                    .id("scoresheet-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.scoresheet_scroll)
                    .py(px(4.))
                    .child(sheet),
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
        let nav_width = if compact { 34. } else { 42. };
        let shown = self.analysed_pos();
        let turn = if shown.turn().is_white() {
            ui::i18n::t("White")
        } else {
            ui::i18n::t("Black")
        };
        let end = self.live.read(cx).end;
        let status = if self.editor.is_some() {
            "Edit position".to_owned()
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
            .unwrap_or_else(|| "–".to_owned());
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
            let strip = |side: chess_core::Color| {
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
            (self.editor.is_none()).then(|| {
                h_flex()
                    .gap(px(10.))
                    .children(strip(chess_core::Color::White))
                    .children(strip(chess_core::Color::Black))
            })
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
        let flipped = self.board.read(cx).is_flipped();
        let flip_tip = match self.model.read(cx).config.shortcuts.get("flip_board") {
            Some(key) => format!("Flip board ({})", key.to_uppercase()),
            None => ui::i18n::t("Flip board").to_owned(),
        };
        v_flex()
            .flex_none()
            .w(board_size)
            .child(
                h_flex()
                    .h(px(39.))
                    .mt(px(18.))
                    .mb(px(6.))
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
                        h_flex().gap(px(12.)).children(material).child(
                            mono(eval_text)
                                .text_size(px(18.))
                                .font_weight(FontWeight::SEMIBOLD),
                        ),
                    ),
            )
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
            .child(graph)
            .child(
                h_flex()
                    .mt(px(12.))
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
                            )),
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
                                    .tooltip("Edit position")
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
                                    .tooltip("Copy the board as an image and save it")
                                    .on_click({
                                        let this = this.clone();
                                        move |_, cx| {
                                            this.update(cx, |screen, cx| screen.snapshot_board(cx));
                                        }
                                    }),
                            ),
                    ),
            )
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
                height: 80.,
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
                    height: 80.,
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
                    height: 80.,
                    on_scrub,
                    empty: "No search depths yet.",
                })
                .into_any_element()
            }
        };
        v_flex()
            .child(
                h_flex()
                    .flex_wrap()
                    .gap(px(6.))
                    .mt(px(12.))
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
                                .h(22.)
                                .pad(7.)
                                .text(11.)
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

    fn render_moves_table(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let this = cx.entity();
        let model = self.model.read(cx);
        let notation = model.config.board.notation;
        let live = self.live.read(cx);
        let wdl_engine = live.wdl_engine();
        // One eval column per analysis engine, so engines can be compared move by move.
        let mut eval_titles: Vec<String> = live
            .engines
            .iter()
            .map(|engine| {
                let short = short_engine(&engine.config.name);
                // Long development version strings would not fit the column.
                let version = &engine.config.version;
                if version.len() <= 5 {
                    format!("{short} {version}").trim().to_owned()
                } else {
                    short
                }
            })
            .collect();
        if eval_titles.is_empty() {
            eval_titles.push(ui::i18n::t("Eval").to_owned());
        }
        let eval_columns = eval_titles.len();
        let wdl_title = wdl_engine
            .map(|engine| format!("{} W / D / L", short_engine(&engine.config.name)))
            .unwrap_or_else(|| "W / D / L".to_owned());
        let reference_title = model
            .reference_db()
            .map(|entry| reference_column_title(&entry.name))
            .unwrap_or_else(|| "Reference".to_owned());
        let show_wdl = self.show_wdl;
        let quality_arrows = model.config.view.quality_arrows;
        const COL_MOVE: f32 = 64.;
        const COL_SF: f32 = 66.;
        const COL_WDL: f32 = 112.;
        const COL_REF: f32 = 84.;
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
            .child(
                div()
                    .w(px(COL_MOVE))
                    .flex_none()
                    .child(section_label(ui::i18n::t("Move"))),
            )
            .children(eval_titles.iter().map(|title| {
                div()
                    .w(px(COL_SF))
                    .flex_none()
                    .pr(px(6.))
                    .overflow_hidden()
                    .child(section_label(title))
            }))
            .when(show_wdl, |header| {
                header.child(
                    div()
                        .w(px(COL_WDL))
                        .flex_none()
                        .child(section_label(&wdl_title)),
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
            .child(
                div()
                    .w(px(COL_REF))
                    .flex_none()
                    .pl(px(4.))
                    .child(section_label(&reference_title)),
            )
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
            );
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
            let selected = self.selected_san.as_deref() == Some(row.san.as_str());
            let san = row.san.clone();
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
            let scores: Vec<String> = (0..eval_columns)
                .map(|column| {
                    row.scores
                        .get(column)
                        .copied()
                        .flatten()
                        .map(Score::display)
                        .unwrap_or_else(|| "—".to_owned())
                })
                .collect();
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
                        let san = san.clone();
                        select_this.update(cx, |screen, cx| screen.select_row(san, cx));
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
                    .child(
                        div()
                            .w(px(COL_MOVE))
                            .flex_none()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(label_color))
                            .child(label),
                    )
                    .children(scores.into_iter().map(|score| {
                        div()
                            .w(px(COL_SF))
                            .flex_none()
                            .text_size(px(13.))
                            .text_color(rgb(T.fg_2))
                            .child(score)
                    }))
                    .when(show_wdl, |row_el| {
                        row_el.child(
                            div().w(px(COL_WDL)).flex_none().pr(px(12.)).child(
                                v_flex().gap(px(2.)).child(wdl_bar(row.wdl)).child(
                                    mono(
                                        wdl_percentages(row.wdl)
                                            .map(|[w, d, l]| format!("{w:.0} / {d:.0} / {l:.0}%"))
                                            .unwrap_or_else(|| "—".into()),
                                    )
                                    .text_size(px(10.))
                                    .text_color(rgb(T.fg_2)),
                                ),
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
                            .w(px(COL_REF))
                            .flex_none()
                            .pl(px(4.))
                            .text_size(px(13.))
                            .text_color(rgb(T.fg_2))
                            .child(stat_text(&row.reference)),
                    )
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
                    ),
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
        panel()
            .flex_none()
            .overflow_hidden()
            .child(
                h_flex()
                    .flex_wrap()
                    .min_h(px(50.))
                    .px(px(16.))
                    .py(px(8.))
                    .gap(px(8.))
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(16.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Moves from here"),
                    )
                    .child(self.render_table_tools(cx)),
            )
            .children(self.render_online_status())
            .child(
                div()
                    .id("candidate-table-scroll")
                    .overflow_x_scroll()
                    .child(
                        v_flex()
                            .min_w(px(COL_MOVE
                                + COL_SF * eval_columns as f32
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

    /// Who found the selected line and how hard it searched: the engine, then small
    /// labelled figures. Depth is the line's own; the rest describe the whole search.
    fn render_search_stats(&self, row: &MoveRow, cx: &gpui::App) -> gpui::AnyElement {
        let live = self.live.read(cx);
        let engine = row
            .engine_id
            .and_then(|id| live.engines.iter().find(|engine| engine.config.id == id));
        let stats = engine.and_then(|engine| engine.stats());
        let depth = match stats.and_then(|stats| stats.seldepth) {
            Some(sel) if sel > row.depth => format!("{}/{sel}", row.depth),
            _ => row.depth.to_string(),
        };
        let mut figures: Vec<(&'static str, String)> = vec![("Depth", depth)];
        let nodes = stats.and_then(|stats| stats.nodes).or(row.nodes);
        figures.extend(nodes.map(|nodes| ("Nodes", engine::format_nodes(nodes))));
        let nps = stats.and_then(|stats| stats.nps).or(row.nps);
        figures.extend(nps.map(|nps| ("Speed", engine::format_speed(nps))));
        if let Some(stats) = stats {
            figures.extend(stats.time_ms.map(|ms| ("Time", search_time(ms))));
            figures.extend(
                stats
                    .hashfull
                    .filter(|full| *full > 0)
                    .map(|full| ("Hash", format!("{:.0}%", f64::from(full) / 10.))),
            );
            figures.extend(
                stats
                    .tbhits
                    .filter(|hits| *hits > 0)
                    .map(|hits| ("TB", engine::format_nodes(hits).replace(" n", ""))),
            );
        }
        // Fixed widths, so changing figures never push each other around.
        let width = |label: &str| match label {
            "Depth" => 46.,
            "Nodes" | "Speed" => 74.,
            "Time" => 52.,
            _ => 40.,
        };
        let figure = |(label, value): (&'static str, String)| {
            v_flex()
                .w(px(width(label)))
                .flex_none()
                .gap(px(1.))
                .child(
                    div()
                        .text_size(px(9.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(T.faint))
                        .child(ui::i18n::t(label).to_uppercase()),
                )
                .child(mono(value).text_size(px(12.)).text_color(rgb(T.fg_2)))
        };
        h_flex()
            .gap(px(8.))
            .items_center()
            .children(figures.into_iter().map(figure))
            .into_any_element()
    }

    fn render_line_preview(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let this = cx.entity();
        let demo = self.model.read(cx).demo;
        let ply = self.analysed_ply();
        let selected = self.selected_row().cloned();
        let selected_label = selected
            .as_ref()
            .map(|row| chess_core::notation::numbered(ply + 1, &row.san))
            .unwrap_or_default();
        let meta = selected
            .as_ref()
            .filter(|row| row.depth > 0)
            .map(|row| self.render_search_stats(row, cx));
        let wdl = self.live.read(cx).wdl_engine().map(|engine| {
            let values = wdl_percentages(selected.as_ref().and_then(|row| row.wdl));
            let header = h_flex()
                .flex_wrap()
                .gap(px(6.))
                .text_size(px(12.))
                .text_color(rgb(T.fg_2))
                .child(
                    div()
                        .mr(px(4.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(engine.config.name.clone()),
                );
            let header = match values {
                Some([w, d, l]) => header
                    .child(stat("White win", format!("{w:.1}%")))
                    .child(stat("Draw", format!("{d:.1}%")))
                    .child(stat("Black win", format!("{l:.1}%"))),
                None => header.child("Waiting for WDL data"),
            };
            v_flex()
                .gap(px(6.))
                .mt(px(10.))
                .child(header)
                .child(wdl_bar(selected.as_ref().and_then(|row| row.wdl)))
        });
        // Hook for future move explanations; only demo mode has text today.
        let explanation = selected
            .as_ref()
            .filter(|_| demo)
            .and_then(|row| crate::demo::explanation(&row.san));
        // A previewed line counts even where the table is empty, such as after the
        // mating move, so the whole line can still be played.
        let has_row = self.action_line().is_some();
        panel()
            .flex_none()
            .p(px(16.))
            .child(
                h_flex()
                    .justify_between()
                    .child(
                        h_flex()
                            .gap(px(10.))
                            .child(section_label("Line preview"))
                            .child(mono(selected_label).font_weight(FontWeight::SEMIBOLD)),
                    )
                    .children(meta),
            )
            .children(wdl)
            .child(self.render_pv(selected.as_ref(), cx))
            .child(divider().bg(rgb(T.border)))
            .child(
                div()
                    .mt(px(12.))
                    .mb(px(12.))
                    .text_size(px(13.))
                    .line_height(px(19.))
                    .text_color(rgb(T.fg_2))
                    .children(explanation),
            )
            .child(
                h_flex()
                    .flex_wrap()
                    .gap(px(6.))
                    .child(
                        btn("play-line", "Play line on board")
                            .pad(9.)
                            .disabled(!has_row)
                            .on_click({
                                let this = this.clone();
                                move |window, cx| {
                                    this.update(cx, |screen, cx| screen.play_line(window, cx))
                                }
                            }),
                    )
                    .child(
                        btn("add-variation", "Add as variation")
                            .pad(9.)
                            .disabled(!has_row)
                            .on_click({
                                let this = this.clone();
                                move |_, cx| this.update(cx, |screen, cx| screen.add_variation(cx))
                            }),
                    )
                    .child(
                        btn("add-rep", "Add to repertoire")
                            .pad(9.)
                            .disabled(!has_row)
                            .on_click({
                                let this = this.clone();
                                move |_, cx| {
                                    this.update(cx, |screen, cx| screen.add_to_repertoire(cx))
                                }
                            }),
                    )
                    .child(
                        btn("copy-line", ui::i18n::t("Copy"))
                            .pad(9.)
                            .disabled(!has_row)
                            .on_click({
                                let this = this.clone();
                                move |_, cx| this.update(cx, |screen, cx| screen.copy_pv(cx))
                            }),
                    ),
            )
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
                .child(label)
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
                        "Delete from here",
                        Box::new(move |window, cx| {
                            delete_this.update(cx, |screen, cx| screen.delete_from(id, window, cx));
                        }),
                    ))
                    .child(item(
                        3,
                        "Delete all sidelines",
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
                        "Promote variation",
                        Box::new(move |_, cx| {
                            promote_this.update(cx, |screen, cx| screen.promote(id, cx));
                        }),
                    ))
                    .child(item(
                        2,
                        "Copy PGN from here",
                        Box::new(move |_, cx| {
                            copy_this.update(cx, |screen, cx| {
                                let text = write_pgn_from(&screen.tree, id);
                                cx.write_to_clipboard(ClipboardItem::new_string(text));
                                screen.context_menu = None;
                                screen.toast("PGN copied.", cx);
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

fn short_engine(name: &str) -> String {
    if name.eq_ignore_ascii_case("stockfish") {
        "SF".to_owned()
    } else {
        name.to_owned()
    }
}

fn reference_column_title(name: &str) -> String {
    name.rsplit([' ', '·'])
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
        let scoresheet_width = (panels.scoresheet_width as f32)
            .clamp(180., 480.)
            .min((f32::from(viewport.width) - 720.).max(180.));
        // The side column keeps room for its table, so the board gives way first.
        self.scale = window.scale_factor();
        let board_size = ui::board::snap_board_size(
            (viewport.height - px(376.))
                .min(viewport.width - px(scoresheet_width + 420.))
                .min(px(panels.board_size.unwrap_or(u32::MAX) as f32))
                .max(px(240.)),
            self.scale,
        );
        self.show_wdl = self.live.read(cx).wdl_engine().is_some();
        let scoresheet = v_flex().h_full().child(self.render_scoresheet(cx));
        let board_column = self.render_board_column(board_size, cx);
        let table = self.render_moves_table(cx);
        let preview = self.render_line_preview(cx);
        let editor = self.editor.is_some().then(|| self.render_editor(cx));
        let engine_panel = self.engine_panel_open.then(|| self.render_engine_panel(cx));
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
                    .child(panel_resize_handle(
                        "scoresheet-resize",
                        AnalysisPanel::Scoresheet,
                    )),
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
                    .pt(px(24.))
                    .pb(px(16.))
                    .child(
                        v_flex()
                            .gap(px(18.))
                            // The search changes the height of everything below the table, so the
                            // Engine panel goes above it to keep its buttons still.
                            .children(editor)
                            .children(engine_panel)
                            .child(table)
                            .child(preview)
                            .children(log_panel)
                            .child(
                                panel()
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
