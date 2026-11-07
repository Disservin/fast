//! Analyse's engine controls: stats, the Engine and Log panels, and PV highlighting.

use std::ops::Range;

use chess_core::{Chess, Move, Position, format_pv, shakmaty::san::San};
use engine::{EngineConfig, EngineId, LogKind, LogLine, OptionKind};
use gpui::{
    AnyElement, ClipboardItem, Context, CursorStyle, Div, FontWeight, HighlightStyle, IntoElement,
    KeyDownEvent, ListHorizontalSizingBehavior, MouseButton, MouseMoveEvent, Pixels, Point,
    ScrollStrategy, SharedString, StyledText, Window, anchored, deferred, div, point, prelude::*,
    px, rgb, uniform_list,
};
use gpui_component::scroll::ScrollableElement;
use ui::{
    Arrow, CoordinateStyle, MONO, T,
    widgets::{btn, check, h_flex, icon_btn, mono, panel, section_label, v_flex},
};

use super::{AnalyseScreen, MoveRow};
use crate::config::AnalysisLimit;

const LOG_ROW_H: f32 = 19.;
/// Selected log text, which has to read clearly under every line colour.
const LOG_SELECTION_BG: u32 = 0x34507A;
/// How often an open Log panel checks its engine for new lines.
pub(super) const LOG_POLL: std::time::Duration = std::time::Duration::from_millis(200);
/// A hovered PV move has to stand out against the panel surface.
const HOVER_BG: u32 = 0x3F4753;
/// Side of the board that follows the cursor over a hovered PV move.
const PEEK_SIZE: f32 = 200.;
/// How far the peek board sits below and right of the cursor, so it never covers the move.
const PEEK_OFFSET: f32 = 14.;
const LIMITS: [(AnalysisLimit, &str); 5] = [
    (AnalysisLimit::Infinite, "∞"),
    (AnalysisLimit::Depth(20), "d20"),
    (AnalysisLimit::Depth(30), "d30"),
    (AnalysisLimit::Seconds(10), "10s"),
    (AnalysisLimit::Seconds(60), "60s"),
];

/// Text selected in the Log panel, as (line, byte) positions in `log_lines`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LogSelection {
    pub anchor: (usize, usize),
    pub head: (usize, usize),
}

impl LogSelection {
    fn at(position: (usize, usize)) -> Self {
        Self {
            anchor: position,
            head: position,
        }
    }

    fn is_empty(&self) -> bool {
        self.anchor == self.head
    }

    fn ordered(&self) -> ((usize, usize), (usize, usize)) {
        (self.anchor.min(self.head), self.anchor.max(self.head))
    }

    /// The selected bytes of one line, if any of it is selected.
    fn range_in(&self, line: usize, text: &str) -> Option<Range<usize>> {
        let (start, end) = self.ordered();
        if line < start.0 || line > end.0 {
            return None;
        }
        let from = if line == start.0 { start.1 } else { 0 };
        let to = if line == end.0 { end.1 } else { text.len() };
        let range = floor_char(text, from)..floor_char(text, to);
        (range.start < range.end).then_some(range)
    }

