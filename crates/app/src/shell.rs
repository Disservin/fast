//! The window shell: top bar, mode routing, global shortcuts, toasts.

use crate::model::{ConfigExt as _, ToastExt};
use std::collections::{BTreeMap, HashMap};

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
    widgets::{Btn, BtnKind, Listen, TextExt, btn, h_flex, icon_btn, v_flex},
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

#[cfg(test)]
pub use self::actions::action_for;
pub use self::actions::{bind_shortcuts, init, refresh_menus, unbind_shortcut};
use self::{
    actions::draws_own_menus,
    chrome::{drag_region, resize_handles, window_controls},
};

mod actions;
mod chrome;
mod session;
mod tab_bar;
mod tabs;
mod top_bar;

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
        NewChess960,
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

/// A random number that differs on every call, for picks such as a Chess960 position.
pub(crate) fn random_u64() -> u64 {
    use std::hash::{BuildHasher as _, Hasher as _};
    // The clock alone is coarse on some systems (100 ns steps on Windows), so its low
    // digits repeat. Each `RandomState` has fresh keys, which SipHash spreads evenly.
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    hasher.write_u128(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |time| time.as_nanos()),
    );
    hasher.finish()
}

/// A Chess960 start position number picked at random.
pub(crate) fn random_chess960_number() -> u16 {
    (random_u64() % 960) as u16
}

/// Windows hides its title bar and the top bar takes its place, window controls
/// included. Elsewhere the system draws one when it can.
pub const DRAWS_TITLE_BAR: bool = cfg!(target_os = "windows");

/// On macOS the title bar is transparent and the top bar extends under it, so the
/// traffic lights sit at its left end instead of on a separate line above it.
pub const MAC_TITLE_BAR: bool = cfg!(target_os = "macos");

/// Where macOS draws the traffic lights: centred in the 58px top bar.
pub const TRAFFIC_LIGHTS_AT: (f32, f32) = (18., 22.);

/// Room the traffic lights take at the top bar's left end, including a gap.
const TRAFFIC_LIGHTS_W: f32 = 72.;

const WINDOW_CONTROL_W: f32 = 36.;
const WINDOW_CONTROLS_W: f32 = WINDOW_CONTROL_W * 3. + 4.;
/// Half the width of the centered mode switch.
const MODE_SWITCH_HALF_W: f32 = 165.;

/// Key context present unless a text field has focus.
pub const SHORTCUT_CONTEXT: &str = "FastShortcuts";
/// Additionally present unless a button has focus, where Space means "activate".
const SPACE_CONTEXT: &str = "FastIdle";
const TAB_CONTEXT: &str = "FastTabs";

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

/// Engine work shown on a tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TabActivity {
    /// The tab's analysis is paused and keeps its lines.
    Paused,
    /// Engines are searching for the tab while another one is shown.
    Searching,
}

