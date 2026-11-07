//! Analyse's engine controls: stats, the Engine and Log panels, and PV highlighting.

use std::ops::Range;

use chess_core::{Move, format_pv, shakmaty::san::San};
use engine::{EngineConfig, EngineId, LogKind, LogLine, OptionKind};
use gpui::{
    AnyElement, ClipboardItem, Context, Div, FontWeight, IntoElement, KeyDownEvent, MouseButton,
    MouseMoveEvent, ScrollStrategy, SharedString, Window, div, prelude::*, px, rgb, uniform_list,
};
use ui::{
    Arrow, MONO, T,
    widgets::{btn, check, h_flex, mono, panel, section_label, v_flex},
};

use super::{AnalyseScreen, MoveRow};
use crate::config::AnalysisLimit;

const LOG_ROW_H: f32 = 19.;
const LIMITS: [(AnalysisLimit, &str); 5] = [
    (AnalysisLimit::Infinite, "∞"),
    (AnalysisLimit::Depth(20), "d20"),
    (AnalysisLimit::Depth(30), "d30"),
    (AnalysisLimit::Seconds(10), "10s"),
    (AnalysisLimit::Seconds(60), "60s"),
];

/// A highlighted run of moves in the selected line. The line is copied so engine
/// updates cannot move it while it is being read.
pub struct PvView {
    pub san: String,
    pub sans: Vec<String>,
    pub anchor: usize,
    pub head: usize,
}

