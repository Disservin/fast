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
    widgets::{btn, check, h_flex, header_tab, icon_btn, mono, panel, section_label, v_flex},
};

use super::{AnalyseScreen, MoveRow, RowKey};
use crate::config::AnalysisLimit;
use crate::debug_window::LOG_ROW_H;
use crate::model::ToastExt as _;

use self::pv::move_squares;
pub use self::{log::LogSelection, pv::PvView};

mod log;
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

/// An engine's value, minimum and maximum for a spin option it declares.
pub fn spin_option(config: &EngineConfig, name: &str) -> Option<(i64, i64, i64)> {
    let option = config
        .info
        .options
        .iter()
        .find(|option| option.name == name)?;
    let OptionKind::Spin { default, min, max } = option.kind else {
        return None;
    };
    let value = config
        .options
        .get(name)
        .and_then(|value| value.parse().ok())
        .unwrap_or(default);
    Some((value, min, max))
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

/// What the Lines buttons ask for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinesTarget {
    Step(bool),
    /// The engine's own default.
    Default,
    /// As many lines as the engine allows, up to [`MAX_LINES`]: every move in every
    /// position, without changing as the game moves on.
    All,
}

/// More lines than any position has legal moves (218 at most).
pub const MAX_LINES: i64 = 256;

/// The MultiPV value for a Lines button, within the engine's range.
pub fn lines_target(target: LinesTarget, current: i64, default: i64, min: i64, max: i64) -> i64 {
    match target {
        LinesTarget::Step(up) => step_value("MultiPV", current, up, min, max),
        LinesTarget::Default => default.clamp(min, max),
        LinesTarget::All => MAX_LINES.clamp(min, max),
    }
}

/// The next stepper value: Hash doubles and halves, everything else moves by one.
pub fn step_value(name: &str, value: i64, up: bool, min: i64, max: i64) -> i64 {
    let next = match (name, up) {
        ("Hash", true) => value.saturating_mul(2),
        ("Hash", false) => value / 2,
        (_, true) => value.saturating_add(1),
        (_, false) => value.saturating_sub(1),
    };
    next.clamp(min, max)
}

impl AnalyseScreen {
    /// The moves Line preview lists: the previewed line, or else the selected row's.
    pub(super) fn preview_line(&self, selected: Option<&MoveRow>) -> Vec<String> {
        match (&self.pv_view, selected) {
            (Some(view), _) => view.sans.clone(),
            (None, Some(row)) if !row.pv.is_empty() => row.pv.clone(),
            (None, Some(row)) => vec![row.san.clone()],
            (None, None) => Vec::new(),
        }
    }

    /// The arrow for move `index` of the line Line preview lists now, if that line
    /// still reaches it.
    pub(super) fn hovered_arrow(&self, index: usize) -> Option<Arrow> {
        let sans = self.preview_line(self.selected_row());
        let base = &self.tree.node(self.current).pos;
        let (_, moves) = replay(base, &sans[..sans.len().min(index + 1)]);
        let (from, to) = moves.get(index).and_then(move_squares)?;
        Some(Arrow::new(from, to))
    }

    /// The position on the board, including a previewed line.
    pub(super) fn shown_pos(&self) -> Chess {
        let moves = self
            .pv_view
            .as_ref()
            .map_or(&[][..], |view| &view.sans[..view.played()]);
        replay(&self.tree.node(self.current).pos, moves).0
    }

    /// The position on the board, which the engine and the table are also following.
    pub(super) fn analysed_pos(&self) -> Chess {
        replay(&self.tree.node(self.current).pos, self.preview_moves()).0
    }

    pub(super) fn analysed_ply(&self) -> u32 {
        self.tree.node(self.current).ply + self.preview_moves().len() as u32
    }

    pub(super) fn analysed_san_path(&self) -> Vec<String> {
        let mut path = self.base_san_path();
        path.extend_from_slice(self.preview_moves());
        path
    }

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

