//! Analyse: scoresheet, board with timeline, and the merged "Moves from here" table.

use std::{collections::HashMap, rc::Rc};

use chess_core::{
    GameTree, Move, NodeId, Position, classify, display_san, format_pv, parse_pgn, position_hash,
    write_pgn, write_pgn_from,
};
use db::{Database, ImportControl, MoveStat, import_games};
use engine::{Score, Wdl};
use gpui::{
    AppContext as _, ClipboardItem, Context, Entity, FontWeight, IntoElement, MouseButton, Pixels,
    Point, Render, ScrollHandle, SharedString, Subscription, Task, Window, anchored, deferred, div,
    prelude::*, px, rgb,
};
use gpui_component::input::{InputEvent, InputState, TextareaState};
use ui::{
    Arrow, BoardEvent, BoardView, MONO, T,
    scoresheet::{ScoresheetProps, scoresheet_body},
    timeline::{TimelineProps, timeline},
    widgets::{btn, divider, h_flex, icon_btn, mono, panel, section_label, v_flex},
};

use crate::{
    controls::text_input,
    live::{LiveAnalysis, LiveEvent},
    model::{AppModel, ModelEvent},
};

/// One candidate move merged from engines, databases and the repertoire.
#[derive(Clone, Debug, Default)]
pub struct MoveRow {
    pub san: String,
    pub score: Option<Score>,
    pub wdl: Option<Wdl>,
    pub pv: Vec<String>,
    pub depth: u32,
    pub nps: Option<u64>,
    pub engine_name: String,
    pub reference: Option<MoveStat>,
    pub mine: Option<MoveStat>,
    pub rep: RepTag,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RepTag {
    Main,
    Side,
    Deviate,
    #[default]
    None,
}

/// Merges candidate sources into table rows, sorted by engine eval for the side to
/// move; moves with no eval follow, by popularity.
pub fn merge_rows(
    engine_lines: Vec<MoveRow>,
    reference: Vec<MoveStat>,
    mine: Vec<MoveStat>,
    rep_moves: &[(String, bool)],
    white_to_move: bool,
    user_to_move: bool,
) -> Vec<MoveRow> {
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
    rows.sort_by(|a, b| {
        let key = |row: &MoveRow| {
            row.score
                .map(|score| score.sort_key() * if white_to_move { 1 } else { -1 })
        };
        match (key(a), key(b)) {
            (Some(x), Some(y)) => y.cmp(&x),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => popularity(b).cmp(&popularity(a)),
        }
    });
    rows
}

type MenuHandler = Box<dyn Fn(&mut Window, &mut gpui::App)>;

pub struct AnalyseScreen {
    model: Entity<AppModel>,
    pub tree: GameTree,
    pub current: NodeId,
    board: Entity<BoardView>,
    live: Entity<LiveAnalysis>,
    rows: Vec<MoveRow>,
    selected_san: Option<String>,
    reference: Vec<MoveStat>,
    mine: Vec<MoveStat>,
    evals: HashMap<NodeId, i32>,
    note: Entity<TextareaState>,
    note_hash: u64,
    comment: Entity<TextareaState>,
    editing_comment: bool,
    fen_input: Entity<InputState>,
    setting_up: bool,
    context_menu: Option<(NodeId, Point<Pixels>)>,
    scoresheet_scroll: ScrollHandle,
    db_task: Option<Task<()>>,
    visible: bool,
    /// The W/D/L column is dropped when the window is too narrow for it.
    show_wdl: bool,
    _subscriptions: Vec<Subscription>,
}

impl AnalyseScreen {
    pub fn new(model: Entity<AppModel>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let board = cx.new(|_| BoardView::new("analyse-board"));
        let live = cx.new(|cx| LiveAnalysis::new(model.clone(), cx));
        let note = cx.new(|cx| TextareaState::new(window, cx));
        let comment = cx.new(|cx| TextareaState::new(window, cx));
        let fen_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("Paste a FEN and press Enter"));
        let subscriptions = vec![
            cx.subscribe_in(&board, window, |this, _, event, window, cx| match event {
                BoardEvent::Moved(mv) => this.play_move(*mv, window, cx),
            }),
            cx.subscribe_in(&live, window, |this, _, event, _, cx| match event {
                LiveEvent::Updated => this.rebuild_rows(cx),
                LiveEvent::Eval(node, cp) => {
                    this.evals.insert(*node, *cp);
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
            cx.subscribe_in(
                &model,
                window,
                |this, model, event, window, cx| match event {
                    ModelEvent::BoardPrefs => {
                        let prefs = model.read(cx).config.board.prefs();
                        this.board
                            .update(cx, |board, cx| board.set_prefs(prefs, cx));
                    }
                    ModelEvent::Databases | ModelEvent::Repertoire => {
                        this.query_databases(window, cx);
                    }
                    ModelEvent::Engines => {}
                },
            ),
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
        board.update(cx, |board, cx| board.set_prefs(prefs, cx));
        let mut this = Self {
            model,
            tree,
            current,
            board,
            live,
            rows: Vec::new(),
            selected_san: None,
            reference: Vec::new(),
            mine: Vec::new(),
            evals,
            note,
            note_hash: 0,
            comment,
            editing_comment: false,
            fen_input,
            setting_up: false,
            context_menu: None,
            scoresheet_scroll: ScrollHandle::new(),
            db_task: None,
            visible: true,
            show_wdl: true,
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
            None => "New game".into(),
        }
    }

    pub fn live_on(&self, cx: &gpui::App) -> bool {
        self.live.read(cx).on
    }

    pub fn set_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        self.visible = visible;
        self.live.update(cx, |live, cx| {
            if visible {
                live.search(cx);
            } else {
                live.pause();
            }
        });
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
    fn san_path(&self) -> Vec<String> {
        self.tree
            .path_to(self.current)
            .iter()
            .map(|&id| self.tree.node(id).san.clone())
            .collect()
    }

    fn position_changed(&mut self, animate: bool, window: &mut Window, cx: &mut Context<Self>) {
        let node = self.tree.node(self.current);
        let pos = node.pos.clone();
        let last = node.mv.as_ref().and_then(|mv| Some((mv.from()?, mv.to())));
        self.board
            .update(cx, |board, cx| board.set_position(pos, last, animate, cx));
        self.selected_san = None;
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
        self.query_databases(window, cx);
        self.scoresheet_scroll.scroll_to_item(0);
        cx.notify();
    }

    fn query_databases(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.selected_san = self.selected_san.take();
        let hash = position_hash(&self.tree.node(self.current).pos);
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
        let node = self.tree.node(self.current);
        let white_to_move = node.pos.turn().is_white();
        let model = self.model.read(cx);
        let live = self.live.read(cx);
        let mut lines = Vec::new();
        if let Some(primary) = live.primary() {
            for line in primary.lines.values() {
                let Some(san) = line.sans.first() else {
                    continue;
                };
                lines.push(MoveRow {
                    san: san.clone(),
                    score: line.score,
                    wdl: line.wdl,
                    pv: line.sans.clone(),
                    depth: line.update.depth,
                    nps: line.update.nps,
                    engine_name: primary.config.name.clone(),
                    ..Default::default()
                });
            }
        }
        if let Some(wdl_engine) = live.wdl_engine() {
            for line in wdl_engine.lines.values() {
                let Some(san) = line.sans.first() else {
                    continue;
                };
                if let Some(row) = lines.iter_mut().find(|row| &row.san == san) {
                    row.wdl = line.wdl.or(row.wdl);
                }
            }
        }
        let path = self.san_path();
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
        self.rows = merge_rows(
            lines,
            self.reference.clone(),
            self.mine.clone(),
            &rep_moves,
            white_to_move,
            white_to_move == model.repertoire.white,
        );
        if self.selected_san.is_none()
            && let Some(first) = self.rows.first().filter(|row| !row.pv.is_empty())
        {
            self.selected_san = Some(first.san.clone());
        }
        self.update_arrows(cx);
        cx.notify();
    }

    fn update_arrows(&mut self, cx: &mut Context<Self>) {
        let pos = self.tree.node(self.current).pos.clone();
        let arrows: Vec<Arrow> = self
            .selected_san
            .iter()
            .filter_map(|san| {
                let parsed: chess_core::shakmaty::san::San = san.parse().ok()?;
                let mv = parsed.to_move(&pos).ok()?;
                Some(Arrow {
                    from: mv.from()?,
                    to: mv.to(),
                })
            })
            .collect();
        self.board
            .update(cx, |board, cx| board.set_arrows(arrows, cx));
    }

    pub fn play_move(&mut self, mv: Move, window: &mut Window, cx: &mut Context<Self>) {
        self.current = self.tree.add_move(self.current, mv);
        self.position_changed(true, window, cx);
    }

    pub fn jump(&mut self, id: NodeId, window: &mut Window, cx: &mut Context<Self>) {
        let adjacent = self.tree.node(id).parent == Some(self.current);
        self.current = id;
        self.context_menu = None;
        self.position_changed(adjacent, window, cx);
    }

    pub fn step(&mut self, delta: i32, window: &mut Window, cx: &mut Context<Self>) {
        let node = self.tree.node(self.current);
        let target = if delta < 0 {
            node.parent
        } else {
            node.children.first().copied()
        };
        if let Some(target) = target {
            self.jump(target, window, cx);
        }
    }

    pub fn go_first(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.jump(GameTree::ROOT, window, cx);
    }

    pub fn go_last(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let end = self
            .tree
            .line_from(self.current)
            .last()
            .copied()
            .unwrap_or(self.current);
        self.jump(end, window, cx);
    }

    pub fn flip(&mut self, cx: &mut Context<Self>) {
        self.board.update(cx, |board, cx| {
            let flipped = board.is_flipped();
            board.set_flipped(!flipped, cx);
        });
    }

    pub fn toggle_engine(&mut self, cx: &mut Context<Self>) {
        self.live.update(cx, |live, cx| live.toggle(cx));
    }

    pub fn load_tree(&mut self, tree: GameTree, window: &mut Window, cx: &mut Context<Self>) {
        self.tree = tree;
        self.current = GameTree::ROOT;
        self.evals.clear();
        self.setting_up = false;
        self.position_changed(false, window, cx);
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
        self.selected_san = Some(san);
        self.update_arrows(cx);
        cx.notify();
    }

    /// Adds SAN moves as a line from the current node; returns the last node.
    fn add_line(&mut self, sans: &[String]) -> NodeId {
        let mut node = self.current;
        for san in sans {
            match self.tree.add_san(node, san) {
                Ok(next) => node = next,
                Err(err) => {
                    log::warn!("could not add {san}: {err}");
                    break;
                }
            }
        }
        node
    }

    fn play_line(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(pv) = self.selected_row().map(|row| row.pv.clone()) else {
            return;
        };
        let end = self.add_line(&pv);
        self.jump(end, window, cx);
    }

    fn add_variation(&mut self, cx: &mut Context<Self>) {
        let Some(pv) = self.selected_row().map(|row| row.pv.clone()) else {
            return;
        };
        self.add_line(&pv);
        cx.notify();
    }

    fn add_to_repertoire(&mut self, cx: &mut Context<Self>) {
        let Some(row) = self.selected_row().cloned() else {
            return;
        };
        let mut path = self.san_path();
        if row.pv.is_empty() {
            path.push(row.san.clone());
        } else {
            path.extend(row.pv.iter().cloned());
        }
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
        let white = self.tree.header("White").unwrap_or("White").to_owned();
        let black = self.tree.header("Black").unwrap_or("Black").to_owned();
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
            .or_else(|| self.opening().map(|o| format!("{} · {}", o.eco, o.name)))
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
        let nag_button = |id: &'static str, label: &'static str, nag: u8, this: Entity<Self>| {
            btn(id, label)
                .w(36.)
                .h(30.)
                .pad(0.)
                .on_click(move |_, cx| this.update(cx, |screen, cx| screen.annotate(nag, cx)))
        };
        v_flex()
            .w(px(300.))
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
                    .child(section_label("Scoresheet").mb(px(9.)))
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
                    .child(scoresheet_body(&self.tree, &props)),
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
                    .child(nag_button("nag-interesting", "!?", 5, this.clone()))
                    .child(btn("comment", "Comment").h(30.).pad(13.).on_click({
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
                    }))
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
        let node = self.tree.node(self.current);
        let turn = if node.pos.turn().is_white() {
            "White"
        } else {
            "Black"
        };
        let status = if node.pos.is_checkmate() {
            "Checkmate".to_owned()
        } else if node.pos.is_stalemate() {
            "Stalemate".to_owned()
        } else {
            format!("{turn} to move")
        };
        let after = (self.current != GameTree::ROOT)
            .then(|| format!("after {}", self.tree.move_label(self.current)));
        let live = self.live.read(cx);
        let eval_text = live
            .primary()
            .and_then(|engine| engine.lines.get(&1))
            .and_then(|line| line.score)
            .map(Score::display)
            .or_else(|| {
                self.evals
                    .get(&self.current)
                    .map(|cp| Score::Cp(*cp).display())
            })
            .unwrap_or_else(|| "–".to_owned());
        let ids = self.timeline_ids();
        let evals: Vec<Option<i32>> = ids.iter().map(|id| self.evals.get(id).copied()).collect();
        let current_index = ids.iter().position(|&id| id == self.current);
        let scrub_this = this.clone();
        let scrub_ids = ids.clone();
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
        let fen = self.tree.fen(self.current);
        let tool_pad = if compact { 5. } else { 13. };
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
                        mono(eval_text)
                            .text_size(px(18.))
                            .font_weight(FontWeight::SEMIBOLD),
                    ),
            )
            .child(div().size(board_size).flex_none().child(self.board.clone()))
            .child(
                section_label("Game timeline · eval per move")
                    .mt(px(16.))
                    .mb(px(6.)),
            )
            .child(timeline(TimelineProps {
                evals,
                current: current_index,
                on_scrub: Rc::new(move |index, window, cx| {
                    if let Some(&id) = scrub_ids.get(index) {
                        scrub_this.update(cx, |screen, cx| {
                            if screen.current != id {
                                screen.jump(id, window, cx);
                            }
                        });
                    }
                }),
            }))
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
                                "First move",
                                this.clone(),
                                Self::go_first,
                            ))
                            .child(nav(
                                "prev",
                                "icons/chevron-left.svg",
                                "Previous move",
                                this.clone(),
                                |s, w, c| s.step(-1, w, c),
                            ))
                            .child(nav(
                                "next",
                                "icons/chevron-right.svg",
                                "Next move",
                                this.clone(),
                                |s, w, c| s.step(1, w, c),
                            ))
                            .child(nav(
                                "last",
                                "icons/skip-forward.svg",
                                "Last move",
                                this.clone(),
                                Self::go_last,
                            )),
                    )
                    .child(
                        h_flex()
                            .gap(px(6.))
                            .child(btn("flip", "Flip").pad(tool_pad).on_click({
                                let this = this.clone();
                                move |_, cx| this.update(cx, |screen, cx| screen.flip(cx))
                            }))
                            .child(btn("setup", "Set up").pad(tool_pad).on_click({
                                let this = this.clone();
                                move |window, cx| {
                                    this.update(cx, |screen, cx| {
                                        screen.setting_up = !screen.setting_up;
                                        if screen.setting_up {
                                            screen.fen_input.update(cx, |input, cx| {
                                                input.focus(window, cx);
                                            });
                                        }
                                        cx.notify();
                                    });
                                }
                            }))
                            .child(btn("copy-fen", "Copy FEN").pad(tool_pad).on_click({
                                let this = this.clone();
                                move |_, cx| {
                                    cx.write_to_clipboard(ClipboardItem::new_string(fen.clone()));
                                    this.update(cx, |screen, cx| screen.toast("FEN copied.", cx));
                                }
                            })),
                    ),
            )
            .when(self.setting_up, |column| {
                column.child(div().mt(px(8.)).child(text_input(&self.fen_input)))
            })
    }

    fn render_moves_table(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let this = cx.entity();
        let model = self.model.read(cx);
        let notation = model.config.board.notation;
        let live = self.live.read(cx);
        let primary = live.primary();
        let wdl_engine = live.wdl_engine();
        let eval_title = primary
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
            .unwrap_or_else(|| "Eval".to_owned());
        let wdl_title = wdl_engine
            .map(|engine| format!("{} W / D / L", short_engine(&engine.config.name)))
            .unwrap_or_else(|| "W / D / L".to_owned());
        let reference_title = model
            .reference_db()
            .map(|entry| reference_column_title(&entry.name))
            .unwrap_or_else(|| "Reference".to_owned());
        let show_wdl = self.show_wdl;
        const COL_MOVE: f32 = 64.;
        const COL_SF: f32 = 56.;
        const COL_WDL: f32 = 88.;
        const COL_REF: f32 = 84.;
        const COL_MY: f32 = 80.;
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
                    .child(section_label("Move")),
            )
            .child(
                div()
                    .w(px(COL_SF))
                    .flex_none()
                    .child(section_label(&eval_title)),
            )
            .when(show_wdl, |header| {
                header.child(
                    div()
                        .w(px(COL_WDL))
                        .flex_none()
                        .child(section_label(&wdl_title)),
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
                    .child(section_label("My games")),
            )
            .child(div().flex_1().min_w_0().child(section_label("Repertoire")));
        let ply = self.tree.node(self.current).ply;
        let pos = self.tree.node(self.current).pos.clone();
        let white_to_move = pos.turn().is_white();
        let mut rows = v_flex().w_full();
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
            let select_this = this.clone();
            let score = row
                .score
                .map(Score::display)
                .unwrap_or_else(|| "—".to_owned());
            let stat_text = |stat: &Option<MoveStat>| {
                stat.as_ref()
                    .map(|stat| {
                        format!("{} · {:.0}%", stat.games, stat.score_percent(white_to_move))
                    })
                    .unwrap_or_else(|| "—".to_owned())
            };
            let (rep_text, rep_color) = match row.rep {
                RepTag::Main => ("Main", T.good),
                RepTag::Side => ("Side line", T.fg_2),
                RepTag::Deviate => ("You deviate", T.warn),
                RepTag::None => ("—", T.muted),
            };
            rows = rows.child(
                h_flex()
                    .id(("move-row", index))
                    .relative()
                    .h(px(46.))
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
                                .bg(rgb(T.fg)),
                        )
                    })
                    .child(
                        div()
                            .w(px(COL_MOVE))
                            .flex_none()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(T.fg))
                            .child(label),
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
                            div()
                                .w(px(COL_WDL))
                                .flex_none()
                                .pr(px(12.))
                                .child(wdl_bar(row.wdl)),
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
                    .h(px(50.))
                    .px(px(16.))
                    .text_size(px(16.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Moves from here"),
            )
            .child(header)
            .child(rows)
    }

    fn render_line_preview(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let this = cx.entity();
        let demo = self.model.read(cx).demo;
        let ply = self.tree.node(self.current).ply;
        let selected = self.selected_row().cloned();
        let selected_label = selected
            .as_ref()
            .map(|row| chess_core::notation::numbered(ply + 1, &row.san))
            .unwrap_or_default();
        let meta = selected.as_ref().filter(|row| row.depth > 0).map(|row| {
            let speed = row
                .nps
                .map(|nps| format!(" · {:.0} Mn/s", nps as f64 / 1_000_000.0))
                .unwrap_or_default();
            format!("{} · depth {}{speed}", row.engine_name, row.depth)
        });
        let pv = selected
            .as_ref()
            .map(|row| {
                if row.pv.is_empty() {
                    chess_core::notation::numbered(ply + 1, &row.san)
                } else {
                    format_pv(ply, &row.pv)
                }
            })
            .unwrap_or_default();
        // Hook for future move explanations; only demo mode has text today.
        let explanation = selected
            .as_ref()
            .filter(|_| demo)
            .and_then(|row| crate::demo::explanation(&row.san));
        let has_row = selected.is_some();
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
                    .children(meta.map(|text| {
                        div()
                            .text_size(px(13.))
                            .text_color(rgb(T.muted))
                            .child(text)
                    })),
            )
            .child(
                div()
                    .mt(px(10.))
                    .mb(px(12.))
                    .min_h(px(26.))
                    .font_family(MONO)
                    .text_size(px(14.))
                    .line_height(px(26.))
                    .text_color(rgb(T.fg))
                    .child(pv),
            )
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
                            .disabled(!has_row)
                            .on_click({
                                let this = this.clone();
                                move |_, cx| this.update(cx, |screen, cx| screen.add_variation(cx))
                            }),
                    )
                    .child(
                        btn("add-rep", "Add to repertoire")
                            .disabled(!has_row)
                            .on_click({
                                let this = this.clone();
                                move |_, cx| {
                                    this.update(cx, |screen, cx| screen.add_to_repertoire(cx))
                                }
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
        let promote_this = this.clone();
        let copy_this = this.clone();
        Some(deferred(
            anchored().position(position).child(
                v_flex()
                    .id("scoresheet-context")
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

/// The three-part win/draw/loss bar.
fn wdl_bar(wdl: Option<Wdl>) -> impl IntoElement {
    let bar = h_flex()
        .h(px(13.))
        .w_full()
        .overflow_hidden()
        .rounded(px(1.));
    let Some(wdl) = wdl else {
        return bar.bg(rgb(T.selected));
    };
    let segment = |value: u32, color: u32| {
        div()
            .h_full()
            .flex_grow(value.max(1) as f32)
            .flex_basis(px(0.))
            .min_w(px(0.))
            .bg(rgb(color))
    };
    bar.child(segment(wdl.win, T.fg))
        .child(segment(wdl.draw, T.bar))
        .child(segment(wdl.loss, 0x0B0D10))
}

impl Render for AnalyseScreen {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let viewport = window.viewport_size();
        // The side column keeps room for its table, so the board gives way first.
        let board_size = (viewport.height - px(324.))
            .min(viewport.width - px(820.))
            .max(px(320.));
        let side_width = viewport.width - board_size - px(300. + 56. + 24.);
        self.show_wdl = side_width >= px(450.);
        let scoresheet = self.render_scoresheet(cx);
        let board_column = self.render_board_column(board_size, cx);
        let table = self.render_moves_table(cx);
        let preview = self.render_line_preview(cx);
        let note = self.note.clone();
        let menu = self.render_context_menu(cx);
        h_flex()
            .items_start()
            .size_full()
            .child(scoresheet)
            .child(div().px(px(28.)).h_full().flex_none().child(board_column))
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
                        v_flex().gap(px(18.)).child(table).child(preview).child(
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
                    if screen.context_menu.take().is_some() {
                        cx.notify();
                    }
                }),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stat(san: &str, games: u64) -> MoveStat {
        MoveStat {
            san: san.to_owned(),
            games,
            white_wins: games,
            draws: 0,
            black_wins: 0,
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
    fn rows_are_the_union_sorted_by_eval_then_popularity() {
        let rows = merge_rows(
            vec![line("h4", 31), line("f4", 42)],
            vec![stat("f4", 412), stat("Kb1", 131), stat("Ng3", 47)],
            vec![stat("h4", 9)],
            &[("f4".to_owned(), true), ("Ng3".to_owned(), false)],
            true,
            true,
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
            false,
            false,
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
            true,
            true,
        );
        assert_eq!(rows.len(), 1);
    }

    #[test]
    fn no_repertoire_means_no_deviation_tag() {
        let rows = merge_rows(vec![], vec![], vec![stat("e4", 3)], &[], true, true);
        assert_eq!(rows[0].rep, RepTag::None);
        // Nor on the opponent's turn, where the moves are not the user's choices.
        let rows = merge_rows(
            vec![],
            vec![],
            vec![stat("e5", 3)],
            &[("c5".to_owned(), true)],
            false,
            false,
        );
        assert_eq!(
            rows.iter().find(|row| row.san == "e5").unwrap().rep,
            RepTag::None
        );
    }
}