    /// The selected text, one log line per text line.
    pub fn text(&self, lines: &[LogLine]) -> String {
        let (start, end) = self.ordered();
        (start.0..=end.0.min(lines.len().saturating_sub(1)))
            .filter_map(|index| {
                let text = &lines.get(index)?.text;
                let range = self.range_in(index, text).unwrap_or(0..0);
                Some(&text[range])
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// The nearest char boundary at or before `index`, clamped to the text.
fn floor_char(text: &str, index: usize) -> usize {
    let mut index = index.min(text.len());
    while !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

/// A highlighted run of moves in the selected line. The line is copied so engine
/// updates cannot move it while it is being read.
pub struct PvView {
    pub sans: Vec<String>,
    pub anchor: usize,
    pub head: usize,
}

impl PvView {
    /// Inclusive first and last highlighted move.
    pub fn range(&self) -> (usize, usize) {
        (self.anchor.min(self.head), self.anchor.max(self.head))
    }

    pub fn is_range(&self) -> bool {
        self.anchor != self.head
    }

    /// How many of the line's moves the board has played: through a single highlighted
    /// move, or up to the start of a range, whose moves are drawn as arrows instead.
    pub fn played(&self) -> usize {
        let (start, end) = self.range();
        if start == end { end + 1 } else { start }
    }

    fn contains(&self, index: usize) -> bool {
        let (start, end) = self.range();
        (start..=end).contains(&index)
    }
}

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
fn replay(base: &Chess, sans: &[String]) -> (Chess, Vec<Move>) {
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
    /// The moves being looked at beyond the game position.
    fn preview_moves(&self) -> &[String] {
        self.pv_view
            .as_ref()
            .map_or(&[], |view| &view.sans[..view.played()])
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

    /// Depth, nodes and speed, with the buttons that open the panels.
    pub(super) fn render_table_tools(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let this = cx.entity();
        let live = self.live.read(cx);
        let (on, paused) = (live.on, live.paused);
        let view = self.model.read(cx).config.view.clone();
        let sort_this = this.clone();
        let all_this = this.clone();
        let arrows_this = this.clone();
        let small = |id: &'static str, label: &'static str| btn(id, label).h(26.).pad(8.).text(12.);
        let lines = self.shown_lines(cx);
        let lines_stepper = lines.map(|value| {
            let (down_this, up_this) = (this.clone(), this.clone());
            let (reset_this, max_this) = (this.clone(), this.clone());
            h_flex()
                .gap(px(2.))
                .mr(px(4.))
                .child(
                    div()
                        .mr(px(2.))
                        .text_size(px(12.))
                        .text_color(rgb(T.muted))
                        .child("Lines"),
                )
                .child(
                    small("lines-down", "−")
                        .w(24.)
                        .pad(0.)
                        .disabled(value <= 1)
                        .on_click(move |_, cx| {
                            down_this.update(cx, |screen, cx| screen.step_lines(false, cx));
                        }),
                )
                .child(
                    mono(value.to_string())
                        .min_w(px(22.))
                        .flex()
                        .justify_center()
                        .text_size(px(13.)),
                )
                .child(
                    small("lines-up", "+")
                        .w(24.)
                        .pad(0.)
                        .tooltip("MultiPV: lines each engine searches, saved in its options")
                        .on_click(move |_, cx| {
                            up_this.update(cx, |screen, cx| screen.step_lines(true, cx));
                        }),
                )
                .child(
                    small("lines-reset", "Reset")
                        .w(48.)
                        .pad(0.)
                        .tooltip("Back to each engine's default number of lines")
                        .on_click(move |_, cx| {
                            reset_this.update(cx, |screen, cx| {
                                screen.set_lines(LinesTarget::Default, cx)
                            });
                        }),
                )
                .child(
                    small("lines-max", "Max")
                        .w(40.)
                        .pad(0.)
                        .tooltip("A line for every legal move in every position (up to 256, as far as the engine allows)")
                        .on_click(move |_, cx| {
                            max_this
                                .update(cx, |screen, cx| screen.set_lines(LinesTarget::All, cx));
                        }),
                )
        });
        let view_tools = h_flex()
            .flex_wrap()
            .gap(px(4.))
            .mr(px(6.))
            .children(lines_stepper)
            .child(
                small(
                    "sort-lines",
                    match view.sort {
                        crate::config::LineSort::Eval => "By eval",
                        crate::config::LineSort::Depth => "By depth",
                    },
                )
                .w(72.)
                .tooltip("Order engine lines by evaluation or by search depth")
                .on_click(move |_, cx| {
                    sort_this.update(cx, |screen, cx| {
                        screen.set_view(cx, |view| {
                            view.sort = match view.sort {
                                crate::config::LineSort::Eval => crate::config::LineSort::Depth,
                                crate::config::LineSort::Depth => crate::config::LineSort::Eval,
                            };
                        });
                    });
                }),
            )
            .child(
                small("all-moves", "All moves")
                    .selected(view.all_moves)
                    .tooltip("List every legal move, analysed or not")
                    .on_click(move |_, cx| {
                        all_this.update(cx, |screen, cx| {
                            screen.set_view(cx, |view| view.all_moves = !view.all_moves);
                        });
                    }),
            )
            .child(
                small("quality-arrows", "Arrows")
                    .selected(view.quality_arrows)
                    .tooltip("An arrow per engine line, coloured from best (blue) to blunder (red)")
                    .on_click(move |_, cx| {
                        arrows_this.update(cx, |screen, cx| {
                            screen.set_view(cx, |view| view.quality_arrows = !view.quality_arrows);
                        });
                    }),
            );
        let pause_this = this.clone();
        let engine_this = this.clone();
        let log_this = this.clone();
        let online_this = this;
        h_flex()
            .flex_wrap()
            .justify_end()
            .gap(px(6.))
            .child(view_tools)
            .when(on, |row| {
                row.child(
                    icon_btn(
                        "engine-pause",
                        if paused {
                            "icons/play.svg"
                        } else {
                            "icons/pause.svg"
                        },
                    )
                    .w(28.)
                    .h(26.)
                    .pad(8.)
                    .text(12.)
                    .selected(paused)
                    .tooltip(if paused {
                        "Resume search (P)"
                    } else {
                        "Pause search and keep the current lines (P)"
                    })
                    .on_click(move |_, cx| {
                        pause_this.update(cx, |screen, cx| screen.toggle_pause(cx));
                    }),
                )
            })
            .child(
                btn("engine-panel", "Engine")
                    .h(26.)
                    .pad(8.)
                    .text(12.)
                    .selected(self.engine_panel_open)
                    .on_click(move |_, cx| {
                        engine_this.update(cx, |screen, cx| {
                            screen.engine_panel_open = !screen.engine_panel_open;
                            cx.notify();
                        });
                    }),
            )
            .child(
                btn("log-panel", "Log")
                    .h(26.)
                    .pad(8.)
                    .text(12.)
                    .selected(self.log_panel_open)
                    .on_click(move |_, cx| {
                        log_this.update(cx, |screen, cx| {
                            screen.log_panel_open = !screen.log_panel_open;
                            cx.notify();
                        });
                    }),
            )
            .child(
                btn("online-panel", "Online")
                    .h(26.)
                    .pad(8.)
                    .text(12.)
                    .selected(self.online.open)
                    .tooltip(
                        "Add the ChessTB Explorer tablebase and chessdb.cn to Moves from here. \
                         Sends each position you visit while on.",
                    )
                    .on_click(move |_, cx| {
                        online_this.update(cx, |screen, cx| screen.toggle_online(cx));
                    }),
            )
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
            let Some(config) = model.engine_mut(id) else {
                return;
            };
            config.use_analysis = on;
            config.enabled = config.use_analysis || config.use_review || config.use_play;
            let status = if config.enabled {
                engine::EngineStatus::Ready
            } else {
                engine::EngineStatus::Disabled
            };
            model.engine_status.insert(id, status);
            model.save_config(cx);
            cx.emit(crate::model::ModelEvent::Engines);
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
        let button =
            |up: bool, this: gpui::Entity<Self>| {
                let (id_text, text) = if up { ("up", "+") } else { ("down", "−") };
                btn(SharedString::from(format!("{name}-{id_text}")), text)
                    .w(26.)
                    .h(26.)
                    .pad(0.)
                    .disabled(spin.is_none_or(|(value, min, max)| {
                        step_value(name, value, up, min, max) == value
                    }))
                    .on_click(move |_, cx| {
                        let Some((value, min, max)) = spin else {
                            return;
                        };
                        let next = step_value(name, value, up, min, max);
                        this.update(cx, |screen, cx| screen.set_spin(id, name, next, cx));
                    })
            };
        let text = spin.map_or_else(|| "—".to_owned(), |(value, ..)| format!("{value}{unit}"));
        v_flex()
            .gap(px(4.))
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(rgb(T.muted))
                    .child(label),
            )
            .child(
                h_flex()
                    .gap(px(4.))
                    .child(button(false, this.clone()))
                    .child(
                        mono(text)
                            .min_w(px(58.))
                            .flex()
                            .justify_center()
                            .text_size(px(13.)),
                    )
                    .child(button(true, this)),
            )
    }

    /// One engine: an on/off check and what its search is doing right now.
    fn engine_row(&self, config: &EngineConfig, chosen: bool, cx: &mut Context<Self>) -> Div {
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
                .child(
                    div()
                        .text_size(px(10.))
                        .text_color(rgb(T.faint))
                        .child(title),
                )
                .child(mono(text).text_size(px(12.)).text_color(rgb(T.fg_2)))
        };
        let dash = || "—".to_owned();
        let depth = stats.map_or_else(dash, |stats| {
            let depth = match stats.seldepth {
                Some(sel) => format!("{}/{sel}", stats.depth),
                None => stats.depth.to_string(),
            };
            if stale {
                format!("stale {depth}")
            } else if cached {
                format!("cached {depth}")
            } else {
                depth
            }
        });
        let nodes = stats
            .and_then(|stats| stats.nodes)
            .map_or_else(dash, engine::format_nodes);
        let speed = stats
            .and_then(|stats| stats.nps)
            .map_or_else(dash, engine::format_speed);
        let hash = stats
            .and_then(|stats| stats.hashfull)
            .map_or_else(dash, |full| format!("{:.0}%", f64::from(full) / 10.));
        let tb = stats
            .and_then(|stats| stats.tbhits)
            .map_or_else(dash, |hits| hits.to_string());
        let name = if config.version.is_empty() || config.version.len() > 8 {
            config.name.clone()
        } else {
            format!("{} {}", config.name, config.version)
        };
        h_flex()
            .h(px(42.))
            .px(px(16.))
            .gap(px(6.))
            .border_b_1()
            .border_color(rgb(T.divider))
            .when(chosen, |row| row.bg(rgb(T.selected)))
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
            .child(cell(depth, 44., "DEPTH"))
            .child(cell(nodes, 62., "NODES"))
            .child(cell(speed, 72., "SPEED"))
            .child(cell(hash, 36., "HASH"))
            .child(cell(tb, 40., "TB"))
    }

    pub(super) fn render_engine_panel(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let this = cx.entity();
        let engines = self.model.read(cx).config.engines.clone();
        let limit = self.model.read(cx).config.analysis_limit;
        let chosen = self.panel_engine_config(cx);
        let on = self.live.read(cx).on;
        let reset_layout = this.clone();
        let mut body = panel().flex_none().overflow_hidden().child(
            h_flex()
                .h(px(40.))
                .px(px(16.))
                .justify_between()
                .border_b_1()
                .border_color(rgb(T.border))
                .child(section_label("Engine"))
                .child(
                    btn("reset-panel-sizes", "Reset panel sizes")
                        .h(26.)
                        .text(12.)
                        .on_click(move |_, cx| {
                            reset_layout.update(cx, |screen, cx| {
                                screen.model.update(cx, |model, cx| {
                                    model.config.panels = crate::config::PanelSizes::default();
                                    model.save_config(cx);
                                });
                                cx.notify();
                            })
                        }),
                )
                .child(
                    btn(
                        "engine-power",
                        if on {
                            "Stop analysis"
                        } else {
                            "Start analysis"
                        },
                    )
                    .h(26.)
                    .pad(8.)
                    .text(12.)
                    .on_click({
                        let this = this.clone();
                        move |_, cx| this.update(cx, |screen, cx| screen.toggle_engine(cx))
                    }),
                ),
        );
        if engines.is_empty() {
            return body
                .child(
                    div()
                        .p(px(16.))
                        .text_size(px(13.))
                        .text_color(rgb(T.muted))
                        .child("No engines yet. Add one in Settings › Engines."),
                )
                .into_any_element();
        }
        for config in &engines {
            let is_chosen = chosen.as_ref().is_some_and(|chosen| chosen.id == config.id);
            body = body.child(self.engine_row(config, is_chosen, cx));
        }
        let pos = self.analysed_pos();
        let restricted = self.live.read(cx).search_moves.clone();
        let reset = this.clone();
        body = body.child(
            v_flex()
                .p(px(16.))
                .gap(px(8.))
                .child(
                    h_flex()
                        .justify_between()
                        .child(section_label(if restricted.is_empty() {
                            "Search moves · all"
                        } else {
                            "Search moves · selected only"
                        }))
                        .child(
                            btn("search-all-moves", "All moves")
                                .h(26.)
                                .disabled(restricted.is_empty())
                                .on_click(move |_, cx| {
                                    reset.update(cx, |screen, cx| {
                                        screen
                                            .live
                                            .update(cx, |live, cx| live.clear_search_moves(cx));
                                    })
                                }),
                        ),
                )
                .child(
                    div()
                        .id("search-move-choices")
                        .max_h(px(110.))
                        .overflow_y_scroll()
                        .child(
                            h_flex().flex_wrap().gap(px(4.)).children(
                                pos.legal_moves()
                                    .into_iter()
                                    .enumerate()
                                    .map(|(index, mv)| {
                                        let san =
                                            chess_core::shakmaty::san::San::from_move(&pos, mv)
                                                .to_string();
                                        let uci =
                                            chess_core::shakmaty::uci::UciMove::from_standard(mv)
                                                .to_string();
                                        let this = this.clone();
                                        btn(("search-move", index), san)
                                            .h(26.)
                                            .pad(7.)
                                            .mono()
                                            .selected(restricted.contains(&uci))
                                            .tooltip("Toggle this move in the engine's root search")
                                            .on_click(move |_, cx| {
                                                this.update(cx, |screen, cx| {
                                                    screen.live.update(cx, |live, cx| {
                                                        live.toggle_search_move(mv, cx)
                                                    });
                                                })
                                            })
                                    }),
                            ),
                        ),
                ),
        );
        let mut limits = h_flex().gap(px(4.));
        for (index, (value, label)) in LIMITS.into_iter().enumerate() {
            let this = this.clone();
            limits = limits.child(
                btn(("limit", index), label)
                    .mono()
                    .h(26.)
                    .pad(7.)
                    .text(12.)
                    .selected(limit == value)
                    .on_click(move |_, cx| {
                        this.update(cx, |screen, cx| {
                            screen.live.update(cx, |live, cx| live.set_limit(value, cx));
                        });
                    }),
            );
        }
        let Some(config) = chosen else {
            return body.into_any_element();
        };
        let id = config.id;
        let has_cached = self
            .live
            .read(cx)
            .engines
            .iter()
            .find(|engine| engine.config.id == id)
            .is_some_and(|engine| engine.lines.values().any(|line| line.cached));
        let replace_this = this.clone();
        let workspace = self.workspace.clone();
        body.child(
            div()
                .flex()
                .flex_row()
                .flex_wrap()
                .items_end()
                .gap_x(px(14.))
                .gap_y(px(10.))
                .p(px(16.))
                .child(self.stepper(&config, "MultiPV", "Lines", "", cx))
                .child(self.stepper(&config, "Threads", "Threads", "", cx))
                .child(self.stepper(&config, "Hash", "Hash", " MB", cx))
                .child(
                    v_flex()
                        .gap(px(4.))
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(rgb(T.muted))
                                .child("Search limit"),
                        )
                        .child(limits),
                )
                .child(
                    btn("all-options", "All options")
                        .h(26.)
                        .pad(8.)
                        .text(12.)
                        .tooltip("Every option this engine offers, in Settings › Engines")
                        .on_click(move |window, cx| {
                            if let Some(workspace) = workspace.upgrade() {
                                workspace.update(cx, |workspace, cx| {
                                    workspace.open_engine_settings(id, window, cx);
                                });
                            }
                        }),
                ),
        )
        .when(has_cached, |panel| {
            panel.child(
                btn("replace-cached-analysis", "Replace cached analysis")
                    .tooltip("Discard this saved result and run a fresh search")
                    .on_click(move |_, cx| {
                        replace_this.update(cx, |screen, cx| {
                            screen
                                .live
                                .update(cx, |live, cx| live.replace_cached(id, cx));
                        });
                    }),
            )
        })
        .into_any_element()
    }

    /// The log of the engine the panels show, if it is in analysis.
    fn panel_log(&self, cx: &gpui::App) -> Option<(EngineId, engine::EngineLog)> {
        let chosen = self.panel_engine_config(cx)?.id;
        self.live
            .read(cx)
            .engines
            .iter()
            .find(|engine| engine.config.id == chosen)
            .map(|engine| (chosen, engine.log.clone()))
    }

    pub(super) fn current_log_key(&self, cx: &gpui::App) -> Option<(EngineId, u64, bool)> {
        self.panel_log(cx)
            .map(|(id, log)| (id, log.revision(), self.log_show_info))
    }

    /// Takes a new snapshot only when the log or the filter changed. A selection holds
    /// the log still, so the text under it cannot move, until it is cleared.
    fn refresh_log(&mut self, cx: &gpui::App) {
        let key = self.current_log_key(cx);
        let same_engine = key.map(|key| key.0) == self.log_key.map(|key| key.0);
        if self.log_selection.is_some() && same_engine {
            return;
        }
        self.log_selection = None;
        if key.is_some() && key == self.log_key {
            return;
        }
        self.log_key = key;
        self.log_lines = self
            .panel_log(cx)
            .map(|(_, log)| log.snapshot(self.log_show_info))
            .unwrap_or_default();
    }

    /// The byte under the pointer in a visible log line.
    fn log_index_at(&self, line: usize, position: Point<Pixels>) -> usize {
        self.log_layouts.borrow().get(&line).map_or(0, |layout| {
            match layout.index_for_position(position) {
                Ok(index) | Err(index) => index,
            }
        })
    }

    fn render_log_row(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let Some(line) = self.log_lines.get(index) else {
            return div().into_any_element();
        };
        let (mark, color) = match line.kind {
            LogKind::Sent => (">", T.fg),
            LogKind::Received => ("<", T.fg_2),
            LogKind::Stderr => ("!", T.warn),
            LogKind::System => ("*", T.muted),
        };
        let selected = self
            .log_selection
            .and_then(|selection| selection.range_in(index, &line.text));
        let text = StyledText::new(SharedString::from(line.text.clone())).with_highlights(
            selected.map(|range| {
                (
                    range,
                    HighlightStyle {
                        background_color: Some(rgb(LOG_SELECTION_BG).into()),
                        ..Default::default()
                    },
                )
            }),
        );
        self.log_layouts
            .borrow_mut()
            .insert(index, text.layout().clone());
        // Rows take their text's width; the list scrolls sideways to the longest.
        h_flex()
            .id(("log-line", index))
            .min_w_full()
            .h(px(LOG_ROW_H))
            .px(px(12.))
            .gap(px(8.))
            .whitespace_nowrap()
            .font_family(MONO)
            .text_size(px(12.))
            .text_color(rgb(color))
            .cursor(CursorStyle::IBeam)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &gpui::MouseDownEvent, _, cx| {
                    let at = (index, this.log_index_at(index, event.position));
                    this.log_selection = Some(LogSelection::at(at));
                    this.log_selecting = true;
                    cx.notify();
                }),
            )
            .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
                if !this.log_selecting || !event.dragging() {
                    return;
                }
                let head = (index, this.log_index_at(index, event.position));
                if let Some(selection) = &mut this.log_selection
                    && selection.head != head
                {
                    selection.head = head;
                    cx.notify();
                }
            }))
            .child(div().flex_none().text_color(rgb(T.faint)).child(mark))
            .child(text)
            .into_any_element()
    }

    /// Ends a drag. A click without a drag clears the selection and lets the log move.
    fn end_log_selection(&mut self, cx: &mut Context<Self>) {
        if !std::mem::take(&mut self.log_selecting) {
            return;
        }
        if self
            .log_selection
            .is_some_and(|selection| selection.is_empty())
        {
            self.log_selection = None;
        }
        cx.notify();
    }

    /// Esc clears the log selection and Ctrl+C copies it. Returns whether the key was used.
    fn handle_log_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) -> bool {
        let Some(selection) = self.log_selection.filter(|_| self.log_panel_open) else {
            return false;
        };
        let keystroke = &event.keystroke;
        if keystroke.key == "escape" {
            self.log_selection = None;
            cx.notify();
            true
        } else if keystroke.key == "c" && keystroke.modifiers.secondary(cx) {
            cx.write_to_clipboard(ClipboardItem::new_string(selection.text(&self.log_lines)));
            self.toast("Selection copied.", cx);
            true
        } else {
            false
        }
    }

    pub(super) fn render_log_panel(&mut self, cx: &mut Context<Self>) -> AnyElement {
        self.refresh_log(cx);
        let this = cx.entity();
        let count = self.log_lines.len();
        let holding = self.log_selection.is_some();
        if self.log_follow && count > 0 && !holding {
            self.log_scroll
                .scroll_to_item(count - 1, ScrollStrategy::Bottom);
        }
        let name = self
            .panel_engine_config(cx)
            .map_or_else(|| "No engine".to_owned(), |config| config.name);
        let info_this = this.clone();
        let follow_this = this.clone();
        let copy_this = this.clone();
        let clear_this = this.clone();
        let tools =
            h_flex()
                .h(px(40.))
                .px(px(16.))
                .gap(px(10.))
                .border_b_1()
                .border_color(rgb(T.border))
                .child(section_label("Log"))
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(rgb(T.muted))
                        .child(name),
                )
                .when(holding, |row| {
                    row.child(
                        div()
                            .text_size(px(12.))
                            .text_color(rgb(T.warn))
                            .child("Held while selected. Ctrl+C copies, Esc releases."),
                    )
                })
                .child(div().flex_1())
                .child(
                    div().flex_none().w(px(58.)).child(
                        check("log-info", self.log_show_info, "Info")
                            .text(12.)
                            .on_change(move |value, _, cx| {
                                info_this.update(cx, |screen, cx| {
                                    screen.log_selection = None;
                                    screen.log_show_info = value;
                                    cx.notify();
                                });
                            }),
                    ),
                )
                .child(
                    div().flex_none().w(px(72.)).child(
                        check("log-follow", self.log_follow, "Follow")
                            .text(12.)
                            .on_change(move |value, _, cx| {
                                follow_this.update(cx, |screen, cx| {
                                    screen.log_follow = value;
                                    cx.notify();
                                });
                            }),
                    ),
                )
                .child(
                    btn("log-copy", "Copy")
                        .h(26.)
                        .pad(8.)
                        .text(12.)
                        .on_click(move |_, cx| {
                            copy_this.update(cx, |screen, cx| {
                                // The selection if there is one, else the whole log.
                                let (text, message) = match screen.log_selection {
                                    Some(selection) => {
                                        (selection.text(&screen.log_lines), "Selection copied.")
                                    }
                                    None => (
                                        screen
                                            .log_lines
                                            .iter()
                                            .map(|line| line.text.as_str())
                                            .collect::<Vec<_>>()
                                            .join("\n"),
                                        "Log copied.",
                                    ),
                                };
                                cx.write_to_clipboard(ClipboardItem::new_string(text));
                                screen.toast(message, cx);
                            });
                        }),
                )
                .child(
                    btn("log-window", "Window")
                        .h(26.)
                        .pad(8.)
                        .text(12.)
                        .tooltip("Follow this engine's raw output in its own window")
                        .on_click({
                            let this = cx.entity();
                            move |_, cx| this.update(cx, |screen, cx| screen.open_engine_output(cx))
                        }),
                )
                .child(btn("log-clear", "Clear").h(26.).pad(8.).text(12.).on_click(
                    move |_, cx| {
                        clear_this.update(cx, |screen, cx| {
                            screen.log_selection = None;
                            let chosen = screen.panel_engine_config(cx).map(|config| config.id);
                            if let Some(engine) = screen
                                .live
                                .read(cx)
                                .engines
                                .iter()
                                .find(|engine| Some(engine.config.id) == chosen)
                            {
                                engine.log.clear();
                            }
                            cx.notify();
                        });
                    },
                ));
        let longest = self
            .log_lines
            .iter()
            .enumerate()
            .max_by_key(|(_, line)| line.text.len())
            .map(|(index, _)| index);
        let body: AnyElement = if count == 0 {
            div()
                .p(px(16.))
                .text_size(px(13.))
                .text_color(rgb(T.muted))
                .child("Nothing logged yet. Start the engine to see its UCI traffic.")
                .into_any_element()
        } else {
            uniform_list(
                "engine-log",
                count,
                cx.processor(|this, range: Range<usize>, _, cx| {
                    // Only the rows just built can be hit-tested.
                    this.log_layouts.borrow_mut().clear();
                    range
                        .map(|index| this.render_log_row(index, cx))
                        .collect::<Vec<_>>()
                }),
            )
            .track_scroll(&self.log_scroll)
            .with_horizontal_sizing_behavior(ListHorizontalSizingBehavior::Unconstrained)
            .with_width_from_item(longest)
            .h(px(LOG_ROW_H * 12.))
            .on_scroll(cx.listener(|this, _, _, cx| {
                this.log_follow = false;
                cx.notify();
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.end_log_selection(cx)),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.end_log_selection(cx)),
            )
            .into_any_element()
        };
        panel()
            .flex_none()
            .overflow_hidden()
            .child(tools)
            .child(
                div()
                    .relative()
                    .py(px(6.))
                    .bg(rgb(T.surface_2))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            this.log_follow = false;
                            cx.notify();
                        }),
                    )
                    .child(body)
                    .vertical_scrollbar(&self.log_scroll)
                    .horizontal_scrollbar(&self.log_scroll),
            )
            .into_any_element()
    }

    /// The line as individual moves: click one to go there, drag to highlight several.
    pub(super) fn render_pv(
        &mut self,
        selected: Option<&MoveRow>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let sans: Vec<String> = match (&self.pv_view, selected) {
            (Some(view), _) => view.sans.clone(),
            (None, Some(row)) if !row.pv.is_empty() => row.pv.clone(),
            (None, Some(row)) => vec![row.san.clone()],
            (None, None) => Vec::new(),
        };
        // Numbered from the game position, wherever the preview has got to.
        let text = format_pv(self.tree.node(self.current).ply, &sans);
        let labels: Vec<&str> = if text.is_empty() {
            Vec::new()
        } else {
            text.split(' ').collect()
        };
        let mut flow = div()
            .id("pv-moves")
            .key_context("FastButton")
            .track_focus(&self.pv_focus)
            .flex()
            .flex_row()
            .flex_wrap()
            .mt(px(10.))
            .mb(px(12.))
            .min_h(px(26.))
            .font_family(MONO)
            .text_size(px(14.))
            .line_height(px(26.))
            .text_color(rgb(T.fg))
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _, _, _| this.pv_dragging = false),
            );
        for (index, label) in labels.into_iter().enumerate() {
            let highlighted = self
                .pv_view
                .as_ref()
                .is_some_and(|view| view.contains(index));
            let hovered = self.pv_hover == Some(index) && !highlighted;
            let begin_sans = sans.clone();
            let hover_sans = sans.clone();
            flow = flow.child(
                div()
                    .id(("pv-move", index))
                    .px(px(3.))
                    .cursor_pointer()
                    .rounded(px(3.))
                    .when(highlighted, |chip| {
                        chip.bg(rgb(T.accent)).text_color(rgb(T.accent_fg))
                    })
                    .when(hovered, |chip| chip.bg(rgb(HOVER_BG)).text_color(rgb(T.fg)))
                    .on_hover(cx.listener(move |this, hovering: &bool, window, cx| {
                        this.hover_pv_move(index, *hovering, &hover_sans, window, cx);
                    }))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, window, cx| {
                            this.begin_pv_highlight(begin_sans.clone(), index, window, cx);
                            cx.stop_propagation();
                        }),
                    )
                    .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
                        if this.pv_dragging && event.dragging() {
                            this.extend_pv_highlight(index, cx);
                        } else if this.pv_hover == Some(index) {
                            // The peek board follows the cursor across the move.
                            this.pv_peek_at = Some(event.position);
                            cx.notify();
                        }
                    }))
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(move |this, _, _, cx| this.end_pv_highlight(index, cx)),
                    )
                    .child(label.to_owned()),
            );
        }
        let back_this = cx.entity();
        let peek = self
            .pv_peek_at
            .filter(|_| self.pv_hover.is_some())
            .map(|at| {
                let offset = px(PEEK_OFFSET);
                deferred(
                    anchored()
                        .position(at + point(offset, offset))
                        .snap_to_window_with_margin(px(8.))
                        .child(
                            div()
                                .size(ui::board::snap_board_size(px(PEEK_SIZE), self.scale))
                                .shadow_lg()
                                .child(self.pv_peek.clone()),
                        ),
                )
                .with_priority(1)
            });
        v_flex()
            .child(flow)
            .children(peek)
            .when(self.pv_view.is_some(), |column| {
                column.child(
                    h_flex()
                        .mb(px(12.))
                        .gap(px(10.))
                        .child(
                            div()
                                .flex_1()
                                .text_size(px(12.))
                                .text_color(rgb(T.muted))
                                .child(
                                    "Drag across moves to highlight. Ctrl+C copies, Esc returns.",
                                ),
                        )
                        .child(
                            btn("pv-back", "Back to game")
                                .h(26.)
                                .pad(8.)
                                .text(12.)
                                .on_click(move |_, cx| {
                                    back_this.update(cx, |screen, cx| screen.exit_pv_preview(cx));
                                }),
                        ),
                )
            })
    }

    fn begin_pv_highlight(
        &mut self,
        sans: Vec<String>,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.pv_focus.focus(window, cx);
        self.pv_hover = None;
        // A second click on the only highlighted move goes back to the game.
        self.pv_toggle_off = self
            .pv_view
            .as_ref()
            .is_some_and(|view| view.anchor == index && view.head == index);
        self.pv_view = Some(PvView {
            sans,
            anchor: index,
            head: index,
        });
        self.pv_dragging = true;
        self.show_pv_preview(cx);
    }

    fn extend_pv_highlight(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(view) = &mut self.pv_view else {
            return;
        };
        if view.head != index {
            view.head = index;
            self.pv_toggle_off = false;
            self.show_pv_preview(cx);
        }
    }

    fn end_pv_highlight(&mut self, index: usize, cx: &mut Context<Self>) {
        self.pv_dragging = false;
        let single = self
            .pv_view
            .as_ref()
            .is_some_and(|view| view.anchor == index && view.head == index);
        if std::mem::take(&mut self.pv_toggle_off) && single {
            self.exit_pv_preview(cx);
        }
    }

    /// Hovering a move draws it on the board and marks it in the line.
    fn hover_pv_move(
        &mut self,
        index: usize,
        hovering: bool,
        sans: &[String],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // The move being previewed is already on the board.
        let on_highlight = self
            .pv_view
            .as_ref()
            .is_some_and(|view| view.contains(index));
        if self.pv_dragging || on_highlight || self.pv_view.as_ref().is_some_and(PvView::is_range) {
            return;
        }
        if !hovering {
            // Moving to the next move reports its hover first, so only clear our own.
            if self.pv_hover == Some(index) {
                self.pv_hover = None;
                self.update_arrows(cx);
                cx.notify();
            }
            return;
        }
        self.pv_hover = Some(index);
        let base = &self.tree.node(self.current).pos;
        let (after, moves) = replay(base, &sans[..sans.len().min(index + 1)]);
        // A line that stops applying before this move has no position to show.
        self.pv_peek_at = (moves.len() > index).then(|| window.mouse_position());
        let squares = moves.get(index).and_then(move_squares);
        let arrows: Vec<Arrow> = squares
            .map(|(from, to)| Arrow::new(from, to))
            .into_iter()
            .collect();
        self.board
            .update(cx, |board, cx| board.set_arrows(arrows, cx));
        // The peek board shows the position after the move, the way the main board faces.
        let flipped = self.board.read(cx).is_flipped();
        let mut prefs = self.model.read(cx).config.board.prefs();
        prefs.coordinates = CoordinateStyle::Off;
        prefs.animation_ms = 0;
        self.pv_peek.update(cx, |peek, cx| {
            peek.set_prefs(prefs, cx);
            peek.set_flipped(flipped, cx);
            peek.set_position(after, squares, false, cx);
        });
        cx.notify();
    }

    /// Puts the highlight on the board and points the engine and table at that position.
    /// One move: the position after it. Several: the position before them, each drawn
    /// as an arrow.
    fn show_pv_preview(&mut self, cx: &mut Context<Self>) {
        let Some(view) = &self.pv_view else {
            return;
        };
        let (start, end) = view.range();
        let played = view.played();
        let base = self.tree.node(self.current).pos.clone();
        let (_, moves) = replay(&base, &view.sans[..=end.min(view.sans.len() - 1)]);
        let (shown, _) = replay(&base, &view.sans[..played]);
        let last = played
            .checked_sub(1)
            .and_then(|index| moves.get(index))
            .and_then(move_squares);
        let arrows: Vec<Arrow> = if start == end {
            Vec::new()
        } else {
            moves[start.min(moves.len())..]
                .iter()
                .filter_map(move_squares)
                .map(|(from, to)| Arrow::new(from, to))
                .collect()
        };
        self.board.update(cx, |board, cx| {
            board.set_position(shown, last, false, cx);
            board.set_arrows(arrows, cx);
            board.set_interactive(false, cx);
        });
        // A new position needs a new search, and a new best line in the table.
        if self.pv_played != Some(played) {
            self.pv_played = Some(played);
            self.selected_san = None;
            self.row_chosen = false;
            let extra: Vec<Move> = moves.into_iter().take(played).collect();
            let (tree, current) = (self.tree.clone(), self.current);
            self.live
                .update(cx, |live, cx| live.set_line(&tree, current, &extra, cx));
            self.query_databases(cx);
        }
        cx.notify();
    }

    /// Picking a table row during a preview continues the line with that row. Returns
    /// false when there is no preview to continue.
    pub(super) fn continue_pv_with(&mut self, san: &str, cx: &mut Context<Self>) -> bool {
        let Some(played) = self.pv_view.as_ref().map(PvView::played) else {
            return false;
        };
        if played == 0 {
            self.exit_pv_preview(cx);
            return false;
        }
        let continuation = self
            .rows
            .iter()
            .find(|row| row.san == san)
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
        self.selected_san = Some(san.to_owned());
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
        self.selected_san = None;
        self.row_chosen = false;
        let node = self.tree.node(self.current);
        let pos = node.pos.clone();
        let last = node.mv.as_ref().and_then(move_squares);
        self.board.update(cx, |board, cx| {
            board.set_position(pos, last, false, cx);
            board.set_arrows(Vec::new(), cx);
            board.set_interactive(true, cx);
        });
        let (tree, current) = (self.tree.clone(), self.current);
        self.live
            .update(cx, |live, cx| live.set_position(&tree, current, cx));
        self.query_databases(cx);
        cx.notify();
    }

    /// Esc leaves a preview or a log selection and Ctrl+C copies it, wherever focus has
    /// gone. A log selection comes first. Returns whether the key was used.
    pub fn handle_pv_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) -> bool {
        if self.handle_log_key(event, cx) {
            return true;
        }
        if self.pv_view.is_none() {
            return false;
        }
        let keystroke = &event.keystroke;
        if keystroke.key == "escape" {
            self.exit_pv_preview(cx);
            true
        } else if keystroke.key == "c" && keystroke.modifiers.secondary(cx) {
            self.copy_pv(cx);
            true
        } else {
            false
        }
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
        self.toast("Line copied.", cx);
    }

    /// Heading text while the board shows something other than the game position.
    pub(super) fn pv_preview_label(&self, ply: u32) -> Option<String> {
        let view = self.pv_view.as_ref()?;
        let (start, end) = view.range();
        let line = format_pv(ply + start as u32, &view.sans[start..=end]);
        Some(format!("previewing {line}"))
    }
}

fn move_squares(mv: &Move) -> Option<(chess_core::Square, chess_core::Square)> {
    Some((mv.from()?, mv.to()))
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

#[cfg(test)]
mod log_tests {
    use super::*;

    fn line(text: &str) -> LogLine {
        LogLine {
            kind: LogKind::Received,
            text: text.to_owned(),
        }
    }

    #[test]
    fn log_selection_spans_lines_in_either_direction() {
        let lines = [line("uciok"), line("readyok"), line("bestmove e2e4")];
        let forward = LogSelection {
            anchor: (0, 3),
            head: (2, 8),
        };
        assert_eq!(forward.text(&lines), "ok\nreadyok\nbestmove");
        let backward = LogSelection {
            anchor: forward.head,
            head: forward.anchor,
        };
        assert_eq!(backward.text(&lines), forward.text(&lines));
        let within = LogSelection {
            anchor: (2, 9),
            head: (2, 40),
        };
        assert_eq!(within.text(&lines), "e2e4");
        assert!(LogSelection::at((1, 2)).is_empty());
    }
}