    /// Lines each engine searches (MultiPV), for every analysis engine at once:
    /// one fewer, one more, back to each engine's default, or as many as allowed.
    fn render_lines_control(&self, cx: &mut Context<Self>) -> Option<Div> {
        let this = cx.entity();
        let small = |id: &'static str, label: &'static str| btn(id, label).compact();
        let lines = self.shown_lines(cx);
        lines.map(|value| {
            let (down_this, up_this) = (this.clone(), this.clone());
            let (reset_this, max_this) = (this.clone(), this.clone());
            h_flex()
                .gap(px(2.))
                .mr(px(4.))
                .child(
                    div()
                        .mr(px(2.))
                        .text_size(px(13.))
                        .text_color(rgb(T.muted))
                        .child(ui::i18n::t("Lines")),
                )
                .child(
                    ui::widgets::stepper("lines", value.to_string())
                        .value_width(24.)
                        .tooltips(
                            "Search one fewer engine line",
                            "MultiPV: lines each engine searches, saved in its options",
                        )
                        .on_down(value > 1, move |_, cx| {
                            down_this.update(cx, |screen, cx| screen.step_lines(false, cx));
                        })
                        .on_up(true, move |_, cx| {
                            up_this.update(cx, |screen, cx| screen.step_lines(true, cx));
                        }),
                )
                .child(
                    small("lines-reset", ui::i18n::t("Reset"))
                        .tooltip(ui::i18n::t("Back to each engine's default number of lines"))
                        .on_click(move |_, cx| {
                            reset_this.update(cx, |screen, cx| {
                                screen.set_lines(LinesTarget::Default, cx)
                            });
                        }),
                )
                .child(
                    small("lines-max", "Max")
                        .tooltip(ui::i18n::t("A line for every legal move in every position (up to 256, as far as the engine allows)"))
                        .on_click(move |_, cx| {
                            max_this
                                .update(cx, |screen, cx| screen.set_lines(LinesTarget::All, cx));
                        }),
                )
        })
    }

    /// The Moves header's controls: the view settings on the left, and the pause
    /// button and panel toggles on the right.
    pub(super) fn render_table_tools(&self, cx: &mut Context<Self>) -> (AnyElement, AnyElement) {
        let this = cx.entity();
        let live = self.live.read(cx);
        let (on, paused) = (live.on, live.paused);
        let view = self.model.read(cx).config.view.clone();
        let all_this = this.clone();
        let arrows_this = this.clone();
        let mine_this = this.clone();
        let small = |id: &'static str, label: &'static str| btn(id, label).compact();
        let view_tools = h_flex()
            .flex_wrap()
            .gap(px(4.))
            .mr(px(6.))
            .child(
                small("all-moves", ui::i18n::t("All moves"))
                    .selected(view.all_moves)
                    .tooltip(ui::i18n::t("List every legal move, analysed or not"))
                    .on_click(move |_, cx| {
                        all_this.update(cx, |screen, cx| {
                            screen.set_view(cx, |view| view.all_moves = !view.all_moves);
                        });
                    }),
            )
            .child(
                small("quality-arrows", "Arrows")
                    .selected(view.quality_arrows)
                    .tooltip(ui::i18n::t(
                        "An arrow per engine line, coloured from best (blue) to blunder (red)",
                    ))
                    .on_click(move |_, cx| {
                        arrows_this.update(cx, |screen, cx| {
                            screen.set_view(cx, |view| view.quality_arrows = !view.quality_arrows);
                        });
                    }),
            )
            .child(
                small("my-moves", "My moves")
                    .selected(view.my_moves)
                    .tooltip(ui::i18n::t(
                        "Show what you played and your repertoire for each move",
                    ))
                    .on_click(move |_, cx| {
                        mine_this.update(cx, |screen, cx| {
                            screen.set_view(cx, |view| view.my_moves = !view.my_moves);
                        });
                    }),
            );
        let pause_this = this.clone();
        let log_this = this.clone();
        let online_this = this;
        let settings = h_flex()
            .flex_wrap()
            .gap(px(8.))
            .child(view_tools)
            .into_any_element();
        let toggles = h_flex()
            .flex_wrap()
            .gap(px(6.))
            .child(
                icon_btn(
                    "engine-pause",
                    if on && !paused {
                        "icons/pause.svg"
                    } else {
                        "icons/play.svg"
                    },
                )
                .w(28.)
                .compact()
                .selected(on && paused)
                .tooltip(if !on {
                    "Start analysis"
                } else if paused {
                    "Resume search (P)"
                } else {
                    "Pause search and keep the current lines (P)"
                })
                .on_click(move |_, cx| {
                    // With the engines off, the same button starts them again.
                    pause_this.update(cx, |screen, cx| {
                        if on {
                            screen.toggle_pause(cx);
                        } else {
                            screen.toggle_engine(cx);
                        }
                    });
                }),
            )
            .child(
                btn("log-panel", "Log")
                    .compact()
                    .selected(self.log_panel_open)
                    .tooltip(ui::i18n::t("Show or hide engine output"))
                    .on_click(move |_, cx| {
                        log_this.update(cx, |screen, cx| {
                            screen.log_panel_open = !screen.log_panel_open;
                            cx.notify();
                        });
                    }),
            )
            .child(
                btn("online-panel", "Online")
                    .compact()
                    .selected(self.online.open)
                    .tooltip(
                        "Add the ChessTB Explorer tablebase and chessdb.cn to Moves from here. \
                         Sends each position you visit while on.",
                    )
                    .on_click(move |_, cx| {
                        online_this.update(cx, |screen, cx| screen.toggle_online(cx));
                    }),
            )
            .into_any_element();
        (toggles, settings)
    }

