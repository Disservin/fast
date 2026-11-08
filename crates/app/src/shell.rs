//! The window shell: top bar, mode routing, global shortcuts, toasts.

use crate::model::ToastExt;
use std::collections::BTreeMap;

use engine::EngineStatus;
use gpui::{
    Action, App, Context, CursorStyle, Decorations, Entity, FocusHandle, FontWeight, IntoElement,
    KeyBinding, MouseButton, Pixels, Render, ResizeEdge, ScrollEvent, ScrollHandle, SharedString,
    Subscription, Window, WindowControlArea, actions, deferred, div, prelude::*, px, rgb,
};
use gpui_component::input::{InputEvent, InputState, TextareaState};
use gpui_component::menu::{ContextMenuExt, PopupMenuItem};
use ui::{
    SANS, T,
    widgets::{Btn, BtnKind, btn, h_flex, icon_btn, v_flex},
};

use crate::{
    model::AppModel,
    screens::{
        analyse::AnalyseScreen,
        database::DatabaseScreen,
        play::PlayScreen,
        settings::{SettingsPage, SettingsScreen},
        test_positions::TestPositionsScreen,
        tournament::TournamentScreen,
        train::TrainScreen,
    },
};

actions!(
    fast,
    [
        PrevMove,
        NextMove,
        FirstMove,
        LastMove,
        ToggleEngine,
        FlipBoard,
        FocusSearch,
        SaveGame,
        NewGame,
        NewTab,
        CloseTab,
        ReopenTab,
        NextTab,
        PrevTab,
        TabBack,
        TabForward,
        OpenGame,
        SavePgnAs,
        PasteGame,
        CopyFen,
        CopyPgn,
        PauseEngine,
        EngineOutput,
        AnalyseBackward,
        MoreLines,
        FewerLines,
        ToggleAllMoves,
        ToggleArrows,
        ToggleSortLines,
        NextGraph,
        ToggleScoresheet,
        ShowAnalyse,
        ShowDatabase,
        ShowTrain,
        ShowPlay,
        ShowSettings,
        NextExplorerDb,
        Quit
    ]
);

/// The application menus: native on macOS, drawn in the top bar elsewhere.
fn app_menus() -> Vec<gpui::Menu> {
    use gpui::{Menu, MenuItem};
    vec![
        Menu::new(ui::i18n::t("File")).items([
            MenuItem::action(ui::i18n::t("New game"), NewGame),
            MenuItem::action(ui::i18n::t("New tab"), NewTab),
            MenuItem::action(ui::i18n::t("Close tab"), CloseTab),
            MenuItem::action(ui::i18n::t("Reopen closed tab"), ReopenTab),
            MenuItem::action(ui::i18n::t("Open PGN or FEN…"), OpenGame),
            MenuItem::separator(),
            MenuItem::action(ui::i18n::t("Save to My games"), SaveGame),
            MenuItem::action(ui::i18n::t("Save as PGN…"), SavePgnAs),
            MenuItem::separator(),
            MenuItem::action(ui::i18n::t("Settings"), ShowSettings),
            MenuItem::separator(),
            MenuItem::action(ui::i18n::t("Quit"), Quit),
        ]),
        Menu::new(ui::i18n::t("Edit")).items([
            MenuItem::action(ui::i18n::t("Copy FEN"), CopyFen),
            MenuItem::action(ui::i18n::t("Copy PGN"), CopyPgn),
            MenuItem::action("Paste FEN or PGN", PasteGame),
        ]),
        Menu::new(ui::i18n::t("View")).items([
            MenuItem::action(ui::i18n::t("Analyse"), ShowAnalyse),
            MenuItem::action(ui::i18n::t("Database"), ShowDatabase),
            MenuItem::action(ui::i18n::t("Train"), ShowTrain),
            MenuItem::action(ui::i18n::t("Play"), ShowPlay),
            MenuItem::action(ui::i18n::t("Next tab"), NextTab),
            MenuItem::action(ui::i18n::t("Previous tab"), PrevTab),
            MenuItem::action(ui::i18n::t("Go back"), TabBack),
            MenuItem::action(ui::i18n::t("Go forward"), TabForward),
            MenuItem::separator(),
            MenuItem::action(ui::i18n::t("Flip board"), FlipBoard),
        ]),
        Menu::new("Go").items([
            MenuItem::action("Start of game", FirstMove),
            MenuItem::action(ui::i18n::t("Previous move"), PrevMove),
            MenuItem::action(ui::i18n::t("Next move"), NextMove),
            MenuItem::action("End of line", LastMove),
        ]),
        Menu::new(ui::i18n::t("Engine")).items([
            MenuItem::action("Engine on / off", ToggleEngine),
            MenuItem::action("Pause / resume", PauseEngine),
            MenuItem::action("Analyse game backwards", AnalyseBackward),
            MenuItem::separator(),
            MenuItem::action("Engine output window", EngineOutput),
        ]),
    ]
}

/// Windows hides its title bar and the top bar takes its place, window controls
/// included. Elsewhere the system draws one when it can.
pub const DRAWS_TITLE_BAR: bool = cfg!(target_os = "windows");

/// Whether the menus are drawn by the app rather than the system.
fn draws_own_menus() -> bool {
    !cfg!(target_os = "macos")
}

const WINDOW_CONTROL_W: f32 = 36.;
const WINDOW_CONTROLS_W: f32 = WINDOW_CONTROL_W * 3. + 4.;
/// Half the width of the centered mode switch.
const MODE_SWITCH_HALF_W: f32 = 165.;

/// Key context present unless a text field has focus.
pub const SHORTCUT_CONTEXT: &str = "FastShortcuts";
/// Additionally present unless a button has focus, where Space means "activate".
const SPACE_CONTEXT: &str = "FastIdle";
const TAB_CONTEXT: &str = "FastTabs";

pub fn action_for(name: &str) -> Option<(Box<dyn Action>, &'static str)> {
    Some(match name {
        "prev_move" => (Box::new(PrevMove), SHORTCUT_CONTEXT),
        "next_move" => (Box::new(NextMove), SHORTCUT_CONTEXT),
        "first_move" => (Box::new(FirstMove), SHORTCUT_CONTEXT),
        "last_move" => (Box::new(LastMove), SHORTCUT_CONTEXT),
        "toggle_engine" => (Box::new(ToggleEngine), SPACE_CONTEXT),
        "flip_board" => (Box::new(FlipBoard), SHORTCUT_CONTEXT),
        "search" => (Box::new(FocusSearch), SHORTCUT_CONTEXT),
        "save_game" => (Box::new(SaveGame), SHORTCUT_CONTEXT),
        "new_game" => (Box::new(NewGame), SHORTCUT_CONTEXT),
        "new_tab" => (Box::new(NewTab), TAB_CONTEXT),
        "close_tab" => (Box::new(CloseTab), TAB_CONTEXT),
        "reopen_tab" => (Box::new(ReopenTab), TAB_CONTEXT),
        "next_tab" => (Box::new(NextTab), TAB_CONTEXT),
        "prev_tab" => (Box::new(PrevTab), TAB_CONTEXT),
        "next_tab_alt" => (Box::new(NextTab), TAB_CONTEXT),
        "prev_tab_alt" => (Box::new(PrevTab), TAB_CONTEXT),
        "tab_back" => (Box::new(TabBack), TAB_CONTEXT),
        "tab_forward" => (Box::new(TabForward), TAB_CONTEXT),
        "open_game" => (Box::new(OpenGame), SHORTCUT_CONTEXT),
        "save_pgn_as" => (Box::new(SavePgnAs), SHORTCUT_CONTEXT),
        "paste_game" => (Box::new(PasteGame), SHORTCUT_CONTEXT),
        "copy_fen" => (Box::new(CopyFen), SHORTCUT_CONTEXT),
        "copy_pgn" => (Box::new(CopyPgn), SHORTCUT_CONTEXT),
        "pause_engine" => (Box::new(PauseEngine), SHORTCUT_CONTEXT),
        "analyse_backward" => (Box::new(AnalyseBackward), SHORTCUT_CONTEXT),
        "more_lines" => (Box::new(MoreLines), SHORTCUT_CONTEXT),
        "fewer_lines" => (Box::new(FewerLines), SHORTCUT_CONTEXT),
        "engine_output" => (Box::new(EngineOutput), SHORTCUT_CONTEXT),
        "all_moves" => (Box::new(ToggleAllMoves), SHORTCUT_CONTEXT),
        "quality_arrows" => (Box::new(ToggleArrows), SHORTCUT_CONTEXT),
        "sort_lines" => (Box::new(ToggleSortLines), SHORTCUT_CONTEXT),
        "next_graph" => (Box::new(NextGraph), SHORTCUT_CONTEXT),
        "scoresheet_style" => (Box::new(ToggleScoresheet), SHORTCUT_CONTEXT),
        "show_analyse" => (Box::new(ShowAnalyse), SHORTCUT_CONTEXT),
        "show_database" => (Box::new(ShowDatabase), SHORTCUT_CONTEXT),
        "show_train" => (Box::new(ShowTrain), SHORTCUT_CONTEXT),
        "show_play" => (Box::new(ShowPlay), SHORTCUT_CONTEXT),
        "show_settings" => (Box::new(ShowSettings), TAB_CONTEXT),
        "next_explorer_db" => (Box::new(NextExplorerDb), SHORTCUT_CONTEXT),
        "quit" => (Box::new(Quit), SHORTCUT_CONTEXT),
        _ => return None,
    })
}

/// Binds the configured keystrokes, skipping any that do not parse.
pub fn bind_shortcuts(shortcuts: &BTreeMap<String, String>, cx: &mut App) {
    for (name, keystroke) in shortcuts {
        // Cleared in Settings.
        if keystroke.is_empty() {
            continue;
        }
        let Some((action, context)) = action_for(name) else {
            continue;
        };
        load_binding(keystroke, action, context, cx);
    }
}

fn load_binding(keystroke: &str, action: Box<dyn Action>, context: &str, cx: &mut App) {
    let predicate = gpui::KeyBindingContextPredicate::parse(context)
        .map(Into::into)
        .ok();
    match KeyBinding::load(keystroke, action, predicate, false, None, cx) {
        Ok(binding) => cx.bind_keys([binding]),
        Err(err) => log::warn!("ignoring shortcut {keystroke}: {err}"),
    }
}

/// Removes a shortcut so it can be rebound.
pub fn unbind_shortcut(keystroke: &str, name: &str, cx: &mut App) {
    if keystroke.is_empty() {
        return;
    }
    if let Some((_, context)) = action_for(name) {
        load_binding(keystroke, Box::new(gpui::NoAction), context, cx);
    }
}

pub fn init(cx: &mut App) {
    cx.on_action(|_: &Quit, cx| cx.quit());
    refresh_menus(cx);
}

