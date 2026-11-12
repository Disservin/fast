//! Database: database list + filter form, virtualized games table, preview panel.

use std::{collections::HashMap, ops::Range, path::PathBuf};

use chess_core::{Chess, GameTree, Position, classify, position_hash};
use db::{
    Database, Filter, FilterStats, GameDetails, GameInfo, GameRow, ImportControl, ImportOptions,
    ImportProgress, ResultFilter, ReviewPosition, ReviewSummary, SideFilter, Sort, SortKey,
    TimeClass, date_bound, import_games,
};
use gpui::{
    AnyElement, AppContext as _, Context, Entity, FontWeight, IntoElement, MouseButton, Render,
    ScrollStrategy, SharedString, Subscription, Task, UniformListScrollHandle, WeakEntity, Window,
    div, prelude::*, px, rgb, uniform_list,
};
use gpui_component::{
    input::{InputEvent, InputState},
    menu::{ContextMenuExt as _, PopupMenuItem},
    scroll::ScrollableElement as _,
    select::SelectEvent,
};
use ui::{
    BoardView, MONO, T,
    widgets::{
        badge, btn, check, h_flex, icon_btn, mono, panel, section_label, segmented, stat, v_flex,
    },
};

use crate::format::{format_count, now_seconds};
use crate::{
    controls::{TextSelect, field, new_input, new_select, select_el, text_input},
    game_download::{DownloadEvent, DownloadPhase, DownloadRequest, Endpoints, Site},
    model::{AppModel, ConfigExt as _, DbEntry, ModelEvent, ToastExt as _, pick_path},
    shell::Workspace,
};

use self::{
    editor::number,
    preview::{MoveNotes, key_moment, leak_context, move_notes},
};
use crate::review::{ReviewFinding, review_accuracy, review_findings};

mod collections;
mod download;
mod editor;
mod filter;
mod import;
mod left;
mod management;
mod preview;
mod query;
mod review;
mod table;

/// The results a game can be given in the game editor, as stored.
const RESULTS: [&str; 4] = ["1-0", "1/2-1/2", "0-1", "*"];

/// The open game editor: a field for each header and the chosen result.
struct GameEditor {
    id: i64,
    white: Entity<InputState>,
    black: Entity<InputState>,
    white_elo: Entity<InputState>,
    black_elo: Entity<InputState>,
    event: Entity<InputState>,
    round: Entity<InputState>,
    date: Entity<InputState>,
    eco: Entity<InputState>,
    result: usize,
    _subscriptions: Vec<Subscription>,
}

impl GameEditor {
    /// The fields in the order they appear, for focus and Enter handling.
    fn inputs(&self) -> [&Entity<InputState>; 8] {
        [
            &self.white,
            &self.black,
            &self.white_elo,
            &self.black_elo,
            &self.event,
            &self.round,
            &self.date,
            &self.eco,
        ]
    }
}

const COLOURS: [&str; 3] = ["Any", "White", "Black"];
/// Result choices with a player set, and without one.
const PLAYER_RESULTS: [&str; 4] = ["Any", "Win", "Draw", "Loss"];
const BOARD_RESULTS: [&str; 4] = ["Any", "1–0", "½–½", "0–1"];
/// How long typing must pause before the filter runs.
const FILTER_DELAY: std::time::Duration = std::time::Duration::from_millis(200);
const TIME_CONTROLS: [&str; 5] = ["Any", "Rapid + Classical", "Classical", "Rapid", "Blitz"];
/// Rows fetched per page as the table scrolls.
const PAGE: usize = 200;
const ROW_H: f32 = 37.5;

// Column widths for the games table.
const W_NUMBER: f32 = 48.;
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

/// Where the Import dialog takes games from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ImportSource {
    File,
    Site(Site),
}

/// How far back a download goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Since {
    All,
    LastYear,
    LastMonth,
    LastUpdate,
}

/// A running download from Lichess or Chess.com.
struct Downloading {
    control: ImportControl,
    site: Site,
    user: String,
    database: PathBuf,
    database_name: SharedString,
    games: u64,
    duplicates: u64,
    /// Chess.com months fetched out of all months asked for.
    months: Option<(usize, usize)>,
    /// Seconds until the site's requested wait is over.
    waiting: Option<u64>,
    phase: DownloadPhase,
}

