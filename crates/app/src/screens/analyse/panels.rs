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
    widgets::{Listen, StyleExt, btn, check, h_flex, mono, panel, section_label, v_flex},
};

use super::{AnalyseScreen, MoveRow, RowKey};
use crate::config::AnalysisLimit;
use crate::debug_window::LOG_ROW_H;
use crate::model::ToastExt as _;

pub use self::{log::LogSelection, pv::PvView};

pub(super) mod limit;
mod log;
mod options;
mod pv;

/// How often an open Log panel checks its engine for new lines.
pub(super) const LOG_POLL: std::time::Duration = std::time::Duration::from_millis(200);
/// How far a search is toward the limit, from 0 to 1; an infinite search has no end.
pub(super) fn limit_progress(stats: &engine::AnalysisUpdate, limit: AnalysisLimit) -> Option<f32> {
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
    pub(super) fn panel_engine_config(&self, cx: &gpui::App) -> Option<EngineConfig> {
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

    /// Search moves: how many of the legal moves the engine searches, and when opened,
    /// a button per move to restrict the root search to.
    pub(super) fn render_search_moves(&self, cx: &mut Context<Self>) -> gpui::Div {
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
            .border_t_as(T.divider)
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
    pub(super) fn render_engine_options_row(
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
}