pub fn refresh_menus(cx: &mut App) {
    cx.set_menus(app_menus());
    gpui_component::GlobalState::global_mut(cx)
        .set_app_menus(app_menus().into_iter().map(gpui::Menu::owned).collect());
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Mode {
    Analyse,
    Database,
    Train,
    Play,
    Settings,
    /// Optional: only opened from the new-tab menu.
    Tournament,
    /// Optional: only opened from the new-tab menu.
    TestPositions,
}

#[derive(Clone)]
enum TabContent {
    Analyse(Entity<AnalyseScreen>),
    Database(Entity<DatabaseScreen>),
    Train(Entity<TrainScreen>),
    Play(Entity<PlayScreen>),
    Settings(Entity<SettingsScreen>),
    Tournament(Entity<TournamentScreen>),
    TestPositions(Entity<TestPositionsScreen>),
}
impl TabContent {
    fn mode(&self) -> Mode {
        match self {
            Self::Analyse(_) => Mode::Analyse,
            Self::Database(_) => Mode::Database,
            Self::Train(_) => Mode::Train,
            Self::Play(_) => Mode::Play,
            Self::Settings(_) => Mode::Settings,
            Self::Tournament(_) => Mode::Tournament,
            Self::TestPositions(_) => Mode::TestPositions,
        }
    }
    /// The tab's screen and, where it has one, what that screen is showing.
    fn title(&self, cx: &App) -> (&'static str, Option<SharedString>) {
        match self {
            Self::Analyse(screen) => (ui::i18n::t("Analyse"), Some(screen.read(cx).breadcrumb())),
            Self::Database(screen) => (ui::i18n::t("Database"), Some(screen.read(cx).breadcrumb())),
            Self::Train(_) => (ui::i18n::t("Train"), None),
            Self::Settings(_) => (ui::i18n::t("Settings"), None),
            Self::Play(screen) => (ui::i18n::t("Play"), Some(screen.read(cx).breadcrumb())),
            Self::Tournament(screen) => (ui::i18n::t("Tournament"), screen.read(cx).tab_detail()),
            Self::TestPositions(screen) => {
                (ui::i18n::t("Test positions"), screen.read(cx).tab_detail())
            }
        }
    }
}
impl Mode {
    /// The screen as it is written in the saved session.
    fn saved(self) -> crate::config::SavedTabKind {
        use crate::config::SavedTabKind as Kind;
        match self {
            Self::Analyse => Kind::Analyse,
            Self::Database => Kind::Database,
            Self::Train => Kind::Train,
            Self::Play => Kind::Play,
            Self::Settings => Kind::Settings,
            Self::Tournament => Kind::Tournament,
            Self::TestPositions => Kind::TestPositions,
        }
    }

    fn from_saved(kind: crate::config::SavedTabKind) -> Self {
        use crate::config::SavedTabKind as Kind;
        match kind {
            Kind::Analyse => Self::Analyse,
            Kind::Database => Self::Database,
            Kind::Train => Self::Train,
            Kind::Play => Self::Play,
            Kind::Settings => Self::Settings,
            Kind::Tournament => Self::Tournament,
            Kind::TestPositions => Self::TestPositions,
        }
    }
}

#[derive(Clone)]
struct WorkspaceTab {
    id: u64,
    content: TabContent,
    /// The name the user gave the tab, shown instead of its screen and detail.
    name: Option<SharedString>,
}

#[derive(Clone)]
struct DraggedTab {
    id: u64,
    title: SharedString,
}

impl Render for DraggedTab {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .px(px(12.))
            .py(px(6.))
            .rounded(px(6.))
            .bg(rgb(T.selected))
            .child(self.title.clone())
    }
}

pub struct Workspace {
    pub model: Entity<AppModel>,
    pub mode: Mode,
    pub analyse: Entity<AnalyseScreen>,
    pub database: Entity<DatabaseScreen>,
    pub train: Entity<TrainScreen>,
    pub play: Entity<PlayScreen>,
    pub settings: Entity<SettingsScreen>,
    /// The tournament tab shown last, if one was ever opened.
    tournament: Option<Entity<TournamentScreen>>,
    /// The test positions tab shown last, if one was ever opened.
    test_positions: Option<Entity<TestPositionsScreen>>,
    tabs: Vec<WorkspaceTab>,
    closed_tabs: Vec<WorkspaceTab>,
    active_tab: u64,
    next_tab_id: u64,
    last_tab: BTreeMap<Mode, u64>,
    /// Tabs visited before the active one, most recent last, for Back.
    back_tabs: Vec<u64>,
    /// Tabs left with Back, most recent last, for Forward.
    forward_tabs: Vec<u64>,
    new_tab_menu_open: bool,
    /// The tab being renamed and the field holding its new name.
    renaming: Option<(u64, Entity<InputState>)>,
    _rename_sub: Option<Subscription>,
    /// The tab strip's horizontal scroll.
    tab_scroll: ScrollHandle,
    /// The tab last scrolled into view, so the strip follows a change of tab but not every
    /// render.
    tab_in_view: std::cell::Cell<u64>,
    search: Entity<InputState>,
    /// Live matches from the local game collections.
    search_results: Vec<(std::path::PathBuf, db::GameRow)>,
    search_open: bool,
    search_selected: Option<usize>,
    search_pending: bool,
    search_failed: bool,
    search_task: Option<gpui::Task<()>>,
    path_input: Entity<InputState>,
    /// The starting position for a new Chess960 analysis tab.
    chess960_number: Entity<InputState>,
    paths_input: Entity<TextareaState>,
    menu_bar: Option<Entity<gpui_component::menu::AppMenuBar>>,
    focus: FocusHandle,
    first_frame: bool,
    /// Test switch: draw window controls regardless of what the system reports.
    pub force_window_controls: bool,
    _subscriptions: Vec<Subscription>,
}

