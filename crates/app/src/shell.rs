//! The window shell: top bar, mode routing, global shortcuts, toasts.

use std::collections::BTreeMap;

use engine::EngineStatus;
use gpui::{
    Action, App, Context, CursorStyle, Decorations, Entity, FocusHandle, FontWeight, IntoElement,
    KeyBinding, MouseButton, Pixels, Render, ResizeEdge, SharedString, Subscription, Window,
    WindowControlArea, actions, deferred, div, prelude::*, px, rgb,
};
use gpui_component::input::{InputEvent, InputState};
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
        Quit
    ]
);

/// The application menus: native on macOS, drawn in the top bar elsewhere.
fn app_menus() -> Vec<gpui::Menu> {
    use gpui::{Menu, MenuItem};
    vec![
        Menu::new("File").items([
            MenuItem::action("New game", NewGame),
            MenuItem::action("New tab", NewTab),
            MenuItem::action("Close tab", CloseTab),
            MenuItem::action("Reopen closed tab", ReopenTab),
            MenuItem::action("Open PGN or FEN…", OpenGame),
            MenuItem::separator(),
            MenuItem::action("Save to My games", SaveGame),
            MenuItem::action("Save as PGN…", SavePgnAs),
            MenuItem::separator(),
            MenuItem::action("Settings", ShowSettings),
            MenuItem::separator(),
            MenuItem::action("Quit", Quit),
        ]),
        Menu::new("Edit").items([
            MenuItem::action("Copy FEN", CopyFen),
            MenuItem::action("Copy PGN", CopyPgn),
            MenuItem::action("Paste FEN or PGN", PasteGame),
        ]),
        Menu::new("View").items([
            MenuItem::action("Analyse", ShowAnalyse),
            MenuItem::action("Database", ShowDatabase),
            MenuItem::action("Train", ShowTrain),
            MenuItem::action("Play", ShowPlay),
            MenuItem::action("Next tab", NextTab),
            MenuItem::action("Previous tab", PrevTab),
            MenuItem::separator(),
            MenuItem::action("Flip board", FlipBoard),
        ]),
        Menu::new("Go").items([
            MenuItem::action("Start of game", FirstMove),
            MenuItem::action("Previous move", PrevMove),
            MenuItem::action("Next move", NextMove),
            MenuItem::action("End of line", LastMove),
        ]),
        Menu::new("Engine").items([
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
        "show_settings" => (Box::new(ShowSettings), SHORTCUT_CONTEXT),
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
}

#[derive(Clone)]
enum TabContent {
    Analyse(Entity<AnalyseScreen>),
    Database(Entity<DatabaseScreen>),
    Train(Entity<TrainScreen>),
    Play(Entity<PlayScreen>),
    Settings(Entity<SettingsScreen>),
    Tournament(Entity<TournamentScreen>),
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
        }
    }
    /// The tab's screen and, where it has one, what that screen is showing.
    fn title(&self, cx: &App) -> (&'static str, Option<SharedString>) {
        match self {
            Self::Analyse(screen) => ("Analyse", Some(screen.read(cx).breadcrumb())),
            Self::Database(screen) => ("Database", Some(screen.read(cx).breadcrumb())),
            Self::Train(_) => ("Train", None),
            Self::Settings(_) => ("Settings", None),
            Self::Play(screen) => ("Play", Some(screen.read(cx).breadcrumb())),
            Self::Tournament(screen) => ("Tournament", screen.read(cx).tab_detail()),
        }
    }
}
#[derive(Clone)]
struct WorkspaceTab {
    id: u64,
    content: TabContent,
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
    tabs: Vec<WorkspaceTab>,
    closed_tabs: Vec<WorkspaceTab>,
    active_tab: u64,
    next_tab_id: u64,
    last_tab: BTreeMap<Mode, u64>,
    new_tab_menu_open: bool,
    search: Entity<InputState>,
    path_input: Entity<InputState>,
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
        let settings = cx.new(|cx| SettingsScreen::new(model.clone(), window, cx));
        let search = cx
            .new(|cx| InputState::new(window, cx).placeholder("Search positions, games, openings"));
        let path_input = cx.new(|cx| InputState::new(window, cx).placeholder("/path/to/file"));
        let path_sub = cx.subscribe_in(&path_input, window, |this, _, event, window, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                this.submit_path(window, cx);
            }
        });
        let search_sub = cx.subscribe_in(&search, window, |this, state, event, window, cx| {
            if let InputEvent::PressEnter { .. } = event {
                let query = state.read(cx).value().to_string();
                this.run_search(query.trim(), window, cx);
            }
        });
        let mut path_prompt_open = false;
        let mut in_background = model.read(cx).config.analyse_in_background;
        let model_sub = cx.observe_in(&model, window, move |this, model, window, cx| {
            // The typed-path bar takes focus as soon as it opens.
            let open = model.read(cx).pending_path.is_some();
            if open && !path_prompt_open {
                this.path_input
                    .update(cx, |input, cx| input.focus(window, cx));
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
            "settings:licenses" => (Mode::Settings, Some(SettingsPage::Licenses)),
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
            Mode::Settings | Mode::Tournament => TabContent::Settings(settings.clone()),
        };
        let mut workspace = Self {
            tabs: vec![WorkspaceTab {
                id: 1,
                content: initial_content,
            }],
            closed_tabs: Vec::new(),
            active_tab: 1,
            next_tab_id: 2,
            last_tab: [(mode, 1)].into(),
            new_tab_menu_open: false,
            model,
            mode,
            analyse,
            database,
            train,
            play,
            settings,
            tournament: None,
            search,
            path_input,
            menu_bar: draws_own_menus().then(|| gpui_component::menu::AppMenuBar::new(cx)),
            focus,
            first_frame: true,
            force_window_controls: false,
            _subscriptions: vec![search_sub, path_sub, model_sub],
        };
        workspace.restore_recovery_tabs(window, cx);
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
            self.tabs.push(WorkspaceTab { id, content });
            self.activate_tab(id, cx);
        }
    }

    fn activate_tab(&mut self, id: u64, cx: &mut Context<Self>) {
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
        }
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
            _ => self
                .existing_content(mode)
                .expect("only tournament tabs are created on demand"),
        };
        WorkspaceTab { id, content }
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

    /// Covers the content area when the + button is open: one card per kind of tab.
    fn new_tab_picker(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        const CHOICES: [(Mode, &str, &str); 4] = [
            (
                Mode::Analyse,
                "Analyse",
                "A board, engine analysis and the move list for a new position or game.",
            ),
            (
                Mode::Play,
                "Play",
                "Play a game against an engine, with clocks.",
            ),
            (
                Mode::Database,
                "Database",
                "Browse, search and import your game collections.",
            ),
            (
                Mode::Tournament,
                "Tournament",
                "Engine-vs-engine matches run with fastchess.",
            ),
        ];
        // Two by two, in one column with the header so their edges line up.
        let mut cards = div().grid().grid_cols(2).gap(px(16.)).w_full();
        for (index, (mode, title, description)) in CHOICES.into_iter().enumerate() {
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
                        div()
                            .text_size(px(19.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(T.fg))
                            .child(title),
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
                                    .child("New tab"),
                            )
                            .child(
                                btn("new-tab-picker-close", "×")
                                    .h(28.)
                                    .w(28.)
                                    .pad(4.)
                                    .tooltip("Close")
                                    .on_click(move |_, cx| {
                                        close.update(cx, |this, cx| {
                                            this.new_tab_menu_open = false;
                                            cx.notify();
                                        })
                                    }),
                            ),
                    )
                    .child(cards),
            )
    }

    fn tab_bar(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let mut tabs = h_flex().gap(px(4.));
        for tab in &self.tabs {
            let id = tab.id;
            let select = cx.entity();
            let close = select.clone();
            tabs = tabs.child(
                h_flex()
                    .id(("tab", id as usize))
                    .on_drag(
                        DraggedTab {
                            id,
                            title: tab.content.title(cx).0.into(),
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
                            cx.notify();
                        }
                    }))
                    .flex_none()
                    .gap(px(2.))
                    .h(px(30.))
                    .pl(px(9.))
                    .pr(px(3.))
                    .rounded(px(6.))
                    .bg(rgb(if id == self.active_tab {
                        T.selected
                    } else {
                        T.surface
                    }))
                    .border_1()
                    .border_color(rgb(if id == self.active_tab {
                        T.border_strong
                    } else {
                        T.border
                    }))
                    .child(
                        div()
                            .id(("tab-select", id as usize))
                            .cursor_pointer()
                            .text_size(px(12.))
                            .on_click(move |_, _, cx| {
                                select.update(cx, |this, cx| this.activate_tab(id, cx))
                            })
                            .child({
                                let (screen, detail) = tab.content.title(cx);
                                h_flex()
                                    .gap(px(6.))
                                    .child(
                                        ui::widgets::mono(id.to_string()).text_color(rgb(T.faint)),
                                    )
                                    .child(div().font_weight(FontWeight::SEMIBOLD).child(screen))
                                    .children(
                                        detail.map(|detail| {
                                            div().text_color(rgb(T.muted)).child(detail)
                                        }),
                                    )
                            }),
                    )
                    .child(
                        btn(("close-tab", id as usize), "×")
                            .h(24.)
                            .w(24.)
                            .pad(3.)
                            .tooltip("Close tab; reopen with Ctrl/Cmd+Shift+T")
                            .on_click(move |window, cx| {
                                close.update(cx, |this, cx| this.close_tab(id, window, cx))
                            }),
                    ),
            );
        }
        let add = cx.entity();
        let reopen = cx.entity();
        h_flex()
            .flex_none()
            .h(px(38.))
            .px(px(10.))
            .gap(px(8.))
            .bg(rgb(T.surface))
            .border_b_1()
            .border_color(rgb(T.border))
            .child(
                div()
                    .id("workspace-tabs")
                    .flex_1()
                    .min_w_0()
                    .overflow_x_scroll()
                    .child(tabs),
            )
            .child(
                h_flex().gap(px(4.)).child(
                    btn("new-tab", "+")
                        .h(26.)
                        .w(28.)
                        .pad(4.)
                        .tooltip("Add a tab")
                        .on_click(move |_, cx| {
                            add.update(cx, |this, cx| {
                                this.new_tab_menu_open = !this.new_tab_menu_open;
                                cx.notify();
                            })
                        }),
                ),
            )
            .child(
                btn("reopen-tab", "Reopen")
                    .h(26.)
                    .pad(7.)
                    .text(12.)
                    .disabled(self.closed_tabs.is_empty())
                    .on_click(move |_, cx| reopen.update(cx, |this, cx| this.reopen_tab(cx))),
            )
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

    fn run_search(&mut self, query: &str, window: &mut Window, cx: &mut Context<Self>) {
        if query.is_empty() {
            return;
        }
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
        let text = self.path_input.read(cx).value().trim().to_owned();
        if text.is_empty() {
            return;
        }
        let pending = self.model.update(cx, |model, cx| {
            cx.notify();
            model.pending_path.take()
        });
        self.path_input
            .update(cx, |input, cx| input.set_value("", window, cx));
        if let Some(pending) = pending {
            (pending.on_path)(std::path::PathBuf::from(text), cx);
        }
    }

    /// Typed-path fallback, for desktops with no file dialog service (such as WSL).
    fn path_bar(&self, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
        let title = self.model.read(cx).pending_path.as_ref()?.title.clone();
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
                    "{title}: no file dialog is available here, enter a path"
                )))
                .child(
                    div()
                        .flex_1()
                        .child(crate::controls::text_input(&self.path_input)),
                )
                .child(
                    btn("path-ok", "Open")
                        .primary()
                        .on_click(move |window, cx| {
                            this.update(cx, |this, cx| this.submit_path(window, cx));
                        }),
                )
                .child(btn("path-cancel", "Cancel").on_click(move |_, cx| {
                    cancel.update(cx, |model, cx| {
                        model.pending_path = None;
                        cx.notify();
                    });
                })),
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
                || "Tournament".into(),
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
            Mode::Database => 72. + 10.,
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
            row = row.child(
                div()
                    .w(px(width))
                    .child(crate::controls::text_input(&self.search).height(34.)),
            );
        }
        match self.mode {
            Mode::Analyse => {
                if let Some(button) = self.engine_button(cx) {
                    row = row.child(button);
                }
            }
            Mode::Database => {
                let db = self.database.clone();
                row = row.child(btn("import", "Import").on_click(move |window, cx| {
                    db.update(cx, |database, cx| database.prompt_import(window, cx));
                }));
            }
            Mode::Train => {
                let settings_this = this.clone();
                row = row.child(
                    btn("repertoires", "Repertoires").on_click(move |window, cx| {
                        settings_this.update(cx, |this, cx| {
                            this.train
                                .update(cx, |train, cx| train.prompt_open(window, cx));
                        });
                    }),
                );
            }
            _ => {}
        }
        let settings_active = self.mode == Mode::Settings;
        row.child(
            icon_btn("settings", "icons/settings.svg")
                .w(48.)
                .selected(settings_active)
                .tooltip("Settings")
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
                                    .child("Fast"),
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
                            .child(self.mode_button(Mode::Analyse, "Analyse", cx))
                            .child(self.mode_button(Mode::Database, "Database", cx))
                            .child(self.mode_button(Mode::Train, "Train", cx))
                            .child(self.mode_button(Mode::Play, "Play", cx)),
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
            column = column.child(
                div()
                    .max_w(px(420.))
                    .px(px(14.))
                    .py(px(10.))
                    .rounded(px(6.))
                    .bg(rgb(T.selected))
                    .border_1()
                    .border_color(rgb(T.border_strong))
                    .text_size(px(14.))
                    .text_color(rgb(T.fg))
                    .child(toast.text.clone()),
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
            ["Input", "Select", "Popover", "Menu", "List", "Dialog"]
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
            .on_key_down(cx.listener(move |this, event: &gpui::KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape"
                    && this.mode == Mode::Tournament
                    && let Some(screen) = this.tournament.clone()
                    && screen.update(cx, |screen, cx| screen.escape(cx))
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
            .on_action(cx.listener(|this, _: &OpenGame, window, cx| {
                this.new_tab(Mode::Analyse, window, cx);
                this.analyse.update(cx, |screen, cx| screen.open_file(cx));
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
                control("window-minimize", "icons/minus.svg", "Minimize")
                    .on_click(|window, _| window.minimize_window()),
            ))
        })
        .when(available.maximize || DRAWS_TITLE_BAR, |row| {
            row.child(system_control(
                WindowControlArea::Max,
                control(
                    "window-maximize",
                    "icons/square.svg",
                    if maximized { "Restore" } else { "Maximize" },
                )
                .on_click(|window, _| window.zoom_window()),
            ))
        })
        .child(system_control(
            WindowControlArea::Close,
            control("window-close", "icons/x.svg", "Close")
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
