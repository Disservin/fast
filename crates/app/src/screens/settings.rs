//! Settings: General, Board & pieces, Engines, Analysis, Databases, Keyboard shortcuts,
//! Accounts, Data, Licenses, Thanks and Changelog.

use std::{collections::HashMap, path::PathBuf, process::Command};

use chess_core::Notation;
use engine::{
    EngineConfig, EngineEvent, EngineHandle, EngineId, EngineStatus, Limits, OptionKind,
    SearchRequest, UciOption,
};
use gpui::{
    AppContext as _, Context, Div, Entity, FocusHandle, FontWeight, IntoElement, KeyDownEvent,
    Render, SharedString, Subscription, UniformListScrollHandle, Window, div, img, prelude::*, px,
    rgb, svg, uniform_list,
};
use gpui_component::{
    input::{InputEvent, InputState},
    scroll::ScrollableElement,
    slider::{Slider, SliderEvent, SliderState},
};
use ui::{
    MONO, T,
    board::theme_swatch,
    widgets::{badge, btn, check, h_flex, key_cap, mono, panel, section_label, segmented, v_flex},
};

use crate::format::format_count;
use crate::{
    config::{SavedCoordinates, SavedPieces, SavedTheme},
    controls::{TextSelect, engine_icon, new_input, new_select, select_el, text_input},
    model::{AppModel, ModelEvent, ToastExt as _, pick_path, split_engine_name},
    shell,
};

pub use self::about::date_of;
pub(crate) use self::shortcuts::{key_names, shortcut_hint};
use self::{
    about::render_changelog,
    analysis::parse_nodes,
    engine_detail::LeaveAnalyse,
    rows::{detail_row, icon_x, option_row, page_title, section},
};

mod about;
mod analysis;
mod benchmark;
mod board;
mod data;
mod databases;
mod engine_detail;
mod engines;
mod rows;
mod shortcuts;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsPage {
    General,
    Board,
    Engines,
    Analysis,
    Databases,
    Shortcuts,
    Accounts,
    Data,
    Licenses,
    Thanks,
    Changelog,
}

impl SettingsPage {
    pub const ALL: [Self; 11] = [
        Self::General,
        Self::Board,
        Self::Engines,
        Self::Analysis,
        Self::Databases,
        Self::Shortcuts,
        Self::Accounts,
        Self::Data,
        Self::Licenses,
        Self::Thanks,
        Self::Changelog,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Self::General => ui::i18n::t("General"),
            Self::Board => ui::i18n::t("Board & pieces"),
            Self::Engines => ui::i18n::t("Engines"),
            Self::Analysis => ui::i18n::t("Analysis"),
            Self::Databases => ui::i18n::t("Databases"),
            Self::Shortcuts => ui::i18n::t("Keyboard shortcuts"),
            Self::Accounts => ui::i18n::t("Accounts"),
            Self::Data => ui::i18n::t("Data"),
            Self::Licenses => ui::i18n::t("Licenses"),
            Self::Thanks => ui::i18n::t("Thanks"),
            Self::Changelog => ui::i18n::t("Changelog"),
        }
    }
}

pub struct SettingsScreen {
    model: Entity<AppModel>,
    page: SettingsPage,
    selected_engine: Option<EngineId>,
    path_input: Entity<InputState>,
    name_input: Entity<InputState>,
    profile_input: Entity<InputState>,
    license_filter: Entity<InputState>,
    license_scroll: UniformListScrollHandle,
    option_inputs: HashMap<String, Entity<InputState>>,
    option_selects: HashMap<String, TextSelect>,
    language: TextSelect,
    animation: Entity<SliderState>,
    volume: Entity<SliderState>,
    player_name: Entity<InputState>,
    player_aliases: Entity<InputState>,
    /// The window's workspace, for starting account updates on the Database tab.
    workspace: gpui::WeakEntity<crate::shell::Workspace>,
    /// Nodes per position for backwards analysis; applied on Enter or leaving the field.
    backward_nodes: Entity<InputState>,
    /// The node count typed there could not be read.
    backward_error: Option<SharedString>,
    capture_focus: FocusHandle,
    capturing: Option<&'static str>,
    benchmark: Option<SharedString>,
    catalog_open: bool,
    install_status: Option<SharedString>,
    probing_engine: bool,
    install_cancel: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    install_task: Option<gpui::Task<()>>,
    opening_task: Option<gpui::Task<()>>,
    opening_status: Option<SharedString>,
    app_update: Option<crate::app_update::Update>,
    app_update_task: Option<gpui::Task<()>>,
    app_update_status: Option<SharedString>,
    /// Disk use per kind of data, from the last time the Data page was opened.
    usage: Option<crate::storage::Usage>,
    usage_task: Option<gpui::Task<()>>,
    _subscriptions: Vec<Subscription>,
    _option_subscriptions: Vec<Subscription>,
}