impl Workspace {
    pub fn new(
        model: Entity<AppModel>,
        screen: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let shortcuts = model.read(cx).config.shortcuts.clone();
        bind_shortcuts(&shortcuts, cx);
        model.update(cx, |model, cx| model.refresh_leaks(cx));
        let this = cx.entity().downgrade();
        let analyse = cx.new(|cx| AnalyseScreen::new(model.clone(), this.clone(), window, cx));
        let database = cx.new(|cx| DatabaseScreen::new(model.clone(), this.clone(), window, cx));
        let train = cx.new(|cx| TrainScreen::new(model.clone(), this.clone(), window, cx));
        let play = cx.new(|cx| PlayScreen::new(model.clone(), this.clone(), window, cx));
        let settings = cx.new(|cx| SettingsScreen::new(model.clone(), this.clone(), window, cx));
        let search = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(ui::i18n::t("Search positions, games, openings"))
        });
        let chess960_number = crate::controls::new_input(
            model.read(cx).config.chess960_number.min(959).to_string(),
            "0–959",
            window,
            cx,
        );
        let path_input = cx.new(|cx| InputState::new(window, cx).placeholder("/path/to/file"));
        let paths_input = cx.new(|cx| TextareaState::new(window, cx));
        let chess960_sub = cx.subscribe(&chess960_number, |_, _, _: &InputEvent, cx| cx.notify());
        let path_sub = cx.subscribe_in(&path_input, window, |this, _, event, window, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                this.submit_path(window, cx);
            }
        });
        let search_sub = cx.subscribe_in(&search, window, |this, state, event, window, cx| {
            if matches!(event, InputEvent::Change) {
                this.update_search(cx);
            }
            if let InputEvent::PressEnter { .. } = event {
                if let Some(index) = this.search_selected.filter(|_| this.search_open) {
                    this.open_search_result(index, cx);
                } else {
                    let query = state.read(cx).value().to_string();
                    this.run_search(query.trim(), window, cx);
                }
            }
        });
        let mut path_prompt_open = false;
        let mut in_background = model.read(cx).config.analyse_in_background;
        let model_sub = cx.observe_in(&model, window, move |this, model, window, cx| {
            // The typed-path bar takes focus as soon as it opens.
            let open = model.read(cx).pending_path.is_some();
            if open && !path_prompt_open {
                if model
                    .read(cx)
                    .pending_path
                    .as_ref()
                    .is_some_and(|pending| pending.multiple)
                {
                    this.paths_input
                        .update(cx, |input, cx| input.focus(window, cx));
                } else {
                    this.path_input
                        .update(cx, |input, cx| input.focus(window, cx));
                }
            }
            path_prompt_open = open;
            // Turning background analysis off stops a hidden Analyse tab's engines now.
            let background = model.read(cx).config.analyse_in_background;
            if in_background && !background && this.mode != Mode::Analyse {
                this.analyse
                    .update(cx, |screen, cx| screen.set_visible(false, cx));
            }
            in_background = background;
            cx.notify();
        });
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        let (mode, settings_page) = match screen.unwrap_or("analyse") {
            "database" => (Mode::Database, None),
            "train" => (Mode::Train, None),
            "play" => (Mode::Play, None),
            "settings" | "settings:general" => (Mode::Settings, Some(SettingsPage::General)),
            "settings:engines" => (Mode::Settings, Some(SettingsPage::Engines)),
            "settings:analysis" => (Mode::Settings, Some(SettingsPage::Analysis)),
            "settings:board" => (Mode::Settings, Some(SettingsPage::Board)),
            "settings:databases" => (Mode::Settings, Some(SettingsPage::Databases)),
            "settings:shortcuts" => (Mode::Settings, Some(SettingsPage::Shortcuts)),
            "settings:data" => (Mode::Settings, Some(SettingsPage::Data)),
            "settings:licenses" => (Mode::Settings, Some(SettingsPage::Licenses)),
            "settings:thanks" => (Mode::Settings, Some(SettingsPage::Thanks)),
            "settings:changelog" => (Mode::Settings, Some(SettingsPage::Changelog)),
            _ => (Mode::Analyse, None),
        };
        train.update(cx, |train, cx| train.set_visible(mode == Mode::Train, cx));
        analyse.update(cx, |analyse, cx| {
            analyse.set_visible(mode == Mode::Analyse, cx)
        });
        if let Some(page) = settings_page {
            settings.update(cx, |settings, cx| settings.set_page(page, window, cx));
        }
        let initial_content = match mode {
            Mode::Analyse => TabContent::Analyse(analyse.clone()),
            Mode::Database => TabContent::Database(database.clone()),
            Mode::Train => TabContent::Train(train.clone()),
            Mode::Play => TabContent::Play(play.clone()),
            // `--screen` never names the optional tournament tab.
            Mode::Settings | Mode::Tournament | Mode::TestPositions => {
                TabContent::Settings(settings.clone())
            }
        };
        let mut workspace = Self {
            tabs: vec![WorkspaceTab {
                id: 1,
                content: initial_content,
                name: None,
            }],
            closed_tabs: Vec::new(),
            active_tab: 1,
            next_tab_id: 2,
            last_tab: [(mode, 1)].into(),
            back_tabs: Vec::new(),
            forward_tabs: Vec::new(),
            new_tab_menu_open: false,
            renaming: None,
            _rename_sub: None,
            tab_scroll: ScrollHandle::new(),
            tab_in_view: std::cell::Cell::new(u64::MAX),
            model,
            mode,
            analyse,
            database,
            train,
            play,
            settings,
            tournament: None,
            test_positions: None,
            search,
            search_results: Vec::new(),
            search_open: false,
            search_selected: None,
            search_pending: false,
            search_failed: false,
            search_task: None,
            path_input,
            chess960_number,
            paths_input,
            menu_bar: draws_own_menus().then(|| gpui_component::menu::AppMenuBar::new(cx)),
            focus,
            first_frame: true,
            force_window_controls: false,
            _subscriptions: vec![search_sub, chess960_sub, path_sub, model_sub],
        };
        // `--screen` asks for one screen, so the saved tabs only come back without it.
        let session = (screen.is_none())
            .then(|| workspace.model.read(cx).config.session.clone())
            .flatten()
            .filter(|session| !session.tabs.is_empty());
        match session {
            Some(session) => workspace.restore_session(session, window, cx),
            None => workspace.restore_recovery_tabs(window, cx),
        }
        workspace
            ._subscriptions
            .push(cx.observe_window_bounds(window, |workspace, window, cx| {
                let bounds = window.window_bounds();
                let size = bounds.get_bounds().size;
                let saved = crate::config::SavedWindow {
                    width: f32::from(size.width).round().max(1.) as u32,
                    height: f32::from(size.height).round().max(1.) as u32,
                    maximized: matches!(bounds, gpui::WindowBounds::Maximized(_)),
                };
                // Fullscreen dimensions are transient; keep the last normal geometry.
                if matches!(bounds, gpui::WindowBounds::Fullscreen(_)) {
                    return;
                }
                workspace.model.update(cx, |model, cx| {
                    if model.config.window.as_ref() != Some(&saved) {
                        model.config.window = Some(saved);
                        model.schedule_config_save(cx);
                    }
                });
            }));
        if screen == Some("tournament") {
            workspace.new_tab(Mode::Tournament, window, cx);
        }
        if screen == Some("test-positions") {
            workspace.new_tab(Mode::TestPositions, window, cx);
        }
        if workspace.model.read(cx).demo && !workspace.tabs.iter().any(|tab| matches!(&tab.content, TabContent::Analyse(screen) if screen.read(cx).tree.variant == chess_core::Variant::Chess960)) {
            let active = workspace.active_tab;
            workspace.new_tab(Mode::Analyse, window, cx);
            workspace.analyse.update(cx, |screen, cx| {
                screen.load_tree(chess_core::GameTree::chess960(734), window, cx)
            });
            workspace.activate_tab(active, cx);
        }
        workspace
    }

    fn existing_content(&self, mode: Mode) -> Option<TabContent> {
        Some(match mode {
            Mode::Analyse => TabContent::Analyse(self.analyse.clone()),
            Mode::Database => TabContent::Database(self.database.clone()),
            Mode::Train => TabContent::Train(self.train.clone()),
            Mode::Play => TabContent::Play(self.play.clone()),
            Mode::Settings => TabContent::Settings(self.settings.clone()),
            Mode::Tournament => TabContent::Tournament(self.tournament.clone()?),
            Mode::TestPositions => TabContent::TestPositions(self.test_positions.clone()?),
        })
    }

    pub fn set_mode(&mut self, mode: Mode, cx: &mut Context<Self>) {
        let existing = self
            .last_tab
            .get(&mode)
            .copied()
            .filter(|id| self.tabs.iter().any(|tab| tab.id == *id))
            .or_else(|| {
                self.tabs
                    .iter()
                    .rev()
                    .find(|tab| tab.content.mode() == mode)
                    .map(|tab| tab.id)
            });
        if let Some(id) = existing {
            self.activate_tab(id, cx);
        } else if let Some(index) = self
            .closed_tabs
            .iter()
            .rposition(|tab| tab.content.mode() == mode)
        {
            let tab = self.closed_tabs.remove(index);
            let id = tab.id;
            self.tabs.push(tab);
            self.activate_tab(id, cx);
        } else if let Some(content) = self.existing_content(mode) {
            let id = self.next_tab_id;
            self.next_tab_id += 1;
            self.tabs.push(WorkspaceTab {
                id,
                content,
                name: None,
            });
            self.activate_tab(id, cx);
        }
    }

    /// Shows a tab and remembers the one left, so Back returns to it.
    fn activate_tab(&mut self, id: u64, cx: &mut Context<Self>) {
        self.show_tab(id, true, cx);
    }

    /// Goes to the tab visited before the current one (`back`) or the one left with Back.
    /// Tabs closed since are skipped.
    fn step_history(&mut self, back: bool, cx: &mut Context<Self>) {
        let (from, to) = if back {
            (&mut self.back_tabs, &mut self.forward_tabs)
        } else {
            (&mut self.forward_tabs, &mut self.back_tabs)
        };
        while let Some(id) = from.pop() {
            if id == self.active_tab || !self.tabs.iter().any(|tab| tab.id == id) {
                continue;
            }
            to.push(self.active_tab);
            self.show_tab(id, false, cx);
            return;
        }
    }

    /// Shows a tab. With `record`, the tab left goes into the Back history and the
    /// Forward history is dropped, as in a browser.
    fn show_tab(&mut self, id: u64, record: bool, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.iter().find(|tab| tab.id == id).cloned() else {
            return;
        };
        // The new-tab picker covers the content, so going to any tab, even the one
        // already active, dismisses it.
        self.new_tab_menu_open = false;
        if self.active_tab == id {
            cx.notify();
            return;
        }
        if self.mode == Mode::Analyse
            && let Err(err) = self.analyse.read(cx).checkpoint()
        {
            self.model.update(cx, |model, cx| {
                model.toast(
                    format!(
                        "Recovery snapshot failed: {err}. This game is still available in its tab."
                    ),
                    cx,
                )
            });
        }
        if self.active_tab != id {
            // Analyse tabs share the engines, so another Analyse tab always takes them over.
            let keep_analysing = self.model.read(cx).config.analyse_in_background
                && !matches!(tab.content, TabContent::Analyse(_));
            if !keep_analysing {
                self.analyse
                    .update(cx, |screen, cx| screen.set_visible(false, cx));
            }
            self.train
                .update(cx, |screen, cx| screen.set_visible(false, cx));
        }
        if record && self.tabs.iter().any(|tab| tab.id == self.active_tab) {
            self.back_tabs.retain(|visited| *visited != self.active_tab);
            self.back_tabs.push(self.active_tab);
            if self.back_tabs.len() > 50 {
                self.back_tabs.remove(0);
            }
            self.forward_tabs.clear();
        }
        self.active_tab = id;
        self.mode = tab.content.mode();
        self.last_tab.insert(self.mode, id);
        match tab.content {
            TabContent::Analyse(screen) => {
                self.analyse = screen;
                self.analyse
                    .update(cx, |screen, cx| screen.set_visible(true, cx));
            }
            TabContent::Database(screen) => self.database = screen,
            TabContent::Train(screen) => {
                self.train = screen;
                self.train
                    .update(cx, |screen, cx| screen.set_visible(true, cx));
            }
            TabContent::Play(screen) => {
                self.play = screen;
                self.play.update(cx, |screen, cx| screen.resume(cx));
            }
            TabContent::Settings(screen) => self.settings = screen,
            TabContent::Tournament(screen) => self.tournament = Some(screen),
            TabContent::TestPositions(screen) => self.test_positions = Some(screen),
        }
        self.save_session(cx);
        cx.notify();
    }

    fn make_tab(
        &self,
        mode: Mode,
        id: u64,
        path: Option<std::path::PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> WorkspaceTab {
        let model = self.model.clone();
        let workspace = cx.entity().downgrade();
        let directory = model.read(cx).paths.data_dir.join("recovery");
        let content = match mode {
            Mode::Analyse => TabContent::Analyse(cx.new(|cx| {
                AnalyseScreen::new_with_recovery(
                    model,
                    workspace,
                    window,
                    cx,
                    path.unwrap_or_else(|| directory.join(format!("analysis-{id}.json"))),
                )
            })),
            Mode::Play => TabContent::Play(cx.new(|cx| {
                PlayScreen::new_with_recovery(
                    model,
                    workspace,
                    window,
                    cx,
                    path.unwrap_or_else(|| directory.join(format!("play-{id}.json"))),
                )
            })),
            Mode::Database => {
                TabContent::Database(cx.new(|cx| DatabaseScreen::new(model, workspace, window, cx)))
            }
            Mode::Tournament => TabContent::Tournament(
                cx.new(|cx| TournamentScreen::new(model, workspace, window, cx)),
            ),
            Mode::TestPositions => TabContent::TestPositions(
                cx.new(|cx| TestPositionsScreen::new(model, workspace, window, cx)),
            ),
            _ => self
                .existing_content(mode)
                .expect("only optional tabs are created on demand"),
        };
        WorkspaceTab {
            id,
            content,
            name: None,
        }
    }

    fn new_tab(&mut self, mode: Mode, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(mode, Mode::Train | Mode::Settings) {
            self.set_mode(mode, cx);
            return;
        }
        let id = self.next_tab_id;
        self.next_tab_id += 1;
        let tab = self.make_tab(mode, id, None, window, cx);
        self.tabs.push(tab);
        self.activate_tab(id, cx);
    }

    pub fn open_analysis(
        &mut self,
        tree: chess_core::GameTree,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.new_tab(Mode::Analyse, window, cx);
        self.analyse
            .update(cx, |screen, cx| screen.load_tree(tree, window, cx));
    }

    pub fn open_pgn(&mut self, pgn: &str, ply: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.new_tab(Mode::Analyse, window, cx);
        self.analyse
            .update(cx, |screen, cx| screen.load_pgn_at(pgn, ply, window, cx));
    }

    /// Opens a stored game in a new Analyse tab, linked to its database so saving
    /// updates it.
    pub fn open_db_game(
        &mut self,
        details: &db::GameDetails,
        ply: usize,
        origin: crate::screens::analyse::DbGame,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.new_tab(Mode::Analyse, window, cx);
        self.analyse.update(cx, |screen, cx| {
            screen.load_db_game(details, ply, origin, window, cx)
        });
    }

    fn close_tab(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(index) = self.tabs.iter().position(|tab| tab.id == id) else {
            return;
        };
        if let TabContent::Analyse(screen) = &self.tabs[index].content {
            if let Err(err) = screen.read(cx).checkpoint() {
                self.model.update(cx, |model, cx| {
                    model.toast(
                        format!("Could not preserve this tab: {err}. Save it before closing."),
                        cx,
                    )
                });
                return;
            }
            screen.update(cx, |screen, cx| screen.set_visible(false, cx));
        }
        if let TabContent::Play(screen) = &self.tabs[index].content {
            screen.update(cx, |screen, cx| screen.suspend(cx));
        }
        let tab = self.tabs.remove(index);
        self.last_tab.retain(|_, value| *value != id);
        self.closed_tabs.push(tab);
        if self.closed_tabs.len() > 10 {
            self.closed_tabs.remove(0);
        }
        if self.active_tab == id {
            if self.tabs.is_empty() {
                self.new_tab(Mode::Analyse, window, cx);
            } else {
                self.activate_tab(self.tabs[index.min(self.tabs.len() - 1)].id, cx);
            }
        }
        self.save_session(cx);
        cx.notify();
    }

    fn reopen_tab(&mut self, cx: &mut Context<Self>) {
        if let Some(tab) = self.closed_tabs.pop() {
            let id = tab.id;
            self.tabs.push(tab);
            self.activate_tab(id, cx);
        }
    }

    fn cycle_tab(&mut self, delta: isize, cx: &mut Context<Self>) {
        if self.tabs.is_empty() {
            return;
        }
        let current = self
            .tabs
            .iter()
            .position(|tab| tab.id == self.active_tab)
            .unwrap_or(0);
        let index = (current as isize + delta).rem_euclid(self.tabs.len() as isize) as usize;
        self.activate_tab(self.tabs[index].id, cx);
    }

    fn restore_recovery_tabs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let directory = self.model.read(cx).paths.data_dir.join("recovery");
        let Ok(entries) = std::fs::read_dir(directory) else {
            return;
        };
        let mut paths: Vec<_> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
            .collect();
        paths.sort();
        for path in paths {
            let Some(name) = path.file_stem().and_then(|name| name.to_str()) else {
                continue;
            };
            let mode = if name.starts_with("analysis-") {
                Mode::Analyse
            } else if name.starts_with("play-") {
                Mode::Play
            } else {
                continue;
            };
            let Some(id) = name
                .rsplit('-')
                .next()
                .and_then(|value| value.parse::<u64>().ok())
                .filter(|id| *id > 1 && *id < u64::MAX)
            else {
                continue;
            };
            if self.tabs.iter().any(|tab| tab.id == id)
                || crate::recovery::Recovery::load(&path).is_none()
            {
                continue;
            }
            self.next_tab_id = self.next_tab_id.max(id + 1);
            let tab = self.make_tab(mode, id, Some(path), window, cx);
            self.tabs.push(tab);
        }
    }

    /// Reopens the tabs that were open when Fast last closed, in their order and with
    /// their names, and shows the one that was shown.
    fn restore_session(
        &mut self,
        session: crate::config::SavedSession,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let directory = self.model.read(cx).paths.data_dir.join("recovery");
        let default_analyse = self.analyse.read(cx).recovery_path().to_path_buf();
        let default_play = self.play.read(cx).recovery_path().to_path_buf();
        // The first tab of each kind takes the screen made at start; later ones get their own.
        let mut used: Vec<Mode> = Vec::new();
        let mut tabs: Vec<WorkspaceTab> = Vec::new();
        for saved in &session.tabs {
            let mode = Mode::from_saved(saved.kind);
            if saved.id == 0 || saved.id == u64::MAX || tabs.iter().any(|tab| tab.id == saved.id) {
                continue;
            }
            // Only a bare file name is accepted, so a damaged config cannot point elsewhere.
            let path = saved
                .recovery
                .as_deref()
                .and_then(|name| std::path::Path::new(name).file_name())
                .map(|name| {
                    let default = match mode {
                        Mode::Analyse => &default_analyse,
                        _ => &default_play,
                    };
                    if default.file_name() == Some(name) {
                        default.clone()
                    } else {
                        directory.join(name)
                    }
                });
            let first = !used.contains(&mode);
            let reuse = match mode {
                Mode::Analyse => first && path.as_ref() == Some(&default_analyse),
                Mode::Play => first && path.as_ref() == Some(&default_play),
                Mode::Database | Mode::Train | Mode::Settings => first,
                Mode::Tournament | Mode::TestPositions => false,
            };
            if matches!(mode, Mode::Train | Mode::Settings) && !first {
                continue;
            }
            let mut tab = match reuse.then(|| self.existing_content(mode)).flatten() {
                Some(content) => WorkspaceTab {
                    id: saved.id,
                    content,
                    name: None,
                },
                None => self.make_tab(mode, saved.id, path, window, cx),
            };
            if let TabContent::Analyse(screen) = &tab.content {
                let choice = saved.explorer_db.clone();
                screen.update(cx, |screen, cx| screen.restore_explorer_choice(choice, cx));
            }
            tab.name = saved
                .name
                .as_deref()
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(|name| SharedString::from(name.to_owned()));
            used.push(mode);
            tabs.push(tab);
        }
        if tabs.is_empty() {
            return;
        }
        // New tabs must not pick up a recovery file left by a tab closed earlier.
        let highest_file = std::fs::read_dir(&directory)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|entry| {
                let name = entry.file_name();
                let stem = std::path::Path::new(&name)
                    .file_stem()?
                    .to_str()?
                    .to_owned();
                stem.rsplit('-').next()?.parse::<u64>().ok()
            })
            .filter(|id| *id < u64::MAX)
            .max()
            .unwrap_or(0);
        let highest_tab = tabs.iter().map(|tab| tab.id).max().unwrap_or(0);
        self.next_tab_id = highest_file.max(highest_tab) + 1;
        let active = tabs
            .get(session.active)
            .or(tabs.first())
            .map(|tab| tab.id)
            .unwrap_or(1);
        self.last_tab = tabs
            .iter()
            .map(|tab| (tab.content.mode(), tab.id))
            .collect();
        self.tabs = tabs;
        // The screens made at start may not be in any tab, so none keeps running unseen.
        self.analyse
            .update(cx, |screen, cx| screen.set_visible(false, cx));
        self.train
            .update(cx, |screen, cx| screen.set_visible(false, cx));
        self.active_tab = u64::MAX;
        self.activate_tab(active, cx);
    }

    /// Records the open tabs in the config, so the next start reopens them.
    fn save_session(&self, cx: &mut Context<Self>) {
        let file_name = |path: &std::path::Path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .map(str::to_owned)
        };
        let tabs = self
            .tabs
            .iter()
            .map(|tab| crate::config::SavedTab {
                kind: tab.content.mode().saved(),
                id: tab.id,
                name: tab.name.as_ref().map(|name| name.to_string()),
                recovery: match &tab.content {
                    TabContent::Analyse(screen) => file_name(screen.read(cx).recovery_path()),
                    TabContent::Play(screen) => file_name(screen.read(cx).recovery_path()),
                    _ => None,
                },
                explorer_db: match &tab.content {
                    TabContent::Analyse(screen) => screen
                        .read(cx)
                        .explorer_choice()
                        .map(|path| path.to_path_buf()),
                    _ => None,
                },
            })
            .collect();
        let active = self
            .tabs
            .iter()
            .position(|tab| tab.id == self.active_tab)
            .unwrap_or(0);
        let session = Some(crate::config::SavedSession { tabs, active });
        self.model.update(cx, |model, cx| {
            if model.config.session != session {
                model.config.session = session;
                model.schedule_config_save(cx);
            }
        });
    }

    /// Opens a field in the tab's place to type a new name for it.
    fn start_rename(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.iter().find(|tab| tab.id == id) else {
            return;
        };
        let current = match &tab.name {
            Some(name) => name.to_string(),
            None => tab.content.title(cx).0.to_owned(),
        };
        let input = cx.new(|cx| InputState::new(window, cx));
        input.update(cx, |input, cx| {
            input.set_value(current, window, cx);
            input.focus(window, cx);
        });
        self._rename_sub =
            Some(
                cx.subscribe_in(&input, window, move |this, _, event, window, cx| {
                    if matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                        this.finish_rename(true, window, cx);
                    }
                }),
            );
        self.renaming = Some((id, input));
        cx.notify();
    }

    /// Closes the rename field, keeping the typed name when `keep` is set. An empty
    /// name gives the tab back its usual title.
    fn finish_rename(&mut self, keep: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some((id, input)) = self.renaming.take() else {
            return;
        };
        self._rename_sub = None;
        if keep {
            let typed = input.read(cx).value().trim().to_owned();
            if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id) {
                let default = tab.content.title(cx).0;
                tab.name = (!typed.is_empty() && typed != default).then(|| typed.into());
            }
            self.save_session(cx);
        }
        self.focus.focus(window, cx);
        cx.notify();
    }

    /// Covers the content area when the + button is open: one card per kind of tab.
    fn new_tab_picker(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let choices: [(Mode, &'static str, &str, &str); 6] = [
            (
                Mode::Analyse,
                "icons/microscope.svg",
                ui::i18n::t("Analyse"),
                ui::i18n::t(
                    "A board, engine analysis and the move list for a new position or game.",
                ),
            ),
            (
                Mode::Play,
                "icons/swords.svg",
                ui::i18n::t("Play"),
                ui::i18n::t("Play a game against an engine, with clocks."),
            ),
            (
                Mode::Database,
                "icons/database.svg",
                ui::i18n::t("Database"),
                ui::i18n::t("Browse, search and import your game collections."),
            ),
            (
                Mode::Train,
                "icons/graduation-cap.svg",
                ui::i18n::t("Train"),
                ui::i18n::t("Drill your repertoire with spaced repetition."),
            ),
            (
                Mode::Tournament,
                "icons/trophy.svg",
                ui::i18n::t("Tournament"),
                ui::i18n::t("Engine-vs-engine matches run with fastchess."),
            ),
            (
                Mode::TestPositions,
                "icons/flask-conical.svg",
                ui::i18n::t("Test positions"),
                ui::i18n::t("Run engines over an EPD suite to find best moves or mates."),
            ),
        ];
        // Two per row, in one column with the header so their edges line up.
        let mut cards = div().grid().grid_cols(2).gap(px(16.)).w_full();
        for (index, (mode, icon_path, title, description)) in choices.into_iter().enumerate() {
            let pick = cx.entity();
            cards = cards.child(
                v_flex()
                    .id(("new-tab-card", index))
                    .min_w_0()
                    .min_h(px(132.))
                    .p(px(24.))
                    .gap(px(10.))
                    .rounded(px(12.))
                    .bg(rgb(T.surface))
                    .border_1()
                    .border_color(rgb(T.border))
                    .cursor_pointer()
                    .hover(|card| card.border_color(rgb(T.border_strong)).bg(rgb(T.selected)))
                    .child(
                        h_flex()
                            .gap(px(10.))
                            .items_center()
                            .child(ui::widgets::icon(icon_path, px(20.), T.accent))
                            .child(
                                div()
                                    .text_size(px(19.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(rgb(T.fg))
                                    .child(title),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(14.))
                            .line_height(px(20.))
                            .text_color(rgb(T.muted))
                            .child(description),
                    )
                    .on_click(move |_, window, cx| {
                        pick.update(cx, |this, cx| {
                            this.new_tab_menu_open = false;
                            this.new_tab(mode, window, cx)
                        })
                    }),
            );
        }
        let close = cx.entity();
        v_flex()
            .id("new-tab-picker")
            .absolute()
            .inset_0()
            .occlude()
            .items_center()
            .justify_center()
            .p(px(16.))
            .bg(rgb(T.bg))
            .child(
                v_flex()
                    .w_full()
                    .max_w(px(820.))
                    .gap(px(20.))
                    .child(
                        h_flex()
                            .w_full()
                            .justify_between()
                            .items_center()
                            .child(
                                div()
                                    .text_size(px(22.))
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(rgb(T.fg))
                                    .child(ui::i18n::t("New tab")),
                            )
                            .child(
                                btn("new-tab-picker-close", "×")
                                    .h(28.)
                                    .w(28.)
                                    .pad(4.)
                                    .tooltip(ui::i18n::t("Close"))
                                    .on_click(move |_, cx| {
                                        close.update(cx, |this, cx| {
                                            this.new_tab_menu_open = false;
                                            cx.notify();
                                        })
                                    }),
                            ),
                    )
                    .child(cards)
                    .child({
                        let random = cx.entity();
                        let create = cx.entity();
                        h_flex()
                            .gap(px(8.))
                            .child(
                                div().w(px(80.)).child(
                                    crate::controls::text_input(&self.chess960_number).mono(),
                                ),
                            )
                            .child(
                                btn("new-960-random", "Random")
                                    .icon("icons/dices.svg")
                                    .compact()
                                    .on_click(move |window, cx| {
                                        random.update(cx, |this, cx| {
                                            let n = std::time::SystemTime::now()
                                                .duration_since(std::time::UNIX_EPOCH)
                                                .map_or(518, |time| {
                                                    (time.subsec_nanos() % 960) as u16
                                                });
                                            this.chess960_number.update(cx, |input, cx| {
                                                input.set_value(n.to_string(), window, cx)
                                            });
                                            cx.notify();
                                        });
                                    }),
                            )
                            .child(ui::widgets::mono(
                                self.chess960_number
                                    .read(cx)
                                    .value()
                                    .trim()
                                    .parse::<u16>()
                                    .ok()
                                    .filter(|n| *n < 960)
                                    .map_or_else(
                                        || ui::i18n::t("Enter a number from 0 to 959").to_owned(),
                                        |n| format!("#{n} {}", chess_core::chess960::back_rank(n)),
                                    ),
                            ))
                            .child(btn("new-960", "New Chess960 game…").compact().on_click(
                                move |window, cx| {
                                    create.update(cx, |this, cx| {
                                        let Some(n) = this
                                            .chess960_number
                                            .read(cx)
                                            .value()
                                            .trim()
                                            .parse::<u16>()
                                            .ok()
                                            .filter(|n| *n < 960)
                                        else {
                                            this.model.toast("Enter a number from 0 to 959", cx);
                                            return;
                                        };
                                        this.model.update(cx, |model, cx| {
                                            model.config.chess960_number = n;
                                            model.save_config(cx);
                                        });
                                        this.new_tab_menu_open = false;
                                        this.new_tab(Mode::Analyse, window, cx);
                                        this.analyse.update(cx, |screen, cx| {
                                            screen.load_tree(
                                                chess_core::GameTree::chess960(n),
                                                window,
                                                cx,
                                            )
                                        });
                                    });
                                },
                            ))
                    }),
            )
    }

    fn tab_bar(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        // A new or newly chosen tab scrolls into view.
        if self.tab_in_view.get() != self.active_tab
            && let Some(index) = self.tabs.iter().position(|tab| tab.id == self.active_tab)
        {
            self.tab_in_view.set(self.active_tab);
            self.tab_scroll.scroll_to_item(index);
        }
        let scroll = self.tab_scroll.clone();
        let mut tabs = h_flex()
            .id("workspace-tabs")
            .min_w_0()
            .h_full()
            .items_end()
            .gap(px(2.))
            .overflow_x_scroll()
            .track_scroll(&self.tab_scroll)
            // A plain mouse wheel scrolls up and down; in the strip it moves along the tabs.
            .on_scroll(move |event: &ScrollEvent, window, cx| {
                let delta = event.delta.pixel_delta(window.line_height());
                let step = if delta.x.abs() > delta.y.abs() {
                    delta.x
                } else {
                    delta.y
                };
                let max = scroll.max_offset().x;
                if max <= px(0.) || step == px(0.) {
                    return;
                }
                let mut offset = scroll.offset();
                offset.x = (offset.x + step).clamp(-max, px(0.));
                scroll.set_offset(offset);
                cx.stop_propagation();
                window.refresh();
            });
        for tab in &self.tabs {
            let id = tab.id;
            let select = cx.entity();
            let close = select.clone();
            let duplicate = select.clone();
            let rename = select.clone();
            let analysis = matches!(tab.content, TabContent::Analyse(_));
            let active = id == self.active_tab;
            let rename_input = self
                .renaming
                .as_ref()
                .filter(|(renaming, _)| *renaming == id)
                .map(|(_, input)| input.clone());
            let (screen, detail) = tab.content.title(cx);
            let label: SharedString = tab.name.clone().unwrap_or_else(|| screen.into());
            let detail = if tab.name.is_some() { None } else { detail };
            tabs = tabs.child(
                h_flex()
                    .id(("tab", id as usize))
                    .on_mouse_up(
                        MouseButton::Middle,
                        cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            this.close_tab(id, window, cx);
                        }),
                    )
                    .on_drag(
                        DraggedTab {
                            id,
                            title: label.clone(),
                        },
                        |drag, _, _, cx| cx.new(|_| drag.clone()),
                    )
                    .drag_over::<DraggedTab>(|style, _, _, _| style.border_color(rgb(T.accent)))
                    .on_drop(cx.listener(move |this, drag: &DraggedTab, _, cx| {
                        let source = this.tabs.iter().position(|tab| tab.id == drag.id);
                        let target = this.tabs.iter().position(|tab| tab.id == id);
                        if let (Some(source), Some(target)) = (source, target) {
                            let tab = this.tabs.remove(source);
                            this.tabs.insert(target, tab);
                            this.save_session(cx);
                            cx.notify();
                        }
                    }))
                    .flex_none()
                    .gap(px(2.))
                    .h(px(33.))
                    .pl(px(11.))
                    .pr(px(4.))
                    .rounded_t(px(6.))
                    .border_t_1()
                    .border_l_1()
                    .border_r_1()
                    // The open tab takes the screen's background and has no bottom edge, so it
                    // runs straight into the screen below it.
                    .map(|tab| {
                        if active {
                            tab.bg(rgb(T.bg)).border_color(rgb(T.border))
                        } else {
                            tab.border_color(gpui::transparent_black())
                                .hover(|tab| tab.bg(rgb(T.hover)))
                        }
                    })
                    .when_some(rename_input, |tab, input| {
                        let cancel = cx.entity();
                        tab.child(
                            div()
                                .w(px(180.))
                                .on_key_down(move |event: &gpui::KeyDownEvent, window, cx| {
                                    if event.keystroke.key == "escape" {
                                        cx.stop_propagation();
                                        cancel.update(cx, |this, cx| {
                                            this.finish_rename(false, window, cx)
                                        });
                                    }
                                })
                                .child(crate::controls::text_input(&input).height(26.)),
                        )
                    })
                    .when(
                        self.renaming
                            .as_ref()
                            .is_none_or(|(renaming, _)| *renaming != id),
                        |tab| {
                            tab.child(
                                div()
                                    .id(("tab-select", id as usize))
                                    .cursor_pointer()
                                    .text_size(px(13.))
                                    .text_color(rgb(if active { T.fg } else { T.muted }))
                                    .tooltip(|window, cx| {
                                        gpui_component::tooltip::Tooltip::new(ui::i18n::t(
                                            "Double-click to rename",
                                        ))
                                        .build(window, cx)
                                    })
                                    .on_click(move |event, window, cx| {
                                        select.update(cx, |this, cx| {
                                            this.activate_tab(id, cx);
                                            if event.click_count() == 2 {
                                                this.start_rename(id, window, cx);
                                            }
                                        })
                                    })
                                    .child({
                                        h_flex()
                                            .gap(px(6.))
                                            .child(
                                                ui::widgets::mono(id.to_string())
                                                    .text_color(rgb(T.faint)),
                                            )
                                            .child(
                                                div()
                                                    .font_weight(if active {
                                                        FontWeight::SEMIBOLD
                                                    } else {
                                                        FontWeight::MEDIUM
                                                    })
                                                    .child(label),
                                            )
                                            .children(detail.map(|detail| {
                                                div()
                                                    .when(active, |d| d.text_color(rgb(T.muted)))
                                                    .child(detail)
                                            }))
                                    }),
                            )
                        },
                    )
                    .child(
                        btn(("close-tab", id as usize), "×")
                            .kind(ui::widgets::BtnKind::Ghost)
                            .h(24.)
                            .w(24.)
                            .pad(3.)
                            .tooltip(ui::i18n::t("Close tab; reopen with Ctrl/Cmd+Shift+T"))
                            .on_click(move |window, cx| {
                                close.update(cx, |this, cx| this.close_tab(id, window, cx))
                            }),
                    )
                    .context_menu(move |menu, _, _| {
                        let rename = rename.clone();
                        let menu =
                            menu.item(PopupMenuItem::new(ui::i18n::t("Rename tab")).on_click(
                                move |_, window, cx| {
                                    rename.update(cx, |this, cx| this.start_rename(id, window, cx))
                                },
                            ));
                        if !analysis {
                            return menu;
                        }
                        let duplicate = duplicate.clone();
                        menu.item(PopupMenuItem::new("Duplicate analysis tab").on_click(
                            move |_, window, cx| {
                                duplicate.update(cx, |this, cx| {
                                    let Some(TabContent::Analyse(source)) = this
                                        .tabs
                                        .iter()
                                        .find(|tab| tab.id == id)
                                        .map(|tab| tab.content.clone())
                                    else {
                                        return;
                                    };
                                    let tree = source.read(cx).tree.clone();
                                    let current = source.read(cx).current;
                                    this.open_analysis(tree, window, cx);
                                    this.analyse
                                        .update(cx, |screen, cx| screen.jump(current, window, cx));
                                });
                            },
                        ))
                    }),
            );
        }
        let add = cx.entity();
        let tip = match self.model.read(cx).config.shortcuts.get("new_tab") {
            Some(key) if !key.is_empty() => {
                format!(
                    "{} ({})",
                    ui::i18n::t("Open a new tab"),
                    crate::screens::settings::key_names(key).join("+")
                )
            }
            _ => ui::i18n::t("Open a new tab").to_owned(),
        };
        h_flex()
            .relative()
            .flex_none()
            .h(px(38.))
            .px(px(10.))
            .gap(px(6.))
            .items_end()
            .bg(rgb(T.surface))
            // The bottom rule is drawn first so the open tab, painted over it, can hide it.
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .bottom_0()
                    .h(px(1.))
                    .bg(rgb(T.border)),
            )
            // The strip only takes the width its tabs need, so New tab sits right after the
            // last one; with many tabs the strip scrolls and the button stays in view.
            .child(tabs)
            // New tab is drawn as one more tab: closed it looks like the inactive tabs, and
            // while its picker is open it looks like the open tab.
            .child({
                let open = self.new_tab_menu_open;
                h_flex()
                    .id("new-tab")
                    .flex_none()
                    .h(px(33.))
                    .px(px(11.))
                    .gap(px(6.))
                    .rounded_t(px(6.))
                    .border_t_1()
                    .border_l_1()
                    .border_r_1()
                    .cursor_pointer()
                    .text_size(px(13.))
                    .font_weight(FontWeight::MEDIUM)
                    .map(|tab| {
                        if open {
                            tab.bg(rgb(T.bg))
                                .border_color(rgb(T.border))
                                .text_color(rgb(T.fg))
                        } else {
                            tab.border_color(gpui::transparent_black())
                                .text_color(rgb(T.muted))
                                .hover(|tab| tab.bg(rgb(T.hover)).text_color(rgb(T.fg)))
                        }
                    })
                    .tooltip(move |window, cx| {
                        gpui_component::tooltip::Tooltip::new(tip.clone()).build(window, cx)
                    })
                    .on_click(move |_, _, cx| {
                        add.update(cx, |this, cx| {
                            this.new_tab_menu_open = !this.new_tab_menu_open;
                            cx.notify();
                        })
                    })
                    .child(ui::widgets::icon(
                        "icons/plus.svg",
                        px(14.),
                        if open { T.fg } else { T.muted },
                    ))
                    .child(ui::i18n::t("New tab"))
            })
    }

    /// Runs an Analyse-only shortcut, ignored on other screens where its effect
    /// would be invisible.
    fn in_analyse(
        &mut self,
        cx: &mut Context<Self>,
        action: impl FnOnce(&mut AnalyseScreen, &mut Context<AnalyseScreen>),
    ) {
        if self.mode == Mode::Analyse {
            self.analyse.update(cx, action);
        }
    }

    /// Refreshes local game suggestions without blocking typing or the board.
    fn update_search(&mut self, cx: &mut Context<Self>) {
        let query = self.search.read(cx).value().trim().to_owned();
        self.search_task = None;
        self.search_results.clear();
        self.search_selected = None;
        self.search_open = !query.is_empty();
        self.search_pending = self.search_open;
        self.search_failed = false;
        if query.is_empty() {
            cx.notify();
            return;
        }
        let entries = self.model.read(cx).databases.clone();
        self.search_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(200))
                .await;
            let result = cx
                .background_spawn(async move {
                    let mut found = Vec::new();
                    let mut failed = false;
                    for entry in entries {
                        match db::Database::open(&entry.path)
                            .and_then(|db| db.search_games(&query, 8 - found.len()))
                        {
                            Ok(rows) => {
                                found.extend(rows.into_iter().map(|row| (entry.path.clone(), row)))
                            }
                            Err(_) => failed = true,
                        }
                        if found.len() == 8 {
                            break;
                        }
                    }
                    (found, failed)
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.search_results = result.0;
                this.search_failed = result.1;
                this.search_pending = false;
                cx.notify();
            });
        }));
        cx.notify();
    }

    /// Opens a suggested game after reading its moves in the background.
    fn open_search_result(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some((path, row)) = self.search_results.get(index) else {
            return;
        };
        let (path, id) = (path.clone(), row.id);
        self.search_open = false;
        let task =
            cx.background_spawn(async move { db::Database::open(path).and_then(|db| db.game(id)) });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |this, window, cx| match result {
                Ok(Some(game)) => this.open_pgn(&game.to_pgn(), 0, window, cx),
                _ => this
                    .model
                    .update(cx, |model, cx| model.toast("Could not open game", cx)),
            });
        })
        .detach();
        cx.notify();
    }

    /// Runs the typed position or player search when Enter is pressed.
    fn run_search(&mut self, query: &str, window: &mut Window, cx: &mut Context<Self>) {
        if query.is_empty() {
            return;
        }
        self.search_open = false;
        // A FEN opens in Analyse; anything else searches players in the database.
        if chess_core::GameTree::from_fen(query).is_ok() {
            let fen = query.to_owned();
            self.new_tab(Mode::Analyse, window, cx);
            self.analyse
                .update(cx, |analyse, cx| analyse.load_fen(&fen, window, cx));
        } else {
            let text = query.to_owned();
            self.database
                .update(cx, |database, cx| database.search_player(&text, window, cx));
            self.set_mode(Mode::Database, cx);
        }
        self.focus.focus(window, cx);
    }

    /// Downloads the new games of a saved Lichess or Chess.com account, showing the
    /// Database tab where its progress appears.
    pub fn update_account(&mut self, index: usize, cx: &mut Context<Self>) {
        self.set_mode(Mode::Database, cx);
        self.database
            .update(cx, |database, cx| database.update_account(index, cx));
    }

    /// Shows the Database tab on `path`, filtered to games reaching the position with
    /// this hash, as when a count in the opening tree is clicked.
    pub fn show_position_in_database(
        &mut self,
        path: std::path::PathBuf,
        hash: u64,
        label: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.set_mode(Mode::Database, cx);
        self.database.update(cx, |database, cx| {
            database.show_position(path, hash, label, window, cx)
        });
    }

    /// Opens Settings on one engine's full option list.
    pub fn open_engine_settings(
        &mut self,
        id: engine::EngineId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.settings.update(cx, |settings, cx| {
            settings.select_engine(Some(id), window, cx);
            settings.set_page(SettingsPage::Engines, window, cx);
        });
        self.set_mode(Mode::Settings, cx);
    }

    fn submit_path(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let multiple = self
            .model
            .read(cx)
            .pending_path
            .as_ref()
            .is_some_and(|pending| pending.multiple);
        let text = if multiple {
            self.paths_input.read(cx).value().to_string()
        } else {
            self.path_input.read(cx).value().to_string()
        };
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        let pending = self.model.update(cx, |model, cx| {
            cx.notify();
            model.pending_path.take()
        });
        self.path_input
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.paths_input
            .update(cx, |input, cx| input.set_value("", window, cx));
        if let Some(pending) = pending {
            let paths = if multiple {
                text.lines()
                    .map(str::trim)
                    .filter(|line| !line.is_empty())
                    .map(std::path::PathBuf::from)
                    .collect()
            } else {
                vec![std::path::PathBuf::from(text)]
            };
            // File callbacks may update this workspace/window; run after its
            // current event handler has released both update guards.
            cx.defer(move |cx| (pending.on_paths)(paths, cx));
        }
    }

    /// Typed-path fallback, for desktops with no file dialog service (such as WSL).
    fn path_bar(&self, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        let title = self.model.read(cx).pending_path.as_ref()?.title.clone();
        let multiple = self.model.read(cx).pending_path.as_ref()?.multiple;
        let this = cx.entity();
        let cancel = self.model.clone();
        Some(
            h_flex()
                .flex_none()
                .w_full()
                .gap(px(10.))
                .px(px(21.))
                .py(px(8.))
                .bg(rgb(T.surface_2))
                .border_b_1()
                .border_color(rgb(T.border))
                .child(div().flex_none().text_color(rgb(T.fg_2)).child(format!(
                    "{title}: {}",
                    if multiple {
                        "enter one file path per line"
                    } else {
                        "enter a path"
                    }
                )))
                .child(div().flex_1().child(if multiple {
                    gpui_component::input::Textarea::new(&self.paths_input)
                        .h(px(88.))
                        .into_any_element()
                } else {
                    crate::controls::text_input(&self.path_input).into_any_element()
                }))
                .child(
                    btn("path-ok", ui::i18n::t("Open"))
                        .primary()
                        .on_click(move |window, cx| {
                            this.update(cx, |this, cx| this.submit_path(window, cx));
                        }),
                )
                .child(
                    btn("path-cancel", ui::i18n::t("Cancel")).on_click(move |_, cx| {
                        cancel.update(cx, |model, cx| {
                            model.pending_path = None;
                            cx.notify();
                        });
                    }),
                ),
        )
    }

    fn breadcrumb(&self, cx: &App) -> SharedString {
        match self.mode {
            Mode::Analyse => self.analyse.read(cx).breadcrumb(),
            Mode::Database => self.database.read(cx).breadcrumb(),
            Mode::Train => self.model.read(cx).repertoire.name.clone().into(),
            Mode::Play => self.play.read(cx).breadcrumb(),
            Mode::Settings => {
                format!("Settings › {}", self.settings.read(cx).page().title()).into()
            }
            Mode::Tournament => self.tournament.as_ref().map_or_else(
                || ui::i18n::t("Tournament").into(),
                |screen| screen.read(cx).breadcrumb(),
            ),
            Mode::TestPositions => self.test_positions.as_ref().map_or_else(
                || ui::i18n::t("Test positions").into(),
                |screen| screen.read(cx).breadcrumb(),
            ),
        }
    }

    fn mode_button(&self, mode: Mode, label: &'static str, cx: &mut Context<Self>) -> Btn {
        let active = self.mode == mode;
        btn(("mode", mode as u32), label)
            .h(31.)
            .pad(14.)
            .text(14.)
            .kind(if active {
                BtnKind::Active
            } else {
                BtnKind::Ghost
            })
            .on_click({
                let this = cx.entity();
                move |_, cx| this.update(cx, |this, cx| this.set_mode(mode, cx))
            })
    }

    fn logo() -> impl IntoElement {
        let square = |filled: bool| {
            div()
                .size(px(9.5))
                .when(filled, |sq| sq.bg(rgb(T.fg)))
                .when(!filled, |sq| sq.border_1().border_color(rgb(T.fg)))
        };
        v_flex()
            .gap(px(1.))
            .child(
                h_flex()
                    .gap(px(1.))
                    .child(square(true))
                    .child(square(false)),
            )
            .child(
                h_flex()
                    .gap(px(1.))
                    .child(square(false))
                    .child(square(true)),
            )
    }

    /// The engine button's text, when the current mode shows one.
    fn engine_button_label(&self, cx: &App) -> Option<String> {
        (self.mode == Mode::Analyse).then(|| {
            let names: Vec<String> = self
                .model
                .read(cx)
                .analysis_engines()
                .iter()
                .map(|engine| engine.name.clone())
                .collect();
            if names.is_empty() {
                "No engine".to_owned()
            } else {
                names.join(" + ")
            }
        })
    }

    fn engine_button(&self, cx: &mut Context<Self>) -> Option<Btn> {
        let model = self.model.read(cx);
        let engines = model.analysis_engines();
        let live_on = self.analyse.read(cx).live_on(cx);
        let names: Vec<String> = engines.iter().map(|engine| engine.name.clone()).collect();
        let ok = !engines.is_empty()
            && engines
                .iter()
                .all(|engine| model.engine_status.get(&engine.id) != Some(&EngineStatus::Error));
        let label = if names.is_empty() {
            "No engine".to_owned()
        } else {
            names.join(" + ")
        };
        let this = cx.entity();
        Some(
            btn("engines-status", label)
                .pad(14.)
                .text(13.)
                .dot(if ok && live_on { T.good } else { T.faint })
                .tooltip(if ok && live_on {
                    "Engines running. Press Space to pause; click for engine settings."
                } else {
                    "Engines stopped. Press Space to start; click for engine settings."
                })
                .on_click(move |window, cx| {
                    this.update(cx, |this, cx| {
                        this.settings.update(cx, |settings, cx| {
                            settings.set_page(SettingsPage::Engines, window, cx);
                        });
                        this.set_mode(Mode::Settings, cx);
                    });
                }),
        )
    }

    fn right_controls(
        &self,
        viewport_w: f32,
        controls: bool,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let this = cx.entity();
        let mut row = h_flex().gap(px(10.));
        // The search box takes what is left beside the centered mode switch.
        let engine_w = self
            .engine_button_label(cx)
            .map_or(0., |label| 60. + label.len() as f32 * 7.);
        let others = match self.mode {
            Mode::Analyse => engine_w + 10.,
            _ => 0.,
        } + 48.
            + 10.
            + if controls {
                WINDOW_CONTROLS_W + 10.
            } else {
                0.
            };
        let free = viewport_w / 2. - MODE_SWITCH_HALF_W - 21. - others;
        let search_w = match self.mode {
            Mode::Analyse | Mode::Database if free >= 120. => Some(free.min(260.)),
            _ => None,
        };
        if let Some(width) = search_w {
            let shortcut = self.model.read(cx).config.shortcuts.get("search").cloned();
            let mut field = crate::controls::text_input(&self.search)
                .height(34.)
                .icon("icons/search.svg");
            // The shortcut only shows when the box has room for it beside the placeholder.
            if let Some(key) = shortcut.filter(|key| !key.is_empty() && width >= 220.) {
                field = field.hint(crate::screens::settings::shortcut_hint(&key));
            }
            let mut search_box =
                div()
                    .relative()
                    .w(px(width))
                    .child(field)
                    .capture_key_down(cx.listener(
                        |this, event: &gpui::KeyDownEvent, window, cx| {
                            if !this.search_open {
                                return;
                            }
                            match event.keystroke.key.as_str() {
                                "escape" => {
                                    this.search_open = false;
                                    this.focus.focus(window, cx);
                                }
                                "enter" if this.search_selected.is_some() => {
                                    this.open_search_result(this.search_selected.unwrap(), cx);
                                }
                                "down" if !this.search_results.is_empty() => {
                                    this.search_selected =
                                        Some(this.search_selected.map_or(0, |index| {
                                            (index + 1) % this.search_results.len()
                                        }));
                                }
                                "up" if !this.search_results.is_empty() => {
                                    this.search_selected = Some(
                                        this.search_selected
                                            .unwrap_or(0)
                                            .checked_sub(1)
                                            .unwrap_or(this.search_results.len() - 1),
                                    );
                                }
                                _ => return,
                            }
                            cx.stop_propagation();
                            cx.notify();
                        },
                    ));
            if self.search_open {
                let mut results = v_flex()
                    .absolute()
                    .top(px(38.))
                    .right_0()
                    .w(px(440.))
                    .rounded(px(6.))
                    .bg(rgb(T.surface))
                    .border_1()
                    .border_color(rgb(T.border))
                    .text_size(px(13.))
                    .text_color(rgb(T.fg))
                    .occlude()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                        this.search_open = false;
                        cx.notify();
                    }));
                let status = if self.search_pending {
                    "Searching…"
                } else if self.search_failed {
                    "Some databases could not be searched"
                } else if self.search_results.is_empty() {
                    "No matching games"
                } else {
                    "Matching games"
                };
                results = results.child(
                    div()
                        .px(px(16.))
                        .py(px(8.))
                        .text_color(rgb(T.muted))
                        .child(ui::i18n::t(status)),
                );
                for (index, (_, game)) in self.search_results.iter().enumerate() {
                    let target = this.clone();
                    results = results.child(
                        div().border_t_1().border_color(rgb(T.divider)).child(
                            btn(
                                ("search-result", index),
                                format!(
                                    "{} — {}  {}  {}",
                                    game.white, game.black, game.result, game.date
                                ),
                            )
                            .compact()
                            .kind(BtnKind::Ghost)
                            .mono()
                            .full()
                            .selected(self.search_selected == Some(index))
                            .on_click(move |_, cx| {
                                target.update(cx, |this, cx| this.open_search_result(index, cx));
                            }),
                        ),
                    );
                }
                search_box = search_box.child(deferred(results));
            }
            row = row.child(search_box);
        }
        match self.mode {
            Mode::Analyse => {
                if let Some(button) = self.engine_button(cx) {
                    row = row.child(button);
                }
            }
            Mode::Train => {
                let settings_this = this.clone();
                row = row.child(btn("repertoires", ui::i18n::t("Repertoires")).on_click(
                    move |window, cx| {
                        settings_this.update(cx, |this, cx| {
                            this.train
                                .update(cx, |train, cx| train.prompt_open(window, cx));
                        });
                    },
                ));
            }
            _ => {}
        }
        let settings_active = self.mode == Mode::Settings;
        row.child(
            icon_btn("settings", "icons/settings.svg")
                .w(48.)
                .selected(settings_active)
                .tooltip(ui::i18n::t("Settings"))
                .on_click(move |_, cx| {
                    this.update(cx, |this, cx| {
                        let next = if this.mode == Mode::Settings {
                            Mode::Analyse
                        } else {
                            Mode::Settings
                        };
                        this.set_mode(next, cx);
                    });
                }),
        )
        .when(controls, |row| row.child(window_controls(window)))
        // Clicks here must not start a window drag.
        .id("top-bar-controls")
        .occlude()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
    }

    fn top_bar(
        &self,
        viewport_w: f32,
        controls: bool,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let crumb = self.breadcrumb(cx);
        // The drawn menu bar takes the breadcrumb's room on narrow windows.
        let show_crumb = self.menu_bar.is_none() || viewport_w >= 1360.;
        // Interactive parts block the drag strip below them; on Windows that strip is
        // the system caption, which would otherwise swallow their clicks.
        let menu_bar = self.menu_bar.clone().map(|bar| {
            div()
                .id("menu-bar")
                .occlude()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(bar)
        });
        div()
            .relative()
            .h(px(58.))
            .flex_none()
            .w_full()
            .bg(rgb(T.surface))
            .border_b_1()
            .border_color(rgb(T.border))
            .when(controls, |bar| bar.child(drag_region()))
            .child(
                h_flex()
                    .absolute()
                    .inset_0()
                    .px(px(21.))
                    .justify_between()
                    .child(
                        h_flex()
                            .gap(px(10.))
                            .child(Self::logo())
                            .child(
                                div()
                                    .font_family(SANS)
                                    .font_weight(FontWeight::BOLD)
                                    .text_size(px(15.))
                                    .text_color(rgb(T.fg))
                                    .child(ui::i18n::t("Fast")),
                            )
                            .children(menu_bar)
                            .when(show_crumb, |row| {
                                row.child(
                                    div()
                                        .text_size(px(13.))
                                        .text_color(rgb(T.muted))
                                        .child(SharedString::from(format!("/ {crumb}"))),
                                )
                            }),
                    )
                    .child(self.right_controls(viewport_w, controls, window, cx)),
            )
            .child(
                h_flex()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left_0()
                    .right_0()
                    .justify_center()
                    .child(
                        h_flex()
                            .id("mode-switch")
                            .occlude()
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .h(px(36.))
                            .px(px(2.))
                            .gap(px(2.))
                            .rounded(px(6.))
                            .bg(rgb(T.bg))
                            .child(self.mode_button(Mode::Analyse, ui::i18n::t("Analyse"), cx))
                            .child(self.mode_button(Mode::Database, ui::i18n::t("Database"), cx))
                            .child(self.mode_button(Mode::Train, ui::i18n::t("Train"), cx))
                            .child(self.mode_button(Mode::Play, ui::i18n::t("Play"), cx)),
                    ),
            )
    }

    fn toasts(&self, cx: &App) -> impl IntoElement + use<> {
        let model = self.model.read(cx);
        let mut column = v_flex()
            .absolute()
            .bottom(px(20.))
            .right(px(24.))
            .gap(px(8.));
        for toast in &model.toasts {
            let action = toast.action.as_ref().map(|action| {
                let (run, id, model) = (action.run.clone(), toast.id, self.model.clone());
                btn(("toast-action", toast.id as usize), action.label.clone())
                    .compact()
                    .on_click(move |window, cx| {
                        run(window, cx);
                        model.update(cx, |model, cx| model.dismiss_toast(id, cx));
                    })
            });
            column = column.child(
                h_flex()
                    .max_w(px(420.))
                    .gap(px(10.))
                    .px(px(14.))
                    .py(px(10.))
                    .rounded(px(6.))
                    .bg(rgb(T.selected))
                    .border_1()
                    .border_color(rgb(T.border_strong))
                    .text_size(px(14.))
                    .text_color(rgb(T.fg))
                    .child(div().flex_1().min_w_0().child(toast.text.clone()))
                    .children(action),
            );
        }
        deferred(column)
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // The stack reads the previous frame, which does not exist on the first render.
        let stack = if std::mem::take(&mut self.first_frame) {
            log::info!("window decorations: {:?}", window.window_decorations());
            Vec::new()
        } else {
            window.context_stack()
        };
        let typing = stack.iter().any(|context| {
            [
                "Input",
                ui::i18n::t("Select"),
                "Popover",
                "Menu",
                "List",
                "Dialog",
            ]
            .iter()
            .any(|name| context.contains(name))
        });
        let path_bar = self.path_bar(cx);
        let viewport_w = f32::from(window.viewport_size().width);
        let client_drawn = matches!(window.window_decorations(), Decorations::Client { .. });
        let controls = client_drawn || self.force_window_controls || DRAWS_TITLE_BAR;
        let on_button = stack.iter().any(|context| context.contains("FastButton"));
        let body = match self.mode {
            Mode::Analyse => self.analyse.clone().into_any_element(),
            Mode::Database => self.database.clone().into_any_element(),
            Mode::Train => self.train.clone().into_any_element(),
            Mode::Play => self.play.clone().into_any_element(),
            Mode::Settings => self.settings.clone().into_any_element(),
            Mode::Tournament => match &self.tournament {
                Some(screen) => screen.clone().into_any_element(),
                None => div().into_any_element(),
            },
            Mode::TestPositions => match &self.test_positions {
                Some(screen) => screen.clone().into_any_element(),
                None => div().into_any_element(),
            },
        };
        v_flex()
            .id("workspace")
            .relative()
            .track_focus(&self.focus)
            .key_context(TAB_CONTEXT)
            // One node carries both contexts: it must be an ancestor of the focused element.
            .when(!typing, |root| {
                root.key_context(
                    if on_button {
                        format!("{SHORTCUT_CONTEXT} {TAB_CONTEXT}")
                    } else {
                        format!("{SHORTCUT_CONTEXT} {SPACE_CONTEXT} {TAB_CONTEXT}")
                    }
                    .as_str(),
                )
            })
            .child(
                div()
                    .flex()
                    .flex_col()
                    .size_full()
                    .bg(rgb(T.bg))
                    .text_color(rgb(T.fg))
                    .font_family(SANS)
                    .text_size(px(14.))
                    .child(self.top_bar(viewport_w, controls, window, cx))
                    .child(self.tab_bar(cx))
                    .children(path_bar)
                    .child(
                        div()
                            .relative()
                            .flex_1()
                            .min_h_0()
                            .w_full()
                            .child(body)
                            .when(self.new_tab_menu_open, |area| {
                                area.child(self.new_tab_picker(cx))
                            }),
                    ),
            )
            .size_full()
            // Clicks on nothing focusable would drop keyboard focus and with it every shortcut.
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    if !window.default_prevented() {
                        window.prevent_default();
                        this.focus.focus(window, cx);
                    }
                }),
            )
            // The back and forward buttons on a mouse move through the tab history.
            .on_mouse_down(
                MouseButton::Navigate(gpui::NavigationDirection::Back),
                cx.listener(|this, _, _, cx| this.step_history(true, cx)),
            )
            .on_mouse_down(
                MouseButton::Navigate(gpui::NavigationDirection::Forward),
                cx.listener(|this, _, _, cx| this.step_history(false, cx)),
            )
            .on_key_down(cx.listener(move |this, event: &gpui::KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape"
                    && this.mode == Mode::Tournament
                    && let Some(screen) = this.tournament.clone()
                    && screen.update(cx, |screen, cx| screen.escape(cx))
                {
                    cx.stop_propagation();
                    return;
                }
                if event.keystroke.key == "escape"
                    && this.mode == Mode::Database
                    && this.database.update(cx, |screen, cx| screen.escape(cx))
                {
                    cx.stop_propagation();
                    return;
                }
                if !typing
                    && this.mode == Mode::Analyse
                    && this
                        .analyse
                        .update(cx, |screen, cx| screen.handle_pv_key(event, cx))
                {
                    cx.stop_propagation();
                }
            }))
            .on_action(cx.listener(|this, _: &PrevMove, window, cx| {
                this.analyse
                    .update(cx, |screen, cx| screen.step(-1, window, cx));
            }))
            .on_action(cx.listener(|this, _: &NextMove, window, cx| {
                this.analyse
                    .update(cx, |screen, cx| screen.step(1, window, cx));
            }))
            .on_action(cx.listener(|this, _: &FirstMove, window, cx| {
                this.analyse
                    .update(cx, |screen, cx| screen.go_first(window, cx));
            }))
            .on_action(cx.listener(|this, _: &LastMove, window, cx| {
                this.analyse
                    .update(cx, |screen, cx| screen.go_last(window, cx));
            }))
            .on_action(cx.listener(|this, _: &ToggleEngine, _, cx| {
                this.analyse
                    .update(cx, |screen, cx| screen.toggle_engine(cx));
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &FlipBoard, _, cx| match this.mode {
                Mode::Play => this.play.update(cx, |screen, cx| screen.flip(cx)),
                Mode::Train => this.train.update(cx, |screen, cx| screen.flip(cx)),
                _ => this.analyse.update(cx, |screen, cx| screen.flip(cx)),
            }))
            .on_action(cx.listener(|this, _: &FocusSearch, window, cx| {
                if !matches!(this.mode, Mode::Analyse | Mode::Database) {
                    this.set_mode(Mode::Analyse, cx);
                }
                this.search
                    .update(cx, |search, cx| search.focus(window, cx));
                this.update_search(cx);
            }))
            .on_action(cx.listener(|this, _: &SaveGame, _, cx| {
                this.analyse.update(cx, |screen, cx| screen.save_game(cx));
            }))
            .on_action(cx.listener(|this, _: &NewGame, window, cx| {
                this.new_tab(Mode::Analyse, window, cx);
                this.analyse
                    .update(cx, |screen, cx| screen.new_game(window, cx));
            }))
            .on_action(cx.listener(|this, _: &NewTab, window, cx| {
                let mode = match this.mode {
                    Mode::Play => Mode::Play,
                    Mode::Database => Mode::Database,
                    _ => Mode::Analyse,
                };
                this.new_tab(mode, window, cx);
            }))
            .on_action(cx.listener(|this, _: &CloseTab, window, cx| {
                this.close_tab(this.active_tab, window, cx)
            }))
            .on_action(cx.listener(|this, _: &ReopenTab, _, cx| this.reopen_tab(cx)))
            .on_action(cx.listener(|this, _: &NextTab, _, cx| this.cycle_tab(1, cx)))
            .on_action(cx.listener(|this, _: &PrevTab, _, cx| this.cycle_tab(-1, cx)))
            .on_action(cx.listener(|this, _: &TabBack, _, cx| this.step_history(true, cx)))
            .on_action(cx.listener(|this, _: &TabForward, _, cx| this.step_history(false, cx)))
            .on_action(cx.listener(|this, _: &OpenGame, window, cx| {
                this.new_tab(Mode::Analyse, window, cx);
                this.analyse
                    .update(cx, |screen, cx| screen.open_file(window, cx));
                this.set_mode(Mode::Analyse, cx);
            }))
            .on_action(cx.listener(|this, _: &SavePgnAs, _, cx| {
                this.analyse.update(cx, |screen, cx| screen.save_pgn_as(cx));
            }))
            .on_action(cx.listener(|this, _: &PasteGame, window, cx| {
                this.analyse
                    .update(cx, |screen, cx| screen.paste_game(window, cx));
                this.set_mode(Mode::Analyse, cx);
            }))
            .on_action(cx.listener(|this, _: &CopyFen, _, cx| {
                this.analyse
                    .update(cx, |screen, cx| screen.copy_fen_action(cx));
            }))
            .on_action(cx.listener(|this, _: &CopyPgn, _, cx| {
                this.analyse
                    .update(cx, |screen, cx| screen.copy_pgn_action(cx));
            }))
            .on_action(cx.listener(|this, _: &PauseEngine, _, cx| {
                this.analyse
                    .update(cx, |screen, cx| screen.toggle_pause(cx));
            }))
            .on_action(cx.listener(|this, _: &MoreLines, _, cx| {
                this.in_analyse(cx, |screen, cx| screen.change_lines(true, cx));
            }))
            .on_action(cx.listener(|this, _: &FewerLines, _, cx| {
                this.in_analyse(cx, |screen, cx| screen.change_lines(false, cx));
            }))
            .on_action(cx.listener(|this, _: &ToggleAllMoves, _, cx| {
                this.in_analyse(cx, |screen, cx| screen.toggle_all_moves(cx));
            }))
            .on_action(cx.listener(|this, _: &ToggleArrows, _, cx| {
                this.in_analyse(cx, |screen, cx| screen.toggle_quality_arrows(cx));
            }))
            .on_action(cx.listener(|this, _: &ToggleSortLines, _, cx| {
                this.in_analyse(cx, |screen, cx| screen.toggle_sort(cx));
            }))
            .on_action(cx.listener(|this, _: &NextExplorerDb, _, cx| {
                this.in_analyse(cx, |screen, cx| screen.next_explorer_db(cx));
            }))
            .on_action(cx.listener(|this, _: &NextGraph, _, cx| {
                this.in_analyse(cx, |screen, cx| screen.next_graph(cx));
            }))
            .on_action(cx.listener(|this, _: &ToggleScoresheet, _, cx| {
                this.in_analyse(cx, |screen, cx| screen.toggle_scoresheet_style(cx));
            }))
            .on_action(cx.listener(|this, _: &AnalyseBackward, window, cx| {
                this.set_mode(Mode::Analyse, cx);
                this.analyse
                    .update(cx, |screen, cx| screen.toggle_backward(window, cx));
            }))
            .on_action(cx.listener(|this, _: &EngineOutput, _, cx| {
                this.analyse
                    .update(cx, |screen, cx| screen.open_engine_output(cx));
            }))
            .on_action(cx.listener(|this, _: &ShowAnalyse, _, cx| this.set_mode(Mode::Analyse, cx)))
            .on_action(
                cx.listener(|this, _: &ShowDatabase, _, cx| this.set_mode(Mode::Database, cx)),
            )
            .on_action(cx.listener(|this, _: &ShowTrain, _, cx| this.set_mode(Mode::Train, cx)))
            .on_action(cx.listener(|this, _: &ShowPlay, _, cx| this.set_mode(Mode::Play, cx)))
            .on_action(
                cx.listener(|this, _: &ShowSettings, _, cx| this.set_mode(Mode::Settings, cx)),
            )
            .child(self.toasts(cx))
            .when(client_drawn && !window.is_maximized(), |root| {
                root.children(resize_handles())
            })
    }
}

