//! Analyse's engine controls: stats, the Engine and Log panels, and PV highlighting.

use std::ops::Range;

use chess_core::{Chess, Move, Position, format_pv, shakmaty::san::San};
use engine::{EngineConfig, EngineId, LogLine, OptionKind};
use gpui::{
    AnyElement, ClipboardItem, Context, CursorStyle, Div, FontWeight, HighlightStyle, IntoElement,
    KeyDownEvent, ListHorizontalSizingBehavior, MouseButton, MouseMoveEvent, Pixels, Point,
    ScrollStrategy, SharedString, StyledText, Window, anchored, deferred, div, point, prelude::*,
    px, rgb, uniform_list,
};
use gpui_component::{
    input::{Input, InputEvent, InputState},
    scroll::ScrollableElement,
};
use ui::{
    Arrow, CoordinateStyle, MONO, T,
    widgets::{
        Listen, TextExt, btn, check, h_flex, header_tab, icon_btn, mono, panel, section_label,
        v_flex,
    },
};

use super::{AnalyseScreen, MoveRow, RowKey};
use crate::config::AnalysisLimit;
use crate::debug_window::LOG_ROW_H;
use crate::model::ToastExt as _;

pub use self::{log::LogSelection, pv::PvView};

mod log;
mod options;
mod pv;

/// The engine controls or local database results shown above the moves table.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum EngineTab {
    /// The engines with their live figures, and replacing cached results.
    Engines,
    /// The chosen engine's lines, threads and hash, the search limit and the moves
    /// searched.
    Options,
    /// Moves and game results from the selected local database.
    Database,
}

/// How often an open Log panel checks its engine for new lines.
pub(super) const LOG_POLL: std::time::Duration = std::time::Duration::from_millis(200);
/// How far a search is toward the limit, from 0 to 1; an infinite search has no end.
fn limit_progress(stats: &engine::AnalysisUpdate, limit: AnalysisLimit) -> Option<f32> {
    let fraction = match limit {
        AnalysisLimit::Infinite => return None,
        AnalysisLimit::Depth(depth) => stats.depth as f64 / f64::from(depth.max(1)),
        AnalysisLimit::Seconds(seconds) => {
            stats.time_ms? as f64 / (f64::from(seconds.max(1)) * 1000.)
        }
        AnalysisLimit::Nodes(nodes) => stats.nodes? as f64 / nodes.max(1) as f64,
    };
    Some(fraction.clamp(0., 1.) as f32)
}

const LIMITS: [(AnalysisLimit, &str); 6] = [
    (AnalysisLimit::Infinite, "∞"),
    (AnalysisLimit::Depth(20), "d20"),
    (AnalysisLimit::Depth(30), "d30"),
    (AnalysisLimit::Seconds(10), "10s"),
    (AnalysisLimit::Seconds(30), "30s"),
    (AnalysisLimit::Seconds(60), "60s"),
];

/// Plays SAN moves from a position, stopping at the first that does not apply.
pub(super) fn replay(base: &Chess, sans: &[String]) -> (Chess, Vec<Move>) {
    let mut pos = base.clone();
    let mut moves = Vec::with_capacity(sans.len());
    for san in sans {
        let Some(mv) = san
            .parse::<San>()
            .ok()
            .and_then(|parsed| parsed.to_move(&pos).ok())
        else {
            break;
        };
        pos.play_unchecked(mv);
        moves.push(mv);
    }
    (pos, moves)
}

impl AnalyseScreen {
    /// The engine the panels show: the chosen one if still configured, else the first.
    fn panel_engine_config(&self, cx: &gpui::App) -> Option<EngineConfig> {
        let engines = &self.model.read(cx).config.engines;
        self.panel_engine
            .and_then(|id| engines.iter().find(|engine| engine.id == id))
            .or_else(|| engines.iter().find(|engine| engine.use_analysis))
            .or_else(|| engines.first())
            .cloned()
    }

    pub(super) fn panel_engine_id(&self, cx: &gpui::App) -> Option<EngineId> {
        self.panel_engine_config(cx).map(|config| config.id)
    }