impl PvView {
    /// Inclusive first and last highlighted move.
    pub fn range(&self) -> (usize, usize) {
        (self.anchor.min(self.head), self.anchor.max(self.head))
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
    /// The engine the panels show: the chosen one if still configured, else the first.
    fn panel_engine_config(&self, cx: &gpui::App) -> Option<EngineConfig> {
        let engines = &self.model.read(cx).config.engines;
        self.panel_engine
            .and_then(|id| engines.iter().find(|engine| engine.id == id))
            .or_else(|| engines.iter().find(|engine| engine.use_analysis))
            .or_else(|| engines.first())
            .cloned()
    }

    /// Depth, nodes and speed, with the buttons that open the panels.
    pub(super) fn render_table_tools(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let this = cx.entity();
        let stats = self.live.read(cx).primary().and_then(|engine| {
            let stats = engine.stats()?;
            let mut parts = vec![format!("d{}", stats.depth)];
            parts.extend(stats.nodes.map(engine::format_nodes));
            parts.extend(stats.nps.map(engine::format_speed));
            Some(parts.join(" · "))
        });
        let engine_this = this.clone();
        let log_this = this;
        h_flex()
            .gap(px(6.))
            .when_some(stats.filter(|_| self.show_wdl), |row, stats| {
                row.child(
                    mono(stats)
                        .mr(px(4.))
                        .text_size(px(12.))
                        .text_color(rgb(T.muted)),
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
        let depth = stats.map_or_else(dash, |stats| match stats.seldepth {
            Some(sel) => format!("{}/{sel}", stats.depth),
            None => stats.depth.to_string(),
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
        let mut body = panel().flex_none().overflow_hidden().child(
            h_flex()
                .h(px(40.))
                .px(px(16.))
                .justify_between()
                .border_b_1()
                .border_color(rgb(T.border))
                .child(section_label("Engine"))
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
        .into_any_element()
    }

    fn refresh_log(&mut self, cx: &gpui::App) {
        let chosen = self.panel_engine_config(cx).map(|config| config.id);
        self.log_lines = self
            .live
            .read(cx)
            .engines
            .iter()
            .find(|engine| Some(engine.config.id) == chosen)
            .map(|engine| engine.log.snapshot(self.log_show_info))
            .unwrap_or_default();
    }

    fn render_log_row(line: &LogLine, index: usize) -> impl IntoElement + use<> {
        let (mark, color) = match line.kind {
            LogKind::Sent => (">", T.fg),
            LogKind::Received => ("<", T.fg_2),
            LogKind::Stderr => ("!", T.warn),
            LogKind::System => ("·", T.muted),
        };
        h_flex()
            .id(("log-line", index))
            .w_full()
            .h(px(LOG_ROW_H))
            .px(px(12.))
            .gap(px(8.))
            .overflow_hidden()
            .whitespace_nowrap()
            .font_family(MONO)
            .text_size(px(12.))
            .text_color(rgb(color))
            .child(div().flex_none().text_color(rgb(T.faint)).child(mark))
            .child(SharedString::from(line.text.clone()))
    }

    pub(super) fn render_log_panel(&mut self, cx: &mut Context<Self>) -> AnyElement {
        self.refresh_log(cx);
        let this = cx.entity();
        let count = self.log_lines.len();
        if self.log_follow && count > 0 {
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
                .child(div().flex_1())
                .child(
                    div().flex_none().w(px(58.)).child(
                        check("log-info", self.log_show_info, "Info")
                            .text(12.)
                            .on_change(move |value, _, cx| {
                                info_this.update(cx, |screen, cx| {
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
                                let text: Vec<&str> = screen
                                    .log_lines
                                    .iter()
                                    .map(|line| line.text.as_str())
                                    .collect();
                                cx.write_to_clipboard(ClipboardItem::new_string(text.join("\n")));
                                screen.toast("Log copied.", cx);
                            });
                        }),
                )
                .child(btn("log-clear", "Clear").h(26.).pad(8.).text(12.).on_click(
                    move |_, cx| {
                        clear_this.update(cx, |screen, cx| {
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
                cx.processor(|this, range: Range<usize>, _, _| {
                    range
                        .map(|index| {
                            Self::render_log_row(&this.log_lines[index], index).into_any_element()
                        })
                        .collect::<Vec<_>>()
                }),
            )
            .track_scroll(&self.log_scroll)
            .h(px(LOG_ROW_H * 12.))
            .into_any_element()
        };
        panel()
            .flex_none()
            .overflow_hidden()
            .child(tools)
            .child(div().py(px(6.)).bg(rgb(T.surface_2)).child(body))
            .into_any_element()
    }

    /// The line as individual moves: click one to see it, drag to highlight several.
    pub(super) fn render_pv(
        &mut self,
        selected: Option<&MoveRow>,
        ply: u32,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let view = self
            .pv_view
            .as_ref()
            .filter(|view| selected.is_some_and(|row| row.san == view.san));
        let sans: Vec<String> = match (view, selected) {
            (Some(view), _) => view.sans.clone(),
            (None, Some(row)) if !row.pv.is_empty() => row.pv.clone(),
            (None, Some(row)) => vec![row.san.clone()],
            (None, None) => Vec::new(),
        };
        let row_san = selected.map(|row| row.san.clone()).unwrap_or_default();
        let text = format_pv(ply, &sans);
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
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                let keystroke = &event.keystroke;
                if keystroke.key == "c" && keystroke.modifiers.secondary(cx) {
                    this.copy_pv(cx);
                    cx.stop_propagation();
                } else if keystroke.key == "escape" {
                    this.exit_pv_preview(cx);
                    cx.stop_propagation();
                }
            }))
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _, _, _| this.pv_dragging = false),
            );
        for (index, label) in labels.into_iter().enumerate() {
            let highlighted = self
                .pv_view
                .as_ref()
                .is_some_and(|view| view.san == row_san && view.contains(index));
            let begin_sans = sans.clone();
            let begin_san = row_san.clone();
            flow = flow.child(
                div()
                    .id(("pv-move", index))
                    .px(px(3.))
                    .cursor_pointer()
                    .rounded(px(3.))
                    .when(highlighted, |chip| chip.bg(rgb(T.fg)).text_color(rgb(T.bg)))
                    .when(!highlighted, |chip| {
                        chip.hover(|style| style.bg(rgb(T.hover)))
                    })
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, window, cx| {
                            this.begin_pv_highlight(
                                begin_san.clone(),
                                begin_sans.clone(),
                                index,
                                window,
                                cx,
                            );
                            cx.stop_propagation();
                        }),
                    )
                    .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
                        if this.pv_dragging && event.dragging() {
                            this.extend_pv_highlight(index, cx);
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
        v_flex().child(flow).when(self.pv_view.is_some(), |column| {
            column.child(
                h_flex()
                    .mb(px(12.))
                    .gap(px(10.))
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(12.))
                            .text_color(rgb(T.muted))
                            .child("Drag across moves to highlight. Ctrl+C copies, Esc returns."),
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
        san: String,
        sans: Vec<String>,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.pv_focus.focus(window, cx);
        // A second click on the only highlighted move goes back to the game.
        self.pv_toggle_off = self
            .pv_view
            .as_ref()
            .is_some_and(|view| view.san == san && view.anchor == index && view.head == index);
        self.pv_view = Some(PvView {
            san,
            sans,
            anchor: index,
            head: index,
        });
        self.pv_dragging = true;
        self.row_chosen = true;
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

    /// Shows the highlight on the board. One move: the position after it. Several: the
    /// position before them, with each drawn as an arrow.
    fn show_pv_preview(&mut self, cx: &mut Context<Self>) {
        let Some(view) = &self.pv_view else {
            return;
        };
        let (start, end) = view.range();
        let played = if start == end { end + 1 } else { start };
        let mut pos = self.tree.node(self.current).pos.clone();
        let mut shown = pos.clone();
        let mut last = None;
        let mut arrows = Vec::new();
        for (index, san) in view.sans.iter().enumerate().take(end + 1) {
            let Some(mv) = san
                .parse::<San>()
                .ok()
                .and_then(|parsed| parsed.to_move(&pos).ok())
            else {
                break;
            };
            let squares = move_squares(&mv);
            chess_core::Position::play_unchecked(&mut pos, mv);
            if index < played {
                shown = pos.clone();
                last = squares;
            } else if let Some((from, to)) = squares {
                arrows.push(Arrow { from, to });
            }
        }
        self.board.update(cx, |board, cx| {
            board.set_position(shown, last, false, cx);
            board.set_arrows(arrows, cx);
            board.set_interactive(false, cx);
        });
        cx.notify();
    }

    /// Ends any highlight and puts the game's position back on the board.
    pub(super) fn exit_pv_preview(&mut self, cx: &mut Context<Self>) {
        if self.pv_view.take().is_none() {
            return;
        }
        self.pv_dragging = false;
        let node = self.tree.node(self.current);
        let pos = node.pos.clone();
        let last = node.mv.as_ref().and_then(move_squares);
        self.board.update(cx, |board, cx| {
            board.set_position(pos, last, false, cx);
            board.set_interactive(true, cx);
        });
        self.update_arrows(cx);
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
            san: "f4".to_owned(),
            sans: Vec::new(),
            anchor: 5,
            head: 2,
        };
        assert_eq!(view.range(), (2, 5));
        assert!(view.contains(3));
        assert!(!view.contains(6));
    }
}