    /// Lines shown by the analysis engines: the first one's MultiPV.
    pub(super) fn shown_lines(&self, cx: &gpui::App) -> Option<i64> {
        self.live
            .read(cx)
            .engines
            .iter()
            .find_map(|engine| spin_option(&engine.config, "MultiPV"))
            .map(|(value, ..)| value)
    }

    /// Steps MultiPV on every analysis engine that has it, saved in each engine's
    /// options, without restarting them.
    pub(super) fn step_lines(&mut self, up: bool, cx: &mut Context<Self>) {
        self.set_lines(LinesTarget::Step(up), cx);
    }

    /// Sets MultiPV on every analysis engine that has it, each within its own range,
    /// saved in its options and applied without restarting it.
    pub(super) fn set_lines(&mut self, target: LinesTarget, cx: &mut Context<Self>) {
        let Some(current) = self.shown_lines(cx) else {
            return;
        };
        let changes: Vec<(EngineId, i64)> = self
            .live
            .read(cx)
            .engines
            .iter()
            .filter_map(|engine| {
                let (value, min, max) = spin_option(&engine.config, "MultiPV")?;
                let default = match engine
                    .config
                    .info
                    .options
                    .iter()
                    .find(|option| option.name == "MultiPV")
                    .map(|option| &option.kind)
                {
                    Some(OptionKind::Spin { default, .. }) => *default,
                    _ => 1,
                };
                let next = lines_target(target, current, default, min, max);
                (next != value).then_some((engine.config.id, next))
            })
            .collect();
        for (id, value) in changes {
            self.live.update(cx, |live, cx| {
                live.set_option(id, "MultiPV", value.to_string(), cx);
            });
        }
        self.rebuild_rows(cx);
    }

    fn set_spin(&mut self, id: EngineId, name: &str, value: i64, cx: &mut Context<Self>) {
        let name = name.to_owned();
        self.live.update(cx, |live, cx| {
            live.set_option(id, &name, value.to_string(), cx);
        });
        self.rebuild_rows(cx);
    }

    /// Turns an engine on or off for live analysis.
    fn set_engine_analysis(&mut self, id: EngineId, on: bool, cx: &mut Context<Self>) {
        self.model.update(cx, |model, cx| {
            model.set_engine_use(id, |config| config.use_analysis = on, cx);
        });
    }