impl Drop for SettingsScreen {
    fn drop(&mut self) {
        if let Some(cancel) = &self.install_cancel {
            cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    }
}

impl SettingsScreen {
    pub fn new(
        model: Entity<AppModel>,
        workspace: gpui::WeakEntity<crate::shell::Workspace>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let current_language = model.read(cx).config.language;
        let language = new_select(
            ui::i18n::Language::ALL
                .iter()
                .map(|language| language.name().into())
                .collect(),
            ui::i18n::Language::ALL
                .iter()
                .position(|language| *language == current_language)
                .unwrap_or(0),
            window,
            cx,
        );
        let current_ms = model.read(cx).config.board.animation_ms;
        let current_volume = model.read(cx).config.board.sound_volume;
        let volume = cx.new(|_| {
            SliderState::new()
                .min(0.)
                .max(100.)
                .step(5.)
                .default_value(current_volume as f32)
        });
        let animation = cx.new(|_| {
            SliderState::new()
                .min(0.)
                .max(400.)
                .step(10.)
                .default_value(current_ms as f32)
        });
        let name = model.read(cx).config.player_name.clone();
        let player_name = new_input(name, ui::i18n::t("Your name in game headers"), window, cx);
        let player_aliases = new_input(
            model.read(cx).config.player_aliases.join("; "),
            ui::i18n::t("Separate names with semicolons"),
            window,
            cx,
        );
        let nodes = model.read(cx).config.backward_nodes;
        let backward_nodes = new_input(nodes.to_string(), "e.g. 500000 or 500k", window, cx);
        let path_input = cx.new(|cx| InputState::new(window, cx));
        let name_input = cx.new(|cx| InputState::new(window, cx));
        let profile_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder(ui::i18n::t("Profile name, e.g. BT2"))
        });
        let license_filter = cx.new(|cx| {
            InputState::new(window, cx).placeholder(ui::i18n::t("Filter by name or licence"))
        });
        let subscriptions = vec![
            cx.subscribe(
                &language,
                |this, state, _: &gpui_component::select::SelectEvent<_>, cx| {
                    let Some(index) = state.read(cx).selected_index(cx) else {
                        return;
                    };
                    let Some(language) = ui::i18n::Language::ALL.get(index.row).copied() else {
                        return;
                    };
                    ui::i18n::set_language(language);
                    this.model.update(cx, |model, cx| {
                        model.config.language = language;
                        model.save_config(cx);
                    });
                    crate::shell::refresh_menus(cx);
                    cx.refresh_windows();
                },
            ),
            cx.subscribe(&license_filter, |_, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            }),
            cx.subscribe_in(&name_input, window, |this, state, event, _, cx| {
                if matches!(event, InputEvent::Blur | InputEvent::PressEnter { .. }) {
                    let name = state.read(cx).value().trim().to_owned();
                    this.rename_engine(name, cx);
                }
            }),
            cx.subscribe_in(&profile_input, window, |this, _, event, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.save_profile(window, cx);
                }
            }),
            cx.subscribe(&volume, |this, _, event: &SliderEvent, cx| {
                if let SliderEvent::Change(value) = event {
                    let volume = value.start().round() as u32;
                    if this.model.read(cx).config.board.sound_volume != volume {
                        this.update_board(cx, |board| board.sound_volume = volume);
                        // A sample at the new loudness.
                        this.model.read(cx).config.board.play_sample();
                    }
                }
            }),
            cx.subscribe(&animation, |this, _, event: &SliderEvent, cx| {
                if let SliderEvent::Change(value) = event {
                    let ms = value.start().round() as u32;
                    this.update_board(cx, |board| board.animation_ms = ms);
                }
            }),
            cx.subscribe_in(&player_name, window, |this, state, event, _, cx| {
                if matches!(event, InputEvent::Blur | InputEvent::PressEnter { .. }) {
                    let name = state.read(cx).value().trim().to_owned();
                    this.model.update(cx, |model, cx| {
                        if model.config.player_name != name && !name.is_empty() {
                            model.config.player_name = name;
                            model.save_config(cx);
                            model.refresh_leaks(cx);
                            cx.emit(ModelEvent::Databases);
                        }
                    });
                }
            }),
            cx.subscribe_in(&player_aliases, window, |this, state, event, _, cx| {
                if matches!(event, InputEvent::Blur | InputEvent::PressEnter { .. }) {
                    let value = state.read(cx).value().to_string();
                    this.model.update(cx, |model, cx| {
                        let previous = model.config.player_aliases.clone();
                        model.config.player_aliases.clear();
                        for name in value.split(';') {
                            model.config.add_player_alias(name);
                        }
                        if model.config.player_aliases != previous {
                            model.save_config(cx);
                            model.refresh_leaks(cx);
                            cx.emit(ModelEvent::Databases);
                        }
                    });
                }
            }),
            cx.subscribe_in(&backward_nodes, window, |this, state, event, window, cx| {
                if matches!(event, InputEvent::Blur | InputEvent::PressEnter { .. }) {
                    let text = state.read(cx).value().to_string();
                    match parse_nodes(&text) {
                        Some(nodes) => this.set_backward_nodes(nodes, window, cx),
                        None => {
                            this.backward_error =
                                Some(format!("\"{}\" is not a node count", text.trim()).into());
                            cx.notify();
                        }
                    }
                }
            }),
            cx.observe(&model, |_, _, cx| cx.notify()),
        ];
        let selected_engine = model
            .read(cx)
            .config
            .engines
            .first()
            .map(|engine| engine.id);
        let mut this = Self {
            model,
            page: SettingsPage::General,
            selected_engine: None,
            path_input,
            name_input,
            profile_input,
            license_filter,
            license_scroll: UniformListScrollHandle::new(),
            option_inputs: HashMap::new(),
            option_selects: HashMap::new(),
            language,
            animation,
            volume,
            player_name,
            player_aliases,
            workspace,
            backward_nodes,
            backward_error: None,
            capture_focus: cx.focus_handle(),
            capturing: None,
            benchmark: None,
            catalog_open: false,
            install_status: None,
            probing_engine: false,
            install_cancel: None,
            install_task: None,
            opening_task: None,
            opening_status: None,
            app_update: None,
            app_update_task: None,
            app_update_status: None,
            usage: None,
            usage_task: None,
            _subscriptions: subscriptions,
            _option_subscriptions: Vec::new(),
        };
        this.select_engine(selected_engine, window, cx);
        this
    }

    pub fn page(&self) -> SettingsPage {
        self.page
    }

    pub fn set_page(&mut self, page: SettingsPage, _window: &mut Window, cx: &mut Context<Self>) {
        self.page = page;
        self.capturing = None;
        if page == SettingsPage::Data {
            self.measure_usage(cx);
        }
        cx.notify();
    }

    fn render_nav(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let this = cx.entity();
        let mut nav = v_flex().gap(px(2.));
        for (index, page) in SettingsPage::ALL.into_iter().enumerate() {
            let selected = self.page == page;
            let this = this.clone();
            nav = nav.child(
                h_flex()
                    .id(("settings-nav", index))
                    .relative()
                    .h(px(36.))
                    .px(px(12.))
                    .rounded(px(4.))
                    .cursor_pointer()
                    .overflow_hidden()
                    .text_size(px(14.))
                    .text_color(rgb(if selected { T.fg } else { T.fg_2 }))
                    .when(selected, |item| {
                        item.bg(rgb(T.selected))
                            .border_1()
                            .border_color(rgb(T.border_strong))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(
                                div()
                                    .absolute()
                                    .left_0()
                                    .top_0()
                                    .bottom_0()
                                    .w(px(3.))
                                    .bg(rgb(T.accent)),
                            )
                    })
                    .when(!selected, |item| item.hover(|style| style.bg(rgb(T.hover))))
                    .on_click(move |_, window, cx| {
                        this.update(cx, |screen, cx| screen.set_page(page, window, cx));
                    })
                    .child(page.title()),
            );
        }
        v_flex()
            .w(px(260.))
            .h_full()
            .flex_none()
            .px(px(12.))
            .pt(px(20.))
            .bg(rgb(T.surface))
            .border_r_1()
            .border_color(rgb(T.border))
            .child(
                div()
                    .px(px(12.))
                    .mb(px(14.))
                    .child(section_label(ui::i18n::t("Settings"))),
            )
            .child(nav)
    }
}