impl TabActivity {
    fn mark(self) -> impl IntoElement {
        let tip = match self {
            Self::Paused => "Analysis paused",
            Self::Searching => "Engines are searching in this tab",
        };
        div()
            .id(tip)
            .flex_none()
            .map(|mark| match self {
                Self::Paused => mark.child(ui::widgets::icon("icons/pause.svg", px(12.), T.muted)),
                Self::Searching => mark.size(px(7.)).rounded_full().bg(rgb(T.accent)),
            })
            .tooltip(move |window, cx| {
                gpui_component::tooltip::Tooltip::new(ui::i18n::t(tip)).build(window, cx)
            })
    }
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
    /// Engine work the tab holds, shown as a mark beside its title.
    fn activity(&self, cx: &App) -> Option<TabActivity> {
        match self {
            Self::Analyse(screen) if screen.read(cx).live_paused(cx) => Some(TabActivity::Paused),
            Self::Analyse(screen) => screen
                .read(cx)
                .live_searching(cx)
                .then_some(TabActivity::Searching),
            Self::Play(screen) => screen
                .read(cx)
                .engine_thinking()
                .then_some(TabActivity::Searching),
            Self::Database(screen) => screen
                .read(cx)
                .is_reviewing()
                .then_some(TabActivity::Searching),
            Self::Tournament(screen) => screen
                .read(cx)
                .is_running()
                .then_some(TabActivity::Searching),
            Self::TestPositions(screen) => screen
                .read(cx)
                .is_running()
                .then_some(TabActivity::Searching),
            Self::Train(_) | Self::Settings(_) => None,
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

pub struct Workspace {
    /// Each tab's last activity mark, and the observer that redraws the strip when it
    /// changes, so a tab working in the background updates its mark.
    tab_watch: HashMap<u64, (Option<TabActivity>, Subscription)>,
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
    /// Keeps new recovery snapshots separate when visible tab numbering restarts.
    next_recovery_id: u64,
    last_tab: BTreeMap<Mode, u64>,
    /// Tabs visited before the active one, most recent last, for Back.
    back_tabs: Vec<u64>,
    /// Tabs left with Back, most recent last, for Forward.
    forward_tabs: Vec<u64>,
    new_tab_menu_open: bool,
    /// The tab being renamed and the field holding its new name.
    renaming: Option<crate::controls::Rename<u64>>,
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
            next_recovery_id: 2,
            last_tab: [(mode, 1)].into(),
            back_tabs: Vec::new(),
            forward_tabs: Vec::new(),
            new_tab_menu_open: false,
            renaming: None,
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
            tab_watch: HashMap::new(),
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
        let has_chess960 = workspace.tabs.iter().any(|tab| {
            matches!(&tab.content, TabContent::Analyse(screen)
                if screen.read(cx).tree.variant == chess_core::Variant::Chess960)
        });
        if workspace.model.read(cx).demo && !has_chess960 {
            let active = workspace.active_tab;
            workspace.new_tab(Mode::Analyse, window, cx);
            workspace.analyse.update(cx, |screen, cx| {
                screen.load_tree(chess_core::GameTree::chess960(734), window, cx)
            });
            workspace.activate_tab(active, cx);
        }
        workspace
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
            let mut tab = self.closed_tabs.remove(index);
            if tab.id == 0 {
                tab.id = self.next_tab_id;
                self.next_tab_id += 1;
            }
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
}

impl Workspace {}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.watch_tabs(cx);
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
            .capture_key_down(cx.listener(move |this, event: &gpui::KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" && this.model.read(cx).pending_path.is_some() {
                    this.model.update(cx, |model, cx| {
                        model.pending_path = None;
                        cx.notify();
                    });
                    cx.stop_propagation();
                }
            }))
            .on_key_down(cx.listener(move |this, event: &gpui::KeyDownEvent, _, cx| {
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
                if this.mode == Mode::Play {
                    this.play.update(cx, |screen, cx| screen.step(-1, cx));
                } else {
                    this.analyse
                        .update(cx, |screen, cx| screen.step(-1, window, cx));
                }
            }))
            .on_action(cx.listener(|this, _: &NextMove, window, cx| {
                if this.mode == Mode::Play {
                    this.play.update(cx, |screen, cx| screen.step(1, cx));
                } else {
                    this.analyse
                        .update(cx, |screen, cx| screen.step(1, window, cx));
                }
            }))
            .on_action(cx.listener(|this, _: &FirstMove, window, cx| {
                if this.mode == Mode::Play {
                    this.play.update(cx, |screen, cx| screen.go_first(cx));
                } else {
                    this.analyse
                        .update(cx, |screen, cx| screen.go_first(window, cx));
                }
            }))
            .on_action(cx.listener(|this, _: &LastMove, window, cx| {
                if this.mode == Mode::Play {
                    this.play.update(cx, |screen, cx| screen.go_last(cx));
                } else {
                    this.analyse
                        .update(cx, |screen, cx| screen.go_last(window, cx));
                }
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
            .on_action(cx.listener(|this, _: &NewChess960, window, cx| {
                this.new_chess960(random_chess960_number(), window, cx);
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