    fn stepper(
        &self,
        config: &EngineConfig,
        name: &'static str,
        label: &'static str,
        unit: &'static str,
        cx: &mut Context<Self>,
    ) -> Div {
        let id = config.id;
        let spin = spin_option(config, name);
        let this = cx.entity();
        let step = |up: bool| {
            let this = this.clone();
            let enabled = spin
                .is_some_and(|(value, min, max)| step_value(name, value, up, min, max) != value);
            let handler = move |_: &mut gpui::Window, cx: &mut gpui::App| {
                let Some((value, min, max)) = spin else {
                    return;
                };
                let next = step_value(name, value, up, min, max);
                this.update(cx, |screen, cx| screen.set_spin(id, name, next, cx));
            };
            (enabled, handler)
        };
        let ((down_on, down), (up_on, up)) = (step(false), step(true));
        let edit_this = this.clone();
        let value_element = if let Some((edit_id, edit_name, input, _)) = &self.spin_edit
            && *edit_id == id
            && *edit_name == name
        {
            Input::new(input).w_full().into_any_element()
        } else {
            let text = spin.map_or_else(
                || "—".to_owned(),
                |(value, ..)| {
                    if name == "Hash" {
                        engine::format_hash(value)
                    } else {
                        format!("{value}{unit}")
                    }
                },
            );
            btn(SharedString::from(format!("{name}-value")), text)
                .w(72.)
                .h(24.)
                .pad(0.)
                .disabled(spin.is_none())
                .on_click(move |window, cx| {
                    let Some((value, min, max)) = spin else {
                        return;
                    };
                    edit_this.update(cx, |screen, cx| {
                        let input = cx.new(|cx| {
                            let mut state = InputState::new(window, cx);
                            state.set_value(value.to_string(), window, cx);
                            state
                        });
                        let subscription =
                            cx.subscribe_in(&input, window, move |screen, input, event, _, cx| {
                                if matches!(event, InputEvent::Blur | InputEvent::PressEnter { .. })
                                {
                                    if let Ok(value) = input.read(cx).value().trim().parse::<i64>()
                                    {
                                        screen.set_spin(id, name, value.clamp(min, max), cx);
                                    }
                                    screen.spin_edit = None;
                                    cx.notify();
                                }
                            });
                        use gpui::Focusable;
                        input.focus_handle(cx).focus(window, cx);
                        screen.spin_edit = Some((id, name, input, subscription));
                        cx.notify();
                    });
                })
                .into_any_element()
        };
        h_flex()
            .flex_none()
            .gap(px(4.))
            .child(
                div()
                    .mr(px(4.))
                    .text_size(px(12.))
                    .text_color(rgb(T.muted))
                    .child(ui::i18n::t(label).to_owned()),
            )
            .child(
                ui::widgets::stepper(SharedString::from(format!("{name}-{id:?}")), "")
                    .mini()
                    .value_width(72.)
                    .value_element(value_element)
                    .on_down(down_on, down)
                    .on_up(up_on, up),
            )
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
        let select_this = this.clone();
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
                .child(
                    div()
                        .text_size(px(11.))
                        .text_color(rgb(T.muted))
                        .child(title),
                )
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
                    .on_change(move |on, _, cx| {
                        this.update(cx, |screen, cx| screen.set_engine_analysis(id, on, cx));
                    }),
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
                    .on_click(move |_, _, cx| {
                        select_this.update(cx, |screen, cx| {
                            screen.panel_engine = Some(id);
                            cx.notify();
                        });
                    })
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
                        let this = this.clone();
                        tabs = tabs.child(header_tab(
                            ("engine-tab", index),
                            label,
                            tab == value,
                            move |_, cx| {
                                this.update(cx, |screen, cx| {
                                    screen.engine_tab = value;
                                    screen.explorer_menu = None;
                                    cx.notify();
                                })
                            },
                        ));
                    }
                    tabs
                })
                .child(div().flex_1())
                // Engine controls keep the search limit in reach.
                .children(
                    (!engines.is_empty() && tab != EngineTab::Database).then(|| {
                        let this = this.clone();
                        btn("infinite-search", "Infinite")
                            .mini()
                            .selected(limit == AnalysisLimit::Infinite)
                            .tooltip(ui::i18n::t(
                                "Search until stopped, instead of up to the search limit",
                            ))
                            .on_click(move |_, cx| {
                                this.update(cx, |screen, cx| {
                                    screen.live.update(cx, |live, cx| live.toggle_infinite(cx));
                                });
                            })
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
                .child(
                    div()
                        .p(px(16.))
                        .text_size(px(13.))
                        .text_color(rgb(T.muted))
                        .child(ui::i18n::t(
                            "No engines yet. Add one in Settings › Engines.",
                        )),
                )
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
            let this = this.clone();
            limits = limits.child(
                btn(("limit", index), label)
                    .mono()
                    .compact()
                    .selected(limit == value)
                    .on_click(move |_, cx| {
                        this.update(cx, |screen, cx| {
                            screen.live.update(cx, |live, cx| live.set_limit(value, cx));
                        });
                    }),
            );
        }
        let workspace = self.workspace.clone();
        // Options apply to the engine chosen on the Engines tab, so its name leads the row.
        if let Some(config) = &chosen {
            let id = config.id;
            let name = format!("{} {}", config.name, config.version)
                .trim()
                .to_owned();
            body = body.child(
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
                        let this = this.clone();
                        btn("restart-engine", "Restart engine")
                            .compact()
                            .tooltip(ui::i18n::t("Quit this engine and start it again"))
                            .on_click(move |_, cx| {
                                this.update(cx, |screen, cx| {
                                    screen.live.update(cx, |live, cx| live.relaunch(id, cx));
                                });
                            })
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
                    ),
            );
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
                        .text_size(px(13.))
                        .text_color(rgb(T.fg_2))
                        .child(ui::i18n::t("Search limit")),
                )
                .child(limits),
        );
        // Restricting the root search is rare, so the move list stays folded until asked for.
        let pos = self.analysed_pos();
        let legal = pos.legal_moves();
        let restricted = self.live.read(cx).search_moves.clone();
        let open = self.search_moves_open || !restricted.is_empty();
        let reset = this.clone();
        let toggle = this.clone();
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
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(13.))
                            .text_color(rgb(T.fg_2))
                            .child(summary),
                    )
                    .when(!restricted.is_empty(), |row| {
                        row.child(
                            btn("search-all-moves", ui::i18n::t("All moves"))
                                .compact()
                                .on_click(move |_, cx| {
                                    reset.update(cx, |screen, cx| {
                                        screen.search_moves_open = false;
                                        screen
                                            .live
                                            .update(cx, |live, cx| live.clear_search_moves(cx));
                                    })
                                }),
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
                            .on_click(move |_, cx| {
                                toggle.update(cx, |screen, cx| {
                                    screen.search_moves_open = !screen.search_moves_open;
                                    cx.notify();
                                })
                            }),
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
                            let this = this.clone();
                            btn(("search-move", index), san)
                                .h(26.)
                                .pad(7.)
                                .mono()
                                .selected(restricted.contains(&uci))
                                .tooltip(ui::i18n::t(
                                    "Toggle this move in the engine's root search",
                                ))
                                .on_click(move |_, cx| {
                                    this.update(cx, |screen, cx| {
                                        screen
                                            .live
                                            .update(cx, |live, cx| live.toggle_search_move(mv, cx));
                                    })
                                })
                        }),
                    )),
            );
        }
        body.child(search).into_any_element()
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
                            .on_click(move |_, cx| {
                                this.update(cx, |screen, cx| {
                                    screen
                                        .live
                                        .update(cx, |live, cx| live.replace_cached(id, cx));
                                });
                            }),
                    ),
            )
        })
    }

    pub(super) fn current_log_key(&self, cx: &gpui::App) -> Option<(EngineId, u64, bool)> {
        self.panel_log(cx)
            .map(|(id, log)| (id, log.revision(), self.log_show_info))
    }

    /// Picking a table row during a preview continues the line with that row. Returns
    /// false when there is no preview to continue.
    pub(super) fn continue_pv_with(&mut self, key: &RowKey, cx: &mut Context<Self>) -> bool {
        let Some(played) = self.pv_view.as_ref().map(PvView::played) else {
            return false;
        };
        // The table shows the game position's moves, not the preview's.
        if played == 0 || self.pv_kibitz {
            self.exit_pv_preview(cx);
            return false;
        }
        let continuation = self
            .rows
            .iter()
            .find(|row| key.matches(row))
            .map(|row| {
                if row.pv.is_empty() {
                    vec![row.san.clone()]
                } else {
                    row.pv.clone()
                }
            })
            .unwrap_or_default();
        if let Some(view) = &mut self.pv_view {
            view.sans.truncate(played);
            view.sans.extend(continuation);
            view.anchor = played - 1;
            view.head = played - 1;
        }
        self.row_chosen = true;
        self.selected = Some(key.clone());
        self.update_arrows(cx);
        cx.notify();
        true
    }

    /// Ends any highlight and returns the board, engine and table to the game position.
    pub(super) fn exit_pv_preview(&mut self, cx: &mut Context<Self>) {
        if self.pv_view.take().is_none() {
            return;
        }
        self.pv_dragging = false;
        self.pv_played = None;
        self.pv_hover = None;
        let kibitz = std::mem::take(&mut self.pv_kibitz);
        if !kibitz {
            self.selected = None;
            self.row_chosen = false;
        }
        let node = self.tree.node(self.current);
        let pos = node.pos.clone();
        let last = node.mv.as_ref().and_then(move_squares);
        self.board.update(cx, |board, cx| {
            board.set_position(pos, last, false, cx);
            board.set_arrows(Vec::new(), cx);
            board.set_interactive(true, cx);
        });
        if kibitz {
            // The main analysis never left the game position, but the table held its
            // selection for the preview: let it follow the best line again, and draw
            // the arrows for what the table shows now.
            self.kibitz.update(cx, |kibitz, cx| kibitz.stop(cx));
            self.rebuild_rows(cx);
        } else {
            let (tree, current) = (self.tree.clone(), self.current);
            self.live
                .update(cx, |live, cx| live.set_position(&tree, current, cx));
            self.query_databases(cx);
        }
        cx.notify();
    }

    /// Copies the highlighted moves, or the whole line if none are highlighted.
    pub(super) fn copy_pv(&mut self, cx: &mut Context<Self>) {
        let ply = self.tree.node(self.current).ply;
        let text = match &self.pv_view {
            Some(view) => {
                let (start, end) = view.range();
                format_pv(ply + start as u32, &view.sans[start..=end])
            }
            None => match self.selected_row() {
                Some(row) if !row.pv.is_empty() => format_pv(ply, &row.pv),
                Some(row) => format_pv(ply, std::slice::from_ref(&row.san)),
                None => return,
            },
        };
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.model.toast("Line copied.", cx);
    }

    /// The kibitz engine's view of the previewed position, apart from the main analysis:
    /// its name, score, depth and best line.
    pub(super) fn render_kibitz_row(&self, cx: &gpui::App) -> Option<AnyElement> {
        let view = self.pv_view.as_ref().filter(|_| self.pv_kibitz)?;
        let kibitz = self.kibitz.read(cx);
        let line = kibitz.line.as_ref();
        let ply = self.tree.node(self.current).ply + view.played() as u32;
        let score = line
            .and_then(|line| line.score)
            .map_or_else(|| "—".to_owned(), engine::Score::display);
        let depth = line.map_or_else(|| "—".to_owned(), |line| line.update.depth.to_string());
        let best = line
            .filter(|line| !line.sans.is_empty())
            .map(|line| format_pv(ply, &line.sans))
            .unwrap_or_else(|| {
                if kibitz.running {
                    ui::i18n::t("Searching…").to_owned()
                } else {
                    "—".to_owned()
                }
            });
        let figure = |label: &'static str, value: String| {
            v_flex()
                .flex_none()
                .child(
                    div()
                        .text_size(px(10.))
                        .text_color(rgb(T.faint))
                        .child(ui::i18n::t(label).to_uppercase()),
                )
                .child(
                    mono(value)
                        .whitespace_nowrap()
                        .text_size(px(12.))
                        .text_color(rgb(T.fg_2)),
                )
        };
        Some(
            h_flex()
                .mt(px(10.))
                .gap(px(14.))
                .items_center()
                .child(
                    v_flex().flex_none().child(section_label("Kibitz")).child(
                        div()
                            .text_size(px(12.))
                            .text_color(rgb(T.muted))
                            .whitespace_nowrap()
                            .child(kibitz.name.clone()),
                    ),
                )
                .child(figure("Eval", score))
                .child(figure("Depth", depth))
                .child(
                    mono(best)
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(px(12.))
                        .text_color(rgb(T.fg_2)),
                )
                .into_any_element(),
        )
    }

    /// Heading text while the board shows something other than the game position.
    pub(super) fn pv_preview_label(&self, ply: u32) -> Option<String> {
        let view = self.pv_view.as_ref()?;
        let (start, end) = view.range();
        let line = format_pv(ply + start as u32, &view.sans[start..=end]);
        Some(format!("previewing {line}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine::{EngineInfo, UciOption};

    #[test]
    fn replay_stops_at_a_move_that_does_not_apply() {
        let line = ["e4", "e5", "e4", "Nf6"].map(String::from);
        let (_, moves) = replay(&Chess::default(), &line);
        assert_eq!(moves.len(), 2);
    }

    fn config() -> EngineConfig {
        EngineConfig {
            info: EngineInfo {
                options: vec![UciOption {
                    name: "Hash".to_owned(),
                    kind: OptionKind::Spin {
                        default: 16,
                        min: 1,
                        max: 64,
                    },
                }],
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn spin_values_come_from_config_then_the_engine_default() {
        let mut config = config();
        assert_eq!(spin_option(&config, "Hash"), Some((16, 1, 64)));
        config.options.insert("Hash".to_owned(), "32".to_owned());
        assert_eq!(spin_option(&config, "Hash"), Some((32, 1, 64)));
        assert_eq!(spin_option(&config, "Threads"), None);
    }

    #[test]
    fn lines_buttons_stay_in_the_engine_range() {
        assert_eq!(lines_target(LinesTarget::Step(true), 3, 1, 1, 500), 4);
        assert_eq!(lines_target(LinesTarget::Default, 7, 1, 1, 500), 1);
        // Max stays at the cap, whatever the position, so every move keeps a line.
        assert_eq!(lines_target(LinesTarget::All, 1, 1, 1, 500), MAX_LINES);
        // An engine allowing fewer lines gets its own maximum.
        assert_eq!(lines_target(LinesTarget::All, 1, 1, 1, 8), 8);
        assert_eq!(lines_target(LinesTarget::Default, 4, 0, 1, 8), 1);
    }

    #[test]
    fn steppers_clamp_and_hash_doubles() {
        assert_eq!(step_value("Hash", 32, true, 1, 64), 64);
        assert_eq!(step_value("Hash", 64, true, 1, 64), 64);
        assert_eq!(step_value("Hash", 1, false, 1, 64), 1);
        assert_eq!(step_value("MultiPV", 3, true, 1, 500), 4);
        assert_eq!(step_value("MultiPV", 1, false, 1, 500), 1);
    }

    #[test]
    fn highlight_ranges_work_in_either_direction() {
        let view = PvView {
            sans: Vec::new(),
            anchor: 5,
            head: 2,
        };
        assert_eq!(view.range(), (2, 5));
        assert!(view.contains(3));
        assert!(!view.contains(6));
        assert!(view.is_range());
        assert_eq!(view.played(), 2);
        let single = PvView {
            sans: Vec::new(),
            anchor: 3,
            head: 3,
        };
        assert!(!single.is_range());
        assert_eq!(single.played(), 4);
    }
}