impl SettingsScreen {}

impl SettingsScreen {}

impl Render for SettingsScreen {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let nav = self.render_nav(cx);
        let content: gpui::AnyElement = match self.page {
            SettingsPage::General => self.render_general(cx).into_any_element(),
            SettingsPage::Board => self.render_board_page(cx).into_any_element(),
            SettingsPage::Engines => self.render_engines(window, cx).into_any_element(),
            SettingsPage::Analysis => self.render_analysis(cx).into_any_element(),
            SettingsPage::Databases => self.render_databases(cx).into_any_element(),
            SettingsPage::Shortcuts => self.render_shortcuts(cx).into_any_element(),
            SettingsPage::Accounts => self.render_accounts().into_any_element(),
            SettingsPage::Data => self.render_data(cx).into_any_element(),
            SettingsPage::Licenses => self.render_licenses(cx).into_any_element(),
            SettingsPage::Thanks => self.render_thanks().into_any_element(),
            SettingsPage::Changelog => render_changelog().into_any_element(),
        };
        h_flex().items_start().size_full().child(nav).child(
            div()
                .id("settings-content")
                .track_focus(&self.capture_focus)
                .on_key_down(cx.listener(|this, event, _, cx| this.on_capture_key(event, cx)))
                .flex_1()
                .min_w_0()
                .h_full()
                .px(px(28.))
                .pt(px(24.))
                .pb(px(24.))
                .child(div().w_full().child(content))
                .overflow_y_scrollbar(),
        )
    }
}
