//! The window shell: top bar, mode routing, global shortcuts, toasts.

use std::collections::BTreeMap;

use engine::EngineStatus;
use gpui::{
    Action, App, Context, Entity, FocusHandle, FontWeight, IntoElement, KeyBinding, Render,
    SharedString, Subscription, Window, actions, deferred, div, prelude::*, px, rgb,
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
        train::TrainScreen,
    },
};

actions!(
    outpost,
    [
        PrevMove,
        NextMove,
        FirstMove,
        LastMove,
        ToggleEngine,
        FlipBoard,
        FocusSearch,
        SaveGame
    ]
);

/// Key context present unless a text field has focus.
pub const SHORTCUT_CONTEXT: &str = "OutpostShortcuts";
/// Additionally present unless a button has focus, where Space means "activate".
const SPACE_CONTEXT: &str = "OutpostIdle";

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
        _ => return None,
    })
}

/// Binds the configured keystrokes, skipping any that do not parse.
pub fn bind_shortcuts(shortcuts: &BTreeMap<String, String>, cx: &mut App) {
    for (name, keystroke) in shortcuts {
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
    if let Some((_, context)) = action_for(name) {
        load_binding(keystroke, Box::new(gpui::NoAction), context, cx);
    }
}

pub fn init(_cx: &mut App) {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Analyse,
    Database,
    Train,
    Play,
    Settings,
}

pub struct Workspace {
    pub model: Entity<AppModel>,
    pub mode: Mode,
    pub analyse: Entity<AnalyseScreen>,
    pub database: Entity<DatabaseScreen>,
    pub train: Entity<TrainScreen>,
    pub play: Entity<PlayScreen>,
    pub settings: Entity<SettingsScreen>,
    search: Entity<InputState>,
    path_input: Entity<InputState>,
    focus: FocusHandle,
    first_frame: bool,
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
        let analyse = cx.new(|cx| AnalyseScreen::new(model.clone(), window, cx));
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
        let model_sub = cx.observe_in(&model, window, move |this, model, window, cx| {
            // The typed-path bar takes focus as soon as it opens.
            let open = model.read(cx).pending_path.is_some();
            if open && !path_prompt_open {
                this.path_input
                    .update(cx, |input, cx| input.focus(window, cx));
            }
            path_prompt_open = open;
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
            "settings:board" => (Mode::Settings, Some(SettingsPage::Board)),
            "settings:databases" => (Mode::Settings, Some(SettingsPage::Databases)),
            "settings:shortcuts" => (Mode::Settings, Some(SettingsPage::Shortcuts)),
            _ => (Mode::Analyse, None),
        };
        if let Some(page) = settings_page {
            settings.update(cx, |settings, cx| settings.set_page(page, window, cx));
        }
        Self {
            model,
            mode,
            analyse,
            database,
            train,
            play,
            settings,
            search,
            path_input,
            focus,
            first_frame: true,
            _subscriptions: vec![search_sub, path_sub, model_sub],
        }
    }

    pub fn set_mode(&mut self, mode: Mode, cx: &mut Context<Self>) {
        if self.mode == mode {
            return;
        }
        self.mode = mode;
        // The engine only runs while Analyse is open.
        let visible = mode == Mode::Analyse;
        self.analyse
            .update(cx, |analyse, cx| analyse.set_visible(visible, cx));
        cx.notify();
    }

    fn run_search(&mut self, query: &str, window: &mut Window, cx: &mut Context<Self>) {
        if query.is_empty() {
            return;
        }
        // A FEN opens in Analyse; anything else searches players in the database.
        if chess_core::GameTree::from_fen(query).is_ok() {
            let fen = query.to_owned();
            self.analyse
                .update(cx, |analyse, cx| analyse.load_fen(&fen, window, cx));
            self.set_mode(Mode::Analyse, cx);
        } else {
            let text = query.to_owned();
            self.database
                .update(cx, |database, cx| database.search_player(&text, window, cx));
            self.set_mode(Mode::Database, cx);
        }
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
                format!("Settings · {}", self.settings.read(cx).page().title()).into()
            }
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
            names.join(" · ")
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

    fn right_controls(&self, narrow: bool, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let this = cx.entity();
        let mut row = h_flex().gap(px(10.));
        // Narrow windows keep the search box clear of the centered mode switch.
        let search_w = match self.mode {
            Mode::Analyse | Mode::Database if narrow => Some(150.),
            Mode::Analyse | Mode::Database => Some(260.),
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
    }

    fn top_bar(&self, narrow: bool, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let crumb = self.breadcrumb(cx);
        div()
            .relative()
            .h(px(58.))
            .flex_none()
            .w_full()
            .bg(rgb(T.surface))
            .border_b_1()
            .border_color(rgb(T.border))
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
                                    .child("Outpost"),
                            )
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .text_color(rgb(T.muted))
                                    .child(SharedString::from(format!("/ {crumb}"))),
                            ),
                    )
                    .child(self.right_controls(narrow, cx)),
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
        let narrow = window.viewport_size().width < px(1300.);
        let on_button = stack
            .iter()
            .any(|context| context.contains("OutpostButton"));
        let body = match self.mode {
            Mode::Analyse => self.analyse.clone().into_any_element(),
            Mode::Database => self.database.clone().into_any_element(),
            Mode::Train => self.train.clone().into_any_element(),
            Mode::Play => self.play.clone().into_any_element(),
            Mode::Settings => self.settings.clone().into_any_element(),
        };
        v_flex()
            .id("workspace")
            .relative()
            .track_focus(&self.focus)
            // One node carries both contexts: it must be an ancestor of the focused element.
            .when(!typing, |root| {
                root.key_context(
                    if on_button {
                        SHORTCUT_CONTEXT.to_owned()
                    } else {
                        format!("{SHORTCUT_CONTEXT} {SPACE_CONTEXT}")
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
                    .child(self.top_bar(narrow, cx))
                    .children(path_bar)
                    .child(div().flex_1().min_h_0().w_full().child(body)),
            )
            .size_full()
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
            .child(self.toasts(cx))
    }
}