    /// One engine: an on/off check, what its search is doing right now, and a thin bar
    /// along the bottom for how far it is toward the search limit.
    fn engine_row(
        &self,
        config: &EngineConfig,
        chosen: bool,
        limit: AnalysisLimit,
        cx: &mut Context<Self>,
    ) -> Div {
        let this = cx.entity();
        let id = config.id;
        let live = self.live.read(cx);
        let running = live.engines.iter().find(|engine| engine.config.id == id);
        let stats = running.and_then(|engine| engine.stats());
        let cached = running
            .and_then(|engine| engine.lines.get(&1))
            .is_some_and(|line| line.cached);
        let stale = running
            .and_then(|engine| engine.lines.get(&1))
            .is_some_and(|line| line.stale);
        let cell = |text: String, width: f32, title: &'static str| {
            v_flex()
                .w(px(width))
                .flex_none()
                .gap(px(1.))
                .child(div().text_as(11., T.muted).child(title))
                .child(
                    mono(text)
                        .whitespace_nowrap()
                        .text_size(px(14.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(rgb(T.fg)),
                )
        };
        let dash = || "—".to_owned();
        let depth = stats.map_or_else(dash, |stats| match stats.seldepth {
            Some(sel) => format!("{}/{sel}", stats.depth),
            None => stats.depth.to_string(),
        });
        let unsupported = self.tree.variant == chess_core::Variant::Chess960
            && !config
                .info
                .options
                .iter()
                .any(|option| option.name == "UCI_Chess960");
        let state = if unsupported {
            Some("No Chess960 support")
        } else if stale {
            Some("stale")
        } else if cached {
            Some("cached")
        } else {
            None
        };
        let nodes = stats
            .and_then(|stats| stats.nodes)
            .map_or_else(dash, engine::format_nodes);
        let speed = stats
            .and_then(|stats| stats.nps)
            .map_or_else(dash, engine::format_speed);
        let time = stats
            .and_then(|stats| stats.time_ms)
            .map_or_else(dash, super::search_time);
        let hash = stats
            .and_then(|stats| stats.hashfull)
            .map_or_else(dash, |full| format!("{:.0}%", f64::from(full) / 10.));
        let tb = stats
            .and_then(|stats| stats.tbhits)
            .filter(|hits| *hits > 0)
            .map(|hits| engine::format_nodes(hits).replace(" n", ""));
        let progress = stats.and_then(|stats| limit_progress(stats, limit));
        let name = if config.version.is_empty() || config.version.len() > 8 {
            config.name.clone()
        } else {
            format!("{} {}", config.name, config.version)
        };
        let row = h_flex()
            .h(px(52.))
            .px(px(16.))
            .gap(px(6.))
            .child(
                div().flex_none().w(px(20.)).child(
                    check(
                        SharedString::from(format!("engine-on-{id}")),
                        config.use_analysis,
                        "",
                    )
                    .on_change(this.listen_with(move |screen, on, _, cx| {
                        screen.set_engine_analysis(id, on, cx)
                    })),
                ),
            )
            .child(crate::controls::engine_icon(config, 20.))
            .child(
                div()
                    .id(SharedString::from(format!("engine-pick-{id}")))
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .cursor_pointer()
                    .font_weight(FontWeight::MEDIUM)
                    .text_size(px(13.))
                    .on_click(this.listen_event(move |screen, _, _, cx| {
                        screen.panel_engine = Some(id);
                        cx.notify();
                    }))
                    .child(name),
            )
            .children(state.map(|state| {
                div()
                    .flex_none()
                    .text_size(px(11.))
                    .text_color(rgb(if unsupported { T.warn } else { T.faint }))
                    .child(ui::i18n::t(state).to_owned())
            }))
            .when(!unsupported, |row| {
                row.child(cell(depth, 68., "DEPTH"))
                    .child(cell(nodes, 78., "NODES"))
                    .child(cell(speed, 90., "SPEED"))
                    .child(cell(time, 64., "TIME"))
                    .child(cell(hash, 50., "HASH"))
                    .children(tb.map(|tb| cell(tb, 60., "TB")))
            });
        v_flex()
            .when(chosen, |el| el.bg(rgb(T.selected)))
            .child(row)
            .child(
                div().h(px(2.)).w_full().bg(rgb(T.divider)).children(
                    progress.map(|fraction| {
                        div().h_full().w(gpui::relative(fraction)).bg(rgb(T.accent))
                    }),
                ),
            )
    }

    pub(super) fn render_engine_panel(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let this = cx.entity();
        let engines = self.model.read(cx).config.engines.clone();
        let limit = self.model.read(cx).config.analysis_limit;
        let chosen = self.panel_engine_config(cx);
        let tab = self.engine_tab;
        let mut body = panel().rounded_none().flex_none().overflow_hidden().child(
            h_flex()
                .h(px(44.))
                .px(px(16.))
                .gap(px(6.))
                .border_b_1()
                .border_color(rgb(T.border))
                .child({
                    let mut tabs = h_flex().h_full().gap(px(16.));
                    for (index, (value, label)) in [
                        (EngineTab::Engines, "Engines"),
                        (EngineTab::Options, "Search options"),
                        (EngineTab::Database, "Database"),
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        tabs = tabs.child(header_tab(
                            ("engine-tab", index),
                            label,
                            tab == value,
                            this.listen(move |screen, _, cx| {
                                screen.engine_tab = value;
                                screen.explorer_menu = None;
                                cx.notify();
                            }),
                        ));
                    }
                    tabs
                })
                .child(div().flex_1())
                // Engine controls keep the search limit in reach.
                .children(
                    (!engines.is_empty() && tab != EngineTab::Database).then(|| {
                        btn("infinite-search", "Infinite")
                            .mini()
                            .selected(limit == AnalysisLimit::Infinite)
                            .tooltip(ui::i18n::t(
                                "Search until stopped, instead of up to the search limit",
                            ))
                            .on_click(this.listen(move |screen, _, cx| {
                                screen.live.update(cx, |live, cx| live.toggle_infinite(cx));
                            }))
                    }),
                ),
        );
        if tab == EngineTab::Database {
            return body
                .child(self.render_database_explorer(cx))
                .into_any_element();
        }
        if engines.is_empty() {
            return body
                .child(div().p(px(16.)).text_as(13., T.muted).child(ui::i18n::t(
                    "No engines yet. Add one in Settings › Engines.",
                )))
                .into_any_element();
        }
        if tab == EngineTab::Engines {
            for config in &engines {
                let is_chosen = chosen.as_ref().is_some_and(|chosen| chosen.id == config.id);
                body = body.child(self.engine_row(config, is_chosen, limit, cx));
            }
            return self.render_engines_tab(body, chosen, cx).into_any_element();
        }
        let mut limits = h_flex().gap(px(4.));
        for (index, (value, label)) in LIMITS.into_iter().enumerate() {
            limits = limits.child(
                btn(("limit", index), label)
                    .mono()
                    .compact()
                    .selected(limit == value)
                    .on_click(this.listen(move |screen, _, cx| {
                        screen.live.update(cx, |live, cx| live.set_limit(value, cx));
                    })),
            );
        }
        // Options apply to the engine chosen on the Engines tab, so its name leads the row.
        if let Some(config) = &chosen {
            body = body.child(self.render_engine_options_row(config, cx));
        }
        body = body.child(
            h_flex()
                .flex_wrap()
                .gap(px(10.))
                .px(px(16.))
                .py(px(10.))
                .border_t_1()
                .border_color(rgb(T.divider))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(90.))
                        .text_as(13., T.fg_2)
                        .child(ui::i18n::t("Search limit")),
                )
                .child(limits),
        );
        body.child(self.render_search_moves(cx)).into_any_element()
    }

    /// Search moves: how many of the legal moves the engine searches, and when opened,
    /// a button per move to restrict the root search to.
    fn render_search_moves(&self, cx: &mut Context<Self>) -> gpui::Div {
        let this = cx.entity();
        // Restricting the root search is rare, so the move list stays folded until asked for.
        let pos = self.analysed_pos();
        let legal = pos.legal_moves();
        let restricted = self.live.read(cx).search_moves.clone();
        let open = self.search_moves_open || !restricted.is_empty();
        // "Search moves" and, beside it in muted text, how many of the legal moves.
        let count = if restricted.is_empty() {
            format!("{} {}", ui::i18n::t("all"), legal.len())
        } else {
            format!(
                "{}/{} {}",
                restricted.len(),
                legal.len(),
                ui::i18n::t("selected")
            )
        };
        let summary = h_flex()
            .gap(px(8.))
            .child(ui::i18n::t("Search moves").to_owned())
            .child(div().text_color(rgb(T.muted)).child(count));
        let mut search = v_flex()
            .px(px(16.))
            .py(px(10.))
            .gap(px(8.))
            .border_t_1()
            .border_color(rgb(T.divider))
            .child(
                h_flex()
                    .gap(px(6.))
                    .child(div().flex_1().text_as(13., T.fg_2).child(summary))
                    .when(!restricted.is_empty(), |row| {
                        row.child(
                            btn("search-all-moves", ui::i18n::t("All moves"))
                                .compact()
                                .on_click(this.listen(move |screen, _, cx| {
                                    screen.search_moves_open = false;
                                    screen
                                        .live
                                        .update(cx, |live, cx| live.clear_search_moves(cx));
                                })),
                        )
                    })
                    .when(restricted.is_empty(), |row| {
                        row.child(
                            btn(
                                "search-moves-toggle",
                                if open { "Hide" } else { "Restrict ›" },
                            )
                            .compact()
                            .kind(ui::widgets::BtnKind::Ghost)
                            .tooltip(ui::i18n::t(
                                "Choose the moves the engine searches from this position",
                            ))
                            .on_click(this.listen(
                                move |screen, _, cx| {
                                    screen.search_moves_open = !screen.search_moves_open;
                                    cx.notify();
                                },
                            )),
                        )
                    }),
            );
        if open {
            search = search.child(
                div()
                    .id("search-move-choices")
                    .max_h(px(110.))
                    .overflow_y_scroll()
                    .child(h_flex().flex_wrap().gap(px(4.)).children(
                        legal.into_iter().enumerate().map(|(index, mv)| {
                            let san =
                                chess_core::shakmaty::san::San::from_move(&pos, mv).to_string();
                            let uci = chess_core::shakmaty::uci::UciMove::from_move(
                                mv,
                                pos.castles().mode(),
                            )
                            .to_string();
                            btn(("search-move", index), san)
                                .h(26.)
                                .pad(7.)
                                .mono()
                                .selected(restricted.contains(&uci))
                                .tooltip(ui::i18n::t(
                                    "Toggle this move in the engine's root search",
                                ))
                                .on_click(this.listen(move |screen, _, cx| {
                                    screen
                                        .live
                                        .update(cx, |live, cx| live.toggle_search_move(mv, cx));
                                }))
                        }),
                    )),
            );
        }
        search
    }

    /// The chosen engine's name with its lines, threads and hash, Restart engine and
    /// All options.
    fn render_engine_options_row(
        &self,
        config: &EngineConfig,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let this = cx.entity();
        let workspace = self.workspace.clone();
        let id = config.id;
        let name = format!("{} {}", config.name, config.version)
            .trim()
            .to_owned();
        h_flex()
            .flex_wrap()
            .gap_x(px(20.))
            .gap_y(px(8.))
            .px(px(16.))
            .py(px(10.))
            .child(
                h_flex()
                    .flex_none()
                    .gap(px(6.))
                    .text_size(px(13.))
                    .font_weight(FontWeight::MEDIUM)
                    .child(crate::controls::engine_icon(config, 14.))
                    .child(name),
            )
            .children(self.render_lines_control(cx))
            .child(self.stepper(config, "Threads", "Threads", "", cx))
            .child(self.stepper(config, "Hash", "Hash", " MB", cx))
            .child(div().flex_1())
            .child({
                btn("restart-engine", "Restart engine")
                    .compact()
                    .tooltip(ui::i18n::t("Quit this engine and start it again"))
                    .on_click(this.listen(move |screen, _, cx| {
                        screen.live.update(cx, |live, cx| live.relaunch(id, cx));
                    }))
            })
            .child(
                btn("all-options", "All options")
                    .compact()
                    .tooltip(ui::i18n::t(
                        "Every option this engine offers, in Settings › Engines",
                    ))
                    .on_click(move |window, cx| {
                        if let Some(workspace) = workspace.upgrade() {
                            workspace.update(cx, |workspace, cx| {
                                workspace.open_engine_settings(id, window, cx);
                            });
                        }
                    }),
            )
    }

    /// The Engine panel's first tab under its engine rows: replacing a cached result
    /// with a fresh search, when the chosen engine's lines came from the cache.
    fn render_engines_tab(
        &mut self,
        body: Div,
        chosen: Option<EngineConfig>,
        cx: &mut Context<Self>,
    ) -> Div {
        let this = cx.entity();
        let Some(config) = chosen else {
            return body;
        };
        let id = config.id;
        let has_cached = self
            .live
            .read(cx)
            .engines
            .iter()
            .find(|engine| engine.config.id == id)
            .is_some_and(|engine| engine.lines.values().any(|line| line.cached));
        body.when(has_cached, |body| {
            body.child(
                h_flex()
                    .justify_end()
                    .px(px(16.))
                    .py(px(10.))
                    .border_t_1()
                    .border_color(rgb(T.divider))
                    .child(
                        btn("replace-cached-analysis", "Replace cached analysis")
                            .compact()
                            .tooltip(ui::i18n::t(
                                "Discard this saved result and run a fresh search",
                            ))
                            .on_click(this.listen(move |screen, _, cx| {
                                screen
                                    .live
                                    .update(cx, |live, cx| live.replace_cached(id, cx));
                            })),
                    ),
            )
        })
    }
}