impl Downloading {
    /// Reports a waiting or cancellation state immediately, including before the first game.
    fn status(&self) -> String {
        if self.control.is_cancelled() {
            return ui::i18n::t("Cancelling download…").to_owned();
        }
        if let Some(seconds) = self.waiting {
            return ui::i18n::tf(
                "{} asks us to wait. Retrying in {} s.",
                &[self.site.name(), &seconds.to_string()],
            );
        }
        self.phase.label(self.site)
    }
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
    /// Searchable collection choice, synchronized with imports and external database changes.
    collection_picker: TextSelect,
    collection_entries: Vec<DbEntry>,
    collection_labels: Vec<SharedString>,
    filters_open: bool,
    player: Entity<InputState>,
    colour: TextSelect,
    result: TextSelect,
    /// Whether the result choices are currently worded for a player.
    result_for_player: bool,
    date_from: Entity<InputState>,
    date_to: Entity<InputState>,
    event: Entity<InputState>,
    min_rating: Entity<InputState>,
    max_rating: Entity<InputState>,
    min_moves: Entity<InputState>,
    max_moves: Entity<InputState>,
    only_annotated: bool,
    /// A reference position for exact pawn and material matches.
    pattern_fen: Entity<InputState>,
    pattern_pawns: bool,
    pattern_material: bool,
    pattern_indexed: bool,
    pattern_control: Option<ImportControl>,
    pattern_progress: (u64, u64),
    pattern_task: Option<Task<()>>,
    /// Whether the "More filters" section is unfolded.
    more_filters: bool,
    /// The name field shown while saving the current search.
    saving_search: Option<Entity<InputState>>,
    _saving_sub: Option<Subscription>,
    filter_task: Option<Task<()>>,
    eco: Entity<InputState>,
    opp_rating: Entity<InputState>,
    time_control: TextSelect,
    reached: bool,
    /// Position captured from the Analyse board, and a label for the summary.
    reached_position: Option<(u64, String)>,
    only_left_rep: bool,
    /// Filter currently applied to the table.
    applied: Filter,
    /// The table's order, remembered per database.
    sort: Sort,
    /// A query is running; the current rows stay until its results arrive.
    querying: bool,
    /// Keyboard focus for each sortable column heading, by `SortKey`.
    summary: SharedString,
    ids: Vec<i64>,
    rows: HashMap<i64, GameRow>,
    requested_pages: std::collections::HashSet<usize>,
    stats: FilterStats,
    leak_text: Option<String>,
    selected: Option<i64>,
    preview: Option<GameDetails>,
    /// Comments and side variations after each mainline move of the previewed game.
    preview_notes: Vec<MoveNotes>,
    preview_key: usize,
    preview_board: Entity<BoardView>,
    scroll: UniformListScrollHandle,
    importing: Option<Importing>,
    /// Whether the Import dialog is open.
    import_dialog: bool,
    /// Collection settings and sources, separate from the game table's filters.
    manager: Option<management::Manager>,
    add_database: Option<management::AddDatabase>,
    collection_control: Option<ImportControl>,
    collection_progress: String,
    collection_error: Option<String>,
    import_source: ImportSource,
    download_user: Entity<InputState>,
    download_since: Since,
    /// Download into the selected database instead of My games.
    download_into_selected: bool,
    download_clocks: bool,
    /// The account is the user's, so its name counts as theirs.
    download_is_me: bool,
    download_error: Option<String>,
    downloading: Option<Downloading>,
    /// The file numbers of games that lost a variation in the last import, shown when
    /// the user asks for details.
    dropped_details: Option<Vec<u64>>,
    /// A database and its source file waiting for the user to confirm a re-import.
    confirm_reimport: Option<(PathBuf, PathBuf)>,
    reviewing: Option<Reviewing>,
    review_engine: Option<engine::EngineId>,
    review_limit: usize,
    review_side: usize,
    review_selected_only: bool,
    /// Whether the batch review settings dialog is open.
    review_settings_open: bool,
    /// The database being renamed and the field holding its new name.
    renaming: Option<crate::controls::Rename<PathBuf>>,
    editing: Option<GameEditor>,
    /// Games waiting for the user to confirm that they should be deleted.
    confirm_delete: Option<Vec<i64>>,
    /// Player names from the selected database matching what is typed in Player, with
    /// their game counts.
    player_suggestions: Vec<(String, u64)>,
    /// The suggestion chosen with the arrow keys.
    player_highlight: Option<usize>,
    /// The name just picked, so filling it in does not suggest it again.
    player_picked: Option<String>,
    suggest_task: Option<Task<()>>,
    query_task: Option<Task<()>>,
    preview_task: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

/// "1/2-1/2" as "½–½", "1-0" as "1–0".
fn result_text(result: &str) -> String {
    match result {
        "1/2-1/2" => "½–½".to_owned(),
        other => other.replace('-', "–"),
    }
}

impl Drop for DatabaseScreen {
    /// Stops optional indexing when its tab closes.
    fn drop(&mut self) {
        if let Some(control) = &self.pattern_control {
            control.cancel();
        }
        if let Some(control) = &self.collection_control {
            control.cancel();
        }
    }
}

impl DatabaseScreen {
    /// Engines are reviewing games.
    /// Closes whichever dialog is open.
    fn close_dialogs(&mut self, cx: &mut Context<Self>) {
        self.manager = None;
        self.add_database = None;
        self.editing = None;
        self.confirm_delete = None;
        self.import_dialog = false;
        self.dropped_details = None;
        self.confirm_reimport = None;
        cx.notify();
    }