/// Lets the top bar move the window when the app draws its own decorations.
fn drag_region() -> impl IntoElement {
    let region = div().id("window-drag").absolute().inset_0();
    if DRAWS_TITLE_BAR {
        // Windows treats this as its caption: dragging, double-click to maximize,
        // snapping and the system menu all work natively.
        return region.window_control_area(WindowControlArea::Drag);
    }
    region
        .on_mouse_down(MouseButton::Left, |event, window, _| {
            if event.click_count == 2 {
                window.zoom_window();
            } else {
                window.start_window_move();
            }
        })
        .on_mouse_down(MouseButton::Right, |event, window, _| {
            window.show_window_menu(event.position);
        })
}

/// On Windows, marks a control so the system handles it, as with its own caption
/// buttons (hover state, Snap Layouts on maximize).
fn system_control(area: WindowControlArea, control: Btn) -> impl IntoElement {
    div()
        .when(DRAWS_TITLE_BAR, |el| el.window_control_area(area))
        .child(control)
}

/// Minimize, maximize and close, for systems that leave decorations to the app.
fn window_controls(window: &Window) -> impl IntoElement {
    let available = window.window_controls();
    let maximized = window.is_maximized();
    let control = |id: &'static str, icon: &'static str, tip: &'static str| {
        icon_btn(id, icon)
            .kind(BtnKind::Ghost)
            .w(WINDOW_CONTROL_W)
            .h(32.)
            .tooltip(tip)
    };
    h_flex()
        .gap(px(2.))
        .when(available.minimize || DRAWS_TITLE_BAR, |row| {
            row.child(system_control(
                WindowControlArea::Min,
                control(
                    "window-minimize",
                    "icons/minus.svg",
                    ui::i18n::t("Minimize"),
                )
                .on_click(|window, _| window.minimize_window()),
            ))
        })
        .when(available.maximize || DRAWS_TITLE_BAR, |row| {
            row.child(system_control(
                WindowControlArea::Max,
                control(
                    "window-maximize",
                    "icons/square.svg",
                    if maximized {
                        ui::i18n::t("Restore")
                    } else {
                        ui::i18n::t("Maximize")
                    },
                )
                .on_click(|window, _| window.zoom_window()),
            ))
        })
        .child(system_control(
            WindowControlArea::Close,
            control("window-close", "icons/x.svg", ui::i18n::t("Close"))
                .on_click(|window, _| window.remove_window()),
        ))
}