    pub fn is_reviewing(&self) -> bool {
        self.reviewing.is_some()
    }

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
        let result = new_select(
            BOARD_RESULTS
                .iter()
                .map(|text| SharedString::from(ui::i18n::t(text)))
                .collect(),
            0,
            window,
            cx,
        );
        let date_from = new_input("", "From", window, cx);
        let date_to = new_input("", "To", window, cx);
        let event = new_input("", "Event or site", window, cx);
        let min_rating = new_input("", "Min", window, cx);
        let max_rating = new_input("", "Max", window, cx);
        let min_moves = new_input("", "Min", window, cx);
        let max_moves = new_input("", "Max", window, cx);
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
        let collection_picker = cx.new(|cx| {
            gpui_component::select::SelectState::new(
                gpui_component::select::SearchableVec::<SharedString>::new(Vec::new()),
                None,
                window,
                cx,
            )
            .searchable(true)
        });
        let mut subscriptions = Vec::new();
        subscriptions.push(cx.subscribe(
            &collection_picker,
            |screen, _, event: &SelectEvent<_>, cx| {
                let SelectEvent::Confirm(Some(label)) = event else {
                    return;
                };
                if let Some(index) = screen
                    .collection_labels
                    .iter()
                    .position(|item| item == label)
                    && let Some(entry) = screen.collection_entries.get(index)
                {
                    screen.selected_db = Some(entry.path.clone());
                    screen.selected = None;
                    screen.apply_filter(cx);
                }
            },
        ));
        subscriptions.push(cx.subscribe_in(
            &player,
            window,
            |this, _, event, window, cx| match event {
                InputEvent::Change => {
                    this.sync_result_labels(window, cx);
                    this.schedule_filter(cx);
                    this.suggest_players(cx);
                }
                InputEvent::PressEnter { .. } => match this.player_highlight {
                    Some(index) => this.pick_player(index, window, cx),
                    None => this.apply_filter(cx),
                },
                InputEvent::Blur => this.close_suggestions(cx),
                _ => {}
            },
        ));
        for input in [
            &eco,
            &opp_rating,
            &date_from,
            &date_to,
            &event,
            &min_rating,
            &max_rating,
            &min_moves,
            &max_moves,
        ] {
            subscriptions.push(cx.subscribe_in(
                input,
                window,
                |this, _, event, _, cx| match event {
                    InputEvent::Change => this.schedule_filter(cx),
                    InputEvent::PressEnter { .. } => this.apply_filter(cx),
                    _ => {}
                },
            ));
        }
        for select in [&colour, &time_control, &result] {
            subscriptions.push(cx.subscribe(select, |this, _, _: &SelectEvent<_>, cx| {
                this.schedule_filter(cx);
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
        let pattern_fen = new_input(
            if model.read(cx).demo {
                "rnbqkbnr/pppp1ppp/8/4p3/4P3/8/PPPP1PPP/RNBQKBNR w KQkq - 0 2"
            } else {
                ""
            },
            "Reference FEN",
            window,
            cx,
        );
        let pattern_sub = cx.subscribe(&pattern_fen, |this, _, _: &InputEvent, cx| {
            this.schedule_filter(cx)
        });
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
        let download_user = new_input("", ui::i18n::t("Username"), window, cx);
        subscriptions.push(
            cx.subscribe_in(
                &download_user,
                window,
                |this, _, event, _, cx| match event {
                    InputEvent::Change => {
                        this.download_error = None;
                        cx.notify();
                    }
                    InputEvent::PressEnter { .. } => this.download_from_dialog(cx),
                    _ => {}
                },
            ),
        );
        let mut this = Self {
            model,
            workspace,
            selected_db,
            db_snapshot: None,
            collection_picker,
            collection_entries: Vec::new(),
            collection_labels: Vec::new(),
            filters_open: true,
            player,
            colour,
            result,
            result_for_player: false,
            date_from,
            date_to,
            event,
            min_rating,
            max_rating,
            min_moves,
            max_moves,
            only_annotated: false,
            pattern_fen,
            pattern_pawns: false,
            pattern_material: false,
            pattern_indexed: false,
            pattern_control: None,
            pattern_progress: (0, 0),
            pattern_task: None,
            more_filters: false,
            saving_search: None,
            _saving_sub: None,
            filter_task: None,
            eco,
            opp_rating,
            time_control,
            reached: demo,
            reached_position,
            only_left_rep: false,
            applied: Filter::default(),
            sort: Sort::default(),
            querying: false,
            summary: "".into(),
            ids: Vec::new(),
            rows: HashMap::new(),
            requested_pages: Default::default(),
            stats: FilterStats::default(),
            leak_text: None,
            selected: None,
            preview: None,
            preview_notes: Vec::new(),
            preview_key: 0,
            preview_board,
            scroll: UniformListScrollHandle::new(),
            importing: None,
            import_dialog: false,
            manager: None,
            add_database: None,
            collection_control: None,
            collection_progress: String::new(),
            collection_error: None,
            import_source: ImportSource::File,
            download_user,
            download_since: Since::All,
            download_into_selected: false,
            download_clocks: true,
            download_is_me: true,
            download_error: None,
            downloading: None,
            dropped_details: None,
            confirm_reimport: None,
            reviewing: None,
            review_engine,
            review_limit: 0,
            review_side: 0,
            review_selected_only: false,
            review_settings_open: false,
            renaming: None,
            editing: None,
            confirm_delete: None,
            player_suggestions: Vec::new(),
            player_highlight: None,
            player_picked: None,
            suggest_task: None,
            query_task: None,
            preview_task: None,
            _subscriptions: {
                let mut subscriptions = subscriptions;
                subscriptions.push(pattern_sub);
                subscriptions
            },
        };
        this.sync_result_labels(window, cx);
        this.apply_filter(cx);
        this
    }

    pub fn breadcrumb(&self) -> SharedString {
        self.db_snapshot
            .as_ref()
            .map_or_else(|| "No database".into(), |entry| entry.name.clone())
    }
}

impl Render for DatabaseScreen {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // The table needs its fixed columns, so the preview makes way on narrow windows.
        let show_preview = window.viewport_size().width >= px(1320.);
        self.sync_collection_picker(window, cx);
        let collections = self.render_collections(cx);
        let left = self.render_left(cx);
        let header = self.render_main_header(cx);
        let stats = self.render_stats();
        let table = self.render_table(cx);
        let preview = self.render_preview(window.scale_factor(), cx);
        let import_bar = self.render_import_bar(cx);
        let download_bar = self.render_download_bar(cx);
        let review_dialog = self
            .review_settings_open
            .then(|| self.render_review_dialog(cx));
        let editor = self.render_editor(cx);
        let confirm_delete = self.render_confirm_delete(cx);
        let import_dialog = self.render_import_dialog(cx);
        let dropped_details = self.render_dropped_details(cx);
        let confirm_reimport = self.render_confirm_reimport(cx);
        let manager = self.render_manager(cx);
        let add_database = self.render_add_database(cx);
        let collection_bar = self.render_collection_bar(cx);
        v_flex()
            .relative()
            .size_full()
            .capture_key_down(cx.listener(|screen, event: &gpui::KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" && !screen.player_suggestions.is_empty() {
                    screen.close_suggestions(cx);
                    cx.stop_propagation();
                }
            }))
            .child(collections)
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .items_start()
                    .when(self.filters_open, |row| row.child(left))
                    .child(
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
                            .children(download_bar)
                            .children(collection_bar)
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
                    ),
            )
            .children(review_dialog)
            .children(editor)
            .children(confirm_delete)
            .children(import_dialog)
            .children(dropped_details)
            .children(confirm_reimport)
            .children(manager)
            .children(add_database)
    }
}

/// A dialog over the Database tab with a titled header and `body`. The × and a click
/// on the dimmed backdrop close the game editor and the delete question.
/// A Database dialog that closes whichever dialog is open.
fn dialog(
    id: &'static str,
    title: &'static str,
    width: f32,
    body: AnyElement,
    screen: Entity<DatabaseScreen>,
) -> AnyElement {
    ui::widgets::dialog(id, title, width, body, move |cx| {
        screen.update(cx, |screen, cx| screen.close_dialogs(cx))
    })
    .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn results_use_typographic_dashes_and_halves() {
        assert_eq!(result_text("1/2-1/2"), "½–½");
        assert_eq!(result_text("1-0"), "1–0");
    }
}