/// Invisible strips along the window edges for resizing.
fn resize_handles() -> Vec<gpui::Stateful<gpui::Div>> {
    const EDGE: Pixels = px(5.);
    const CORNER: Pixels = px(10.);
    let handle = |index: u32, edge: ResizeEdge, cursor: CursorStyle| {
        div()
            .id(("window-resize", index))
            .absolute()
            .occlude()
            .cursor(cursor)
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                window.start_window_resize(edge);
                cx.stop_propagation();
            })
    };
    vec![
        handle(0, ResizeEdge::Top, CursorStyle::ResizeUpDown)
            .top_0()
            .left(CORNER)
            .right(CORNER)
            .h(EDGE),
        handle(1, ResizeEdge::Bottom, CursorStyle::ResizeUpDown)
            .bottom_0()
            .left(CORNER)
            .right(CORNER)
            .h(EDGE),
        handle(2, ResizeEdge::Left, CursorStyle::ResizeLeftRight)
            .left_0()
            .top(CORNER)
            .bottom(CORNER)
            .w(EDGE),
        handle(3, ResizeEdge::Right, CursorStyle::ResizeLeftRight)
            .right_0()
            .top(CORNER)
            .bottom(CORNER)
            .w(EDGE),
        handle(4, ResizeEdge::TopLeft, CursorStyle::ResizeUpLeftDownRight)
            .top_0()
            .left_0()
            .size(CORNER),
        handle(5, ResizeEdge::TopRight, CursorStyle::ResizeUpRightDownLeft)
            .top_0()
            .right_0()
            .size(CORNER),
        handle(
            6,
            ResizeEdge::BottomLeft,
            CursorStyle::ResizeUpRightDownLeft,
        )
        .bottom_0()
        .left_0()
        .size(CORNER),
        handle(
            7,
            ResizeEdge::BottomRight,
            CursorStyle::ResizeUpLeftDownRight,
        )
        .bottom_0()
        .right_0()
        .size(CORNER),
    ]
}
