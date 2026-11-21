//! The Engines panel: one compact row per engine with its search figures, its lines
//! under it, and the online lookups along the bottom.

use gpui::AnyElement;
use ui::widgets::check;

use super::*;
use crate::config::AnalysisLimit;
use engine::EngineConfig;

/// Widths of the line columns: evaluation, WDL, cloud score and the add button.
const COL_EVAL: f32 = 48.;
const COL_WDL: f32 = 36.;
const COL_CLOUD: f32 = 44.;
const COL_ADD: f32 = 24.;
/// Height of one engine line.
const LINE_H: f32 = 32.;

impl AnalyseScreen {
    /// The Engines panel: header with Online, Log, settings and pause; the search
    /// settings when open; then each engine with its lines; then the online footer.
    pub(super) fn render_engines(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let engines = self.model.read(cx).config.engines.clone();
        let mut body = panel()
            .rounded_none()
            .flex_none()
            .overflow_hidden()
            .child(self.render_engines_header(cx));
        if engines.is_empty() {
            return body
                .child(ui::widgets::note(
                    "No engines yet. Add one in Settings › Engines.",
                ))
                .into_any_element();
        }
        if self.engine_settings_open {
            body = body.child(self.render_engine_settings(cx));
        }
        body = body.child(self.render_line_captions());
        let limit = self.model.read(cx).config.analysis_limit;
        let running: Vec<EngineId> = self.live.read(cx).engine_ids();
        for (index, config) in engines.iter().enumerate() {
            body = body.child(self.render_engine_heading(config, index, limit, cx));
            if config.use_analysis && running.contains(&config.id) {
                body = body.child(self.render_engine_lines(config.id, cx));
            }
        }
        body.children(self.render_online_footer(cx))
            .into_any_element()
    }

    /// The panel header: its title, then Online, Log, search settings and pause.
    fn render_engines_header(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let this = cx.entity();
        let live = self.live.read(cx);
        let (on, paused) = (live.on, live.paused);
        let running = on && !paused;
        h_flex()
            .h(px(44.))
            .px(px(16.))
            .gap(px(4.))
            .border_b_as(T.border)
            .child(section_label("Engines"))
            .child(div().flex_1())
            .child(
                btn("online-toggle", "Online")
                    .mini()
                    .icon("icons/cloud.svg")
                    .selected(self.online.open)
                    .tooltip(
                        "Cloud evaluations from chessdb.cn and the ChessTB tablebase. \
                         Sends each position you visit while on.",
                    )
                    .on_click(this.listen(move |screen, _, cx| screen.toggle_online(cx))),
            )
            .child(
                btn("log-toggle", "Log")
                    .mini()
                    .icon("icons/terminal.svg")
                    .selected(self.log_panel_open)
                    .tooltip("What the engines send and receive")
                    .on_click(this.listen(move |screen, _, cx| {
                        screen.log_panel_open = !screen.log_panel_open;
                        cx.notify();
                    })),
            )
            .child(
                icon_btn("engine-settings", "icons/settings.svg")
                    .mini()
                    .w(24.)
                    .pad(0.)
                    .selected(self.engine_settings_open)
                    .tooltip("Lines, search limit, threads and hash")
                    .on_click(this.listen(move |screen, _, cx| {
                        screen.engine_settings_open = !screen.engine_settings_open;
                        cx.notify();
                    })),
            )
            .child(
                icon_btn(
                    "pause-engines",
                    if running {
                        "icons/pause.svg"
                    } else {
                        "icons/play.svg"
                    },
                )
                .mini()
                .w(24.)
                .pad(0.)
                .tooltip(if running {
                    "Pause all engines"
                } else {
                    "Start all engines"
                })
                .on_click(this.listen(move |screen, _, cx| {
                    if screen.live_on(cx) {
                        screen.toggle_pause(cx);
                    } else {
                        screen.toggle_engine(cx);
                    }
                })),
            )
    }

    /// The search settings of the chosen engine: lines, threads and hash, the search
    /// limit and the moves searched.
    fn render_engine_settings(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let this = cx.entity();
        let mut settings = v_flex().bg(rgb(T.surface_2)).border_b_as(T.border);
        let chosen = self.panel_engine_config(cx);
        if let Some(config) = &chosen {
            settings = settings.child(self.render_engine_options_row(config, cx));
        }
        let quality_arrows = self.model.read(cx).config.view.quality_arrows;
        let infinite = self.model.read(cx).config.analysis_limit == AnalysisLimit::Infinite;
        let cached = chosen.as_ref().is_some_and(|config| {
            self.live
                .read(cx)
                .engines
                .iter()
                .find(|engine| engine.config.id == config.id)
                .is_some_and(|engine| engine.lines.values().any(|line| line.cached))
        });
        let chosen_id = chosen.map(|config| config.id);
        settings
            .child(
                h_flex()
                    .flex_wrap()
                    .gap(px(6.))
                    .px(px(16.))
                    .py(px(8.))
                    .border_t_as(T.divider)
                    .child(
                        btn("quality-arrows", "Quality arrows")
                            .compact()
                            .selected(quality_arrows)
                            .tooltip(
                                "An arrow per engine line, coloured from best to blunder, \
                                 instead of one per engine in its own colour",
                            )
                            .on_click(this.listen(move |screen, _, cx| {
                                screen.set_view(cx, |view| {
                                    view.quality_arrows = !view.quality_arrows
                                });
                            })),
                    )
                    .child(div().flex_1())
                    .when_some(chosen_id.filter(|_| cached), |row, id| {
                        row.child(
                            btn("replace-cached-analysis", "Replace cached analysis")
                                .compact()
                                .tooltip("Discard this saved result and run a fresh search")
                                .on_click(this.listen(move |screen, _, cx| {
                                    screen
                                        .live
                                        .update(cx, |live, cx| live.replace_cached(id, cx));
                                })),
                        )
                    }),
            )
            .child(
                h_flex()
                    .flex_wrap()
                    .gap(px(10.))
                    .px(px(16.))
                    .py(px(10.))
                    .border_t_as(T.divider)
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(90.))
                            .text_as(13., T.fg_2)
                            .child(ui::i18n::t("Search limit")),
                    )
                    .child(self.limit_controls.clone())
                    .child(
                        btn("infinite-search", "Infinite")
                            .compact()
                            .selected(infinite)
                            .tooltip("Search until stopped, instead of up to the search limit")
                            .on_click(this.listen(move |screen, _, cx| {
                                screen.live.update(cx, |live, cx| live.toggle_infinite(cx));
                            })),
                    ),
            )
            .child(self.render_search_moves(cx))
    }

    /// Tiny captions over the line columns.
    fn render_line_captions(&self) -> impl IntoElement + use<> {
        let caption = |width: Option<f32>, text: &'static str| {
            div()
                .when_some(width, |el, width| el.col_w(width))
                .when(width.is_none(), |el| el.flex_1())
                .text_as(10., T.faint)
                .child(ui::i18n::t(text).to_uppercase())
        };
        h_flex()
            .h(px(24.))
            .px(px(16.))
            .gap(px(8.))
            .bg(rgb(T.surface_2))
            .border_b_as(T.divider)
            .child(caption(Some(COL_EVAL), "Eval"))
            .when(self.show_wdl, |row| {
                row.child(caption(Some(COL_WDL), "WDL"))
            })
            .child(caption(None, "Line"))
            .when(self.online.open, |row| {
                row.child(caption(Some(COL_CLOUD), "Cloud"))
            })
            .child(div().col_w(COL_ADD))
    }

    /// One engine's row: on/off, its colour and name, what its search is doing, and its
    /// settings; a thin bar along the bottom shows how far it is toward the limit.
    fn render_engine_heading(
        &self,
        config: &EngineConfig,
        index: usize,
        limit: AnalysisLimit,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let this = cx.entity();
        let id = config.id;
        let live = self.live.read(cx);
        let running = live.engines.iter().find(|engine| engine.config.id == id);
        let stats = running.and_then(|engine| engine.stats()).cloned();
        let best = running.and_then(|engine| engine.lines.get(&1));
        let (cached, stale) = (
            best.is_some_and(|line| line.cached),
            best.is_some_and(|line| line.stale),
        );
        let unsupported = self.tree.variant == chess_core::Variant::Chess960
            && !config
                .info
                .options
                .iter()
                .any(|option| option.name == "UCI_Chess960");
        let summary = if !config.use_analysis {
            ui::i18n::t("off").to_owned()
        } else if unsupported {
            ui::i18n::t("No Chess960 support").to_owned()
        } else if stale {
            ui::i18n::t("stale").to_owned()
        } else if cached {
            ui::i18n::t("cached").to_owned()
        } else {
            stats.as_ref().map_or_else(
                || "—".to_owned(),
                |stats| {
                    let mut parts = vec![format!("d{}", stats.depth)];
                    parts.extend(stats.nps.map(engine::format_speed));
                    parts.extend(stats.time_ms.map(super::search_time));
                    parts.join("  ")
                },
            )
        };
        let tip = {
            let mut lines = vec![
                format!("{} {}", config.name, config.version)
                    .trim()
                    .to_owned(),
            ];
            if let Some(stats) = &stats {
                if let Some(sel) = stats.seldepth {
                    lines.push(format!("{} {}/{sel}", ui::i18n::t("Depth"), stats.depth));
                }
                if let Some(nodes) = stats.nodes {
                    lines.push(format!(
                        "{} {}",
                        ui::i18n::t("Nodes"),
                        engine::format_nodes(nodes)
                    ));
                }
                if let Some(full) = stats.hashfull {
                    lines.push(format!(
                        "{} {:.0}%",
                        ui::i18n::t("Hash"),
                        f64::from(full) / 10.
                    ));
                }
                if let Some(hits) = stats.tbhits.filter(|hits| *hits > 0) {
                    lines.push(format!(
                        "{} {}",
                        ui::i18n::t("Tablebase hits"),
                        engine::format_nodes(hits)
                    ));
                }
            }
            SharedString::from(lines.join("\n"))
        };
        let progress = stats
            .as_ref()
            .and_then(|stats| panels::limit_progress(stats, limit))
            .filter(|_| config.use_analysis);
        let colour = board_header::engine_colour(config, index);
        let chosen = self.engine_settings_open && self.panel_engine_id(cx) == Some(id);
        v_flex()
            .border_b_as(T.divider)
            .child(
                h_flex()
                    .h(px(36.))
                    .px(px(16.))
                    .gap(px(8.))
                    .child(
                        div().col_w(18.).child(
                            check(
                                SharedString::from(format!("engine-on-{id}")),
                                config.use_analysis,
                                "",
                            )
                            .on_change(this.listen_with(
                                move |screen, on, _, cx| screen.set_engine_analysis(id, on, cx),
                            )),
                        ),
                    )
                    .child(
                        div()
                            .flex_none()
                            .w(px(3.))
                            .h(px(16.))
                            .rounded(px(2.))
                            .bg(rgb(colour)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(13.))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(rgb(if config.use_analysis { T.fg } else { T.muted }))
                            .child(config.name.clone()),
                    )
                    .child(
                        mono(summary)
                            .id(SharedString::from(format!("engine-stats-{id}")))
                            .flex_none()
                            .whitespace_nowrap()
                            .text_as(12., if unsupported { T.warn } else { T.fg_2 })
                            .tooltip(move |window, cx| {
                                gpui_component::tooltip::Tooltip::new(tip.clone()).build(window, cx)
                            }),
                    )
                    .child(
                        icon_btn(("engine-options", id as usize), "icons/settings.svg")
                            .mini()
                            .w(24.)
                            .pad(0.)
                            .kind(ui::widgets::BtnKind::Ghost)
                            .selected(chosen)
                            .tooltip("Lines, threads and hash for this engine")
                            .on_click(this.listen(move |screen, _, cx| {
                                let open = !(screen.engine_settings_open
                                    && screen.panel_engine == Some(id));
                                screen.panel_engine = Some(id);
                                screen.engine_settings_open = open;
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div().h(px(2.)).w_full().children(
                    progress.map(|fraction| {
                        div().h_full().w(gpui::relative(fraction)).bg(rgb(T.accent))
                    }),
                ),
            )
    }

    /// An engine's lines, best first, or a note while there are none yet.
    fn render_engine_lines(
        &mut self,
        id: EngineId,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let this = cx.entity();
        let rows: Vec<(usize, MoveRow)> = self
            .rows
            .iter()
            .enumerate()
            .filter(|(_, row)| row.engine_id == Some(id) && !row.pv.is_empty())
            .map(|(index, row)| (index, row.clone()))
            .collect();
        let mut lines = v_flex();
        if rows.is_empty() {
            let searching = self.live.read(cx).on && self.live.read(cx).end.is_none();
            // While searching, blank rows hold the height of the lines shown last, so the
            // panels below stay put when the position changes.
            let reserved = if searching {
                self.engine_line_counts.get(&id).copied().unwrap_or(1)
            } else {
                1
            };
            let row = || {
                div()
                    .h(px(LINE_H))
                    .px(px(16.))
                    .flex()
                    .items_center()
                    .border_b_as(T.divider)
            };
            return lines
                .child(row().text_as(12., T.muted).child(ui::i18n::t(if searching {
                    "Waiting for engine lines…"
                } else {
                    "Stopped"
                })))
                .children((1..reserved).map(|_| row()));
        }
        self.engine_line_counts.insert(id, rows.len());
        let quality_arrows = self.model.read(cx).config.view.quality_arrows;
        let white_to_move = self.analysed_pos().turn().is_white();
        let notation = self.model.read(cx).config.board.notation;
        let pos = self.analysed_pos();
        for (index, row) in rows {
            let selected = self.selected.as_ref().is_some_and(|key| key.matches(&row));
            let expanded = self
                .expanded_pv
                .as_ref()
                .is_some_and(|key| key.matches(&row));
            let key = RowKey::of(&row);
            let mv = row
                .san
                .parse::<chess_core::shakmaty::san::San>()
                .ok()
                .and_then(|parsed| parsed.to_move(&pos).ok());
            let shown = display_san(&row.san, mv.as_ref(), notation);
            let label = chess_core::notation::numbered(self.analysed_ply() + 1, &shown);
            let label_colour = row
                .quality
                .filter(|_| quality_arrows)
                .map_or(T.fg, quality_color);
            let (cloud_text, cloud_colour) = row.cloud.as_ref().map_or_else(
                || ("—".to_owned(), T.faint),
                |cloud| online::cloud_cell(cloud, white_to_move),
            );
            let (select, expand, add) = (this.clone(), key.clone(), row.pv.clone());
            let group = SharedString::from(format!("engine-line-{index}"));
            let wdl = moves_table::wdl_percentages(row.wdl);
            lines = lines.child(
                h_flex()
                    .id(("engine-line", index))
                    .group(group.clone())
                    .relative()
                    .h(px(LINE_H))
                    .px(px(16.))
                    .gap(px(8.))
                    .border_b_as(T.divider)
                    .cursor_pointer()
                    .when(selected, |el| el.bg(rgb(T.selected)))
                    .when(!selected, |el| el.hover(|style| style.bg(rgb(T.hover))))
                    .on_click(move |_, _, cx| {
                        let key = key.clone();
                        select.update(cx, |screen, cx| screen.select_row(key, cx));
                    })
                    .when(selected, |el| {
                        el.child(
                            div()
                                .absolute()
                                .left_0()
                                .top_0()
                                .bottom_0()
                                .w(px(3.))
                                .bg(rgb(T.accent)),
                        )
                    })
                    .child(
                        mono(row.score.map_or_else(|| "—".to_owned(), Score::display))
                            .col_w(COL_EVAL)
                            .py(px(2.))
                            .rounded(px(4.))
                            .bg(rgb(T.surface_2))
                            .text_center()
                            .whitespace_nowrap()
                            .text_as(12., T.fg),
                    )
                    .when(self.show_wdl, |el| {
                        el.child(
                            div()
                                .id(("engine-line-wdl", index))
                                .col_w(COL_WDL)
                                .child(moves_table::wdl_bar(row.wdl).into_any_element())
                                .when_some(wdl, |cell, [w, d, l]| {
                                    let text = SharedString::from(format!(
                                        "{} {w:.0}%, {} {d:.0}%, {} {l:.0}%",
                                        ui::i18n::t("White wins"),
                                        ui::i18n::t("draw"),
                                        ui::i18n::t("Black wins"),
                                    ));
                                    cell.tooltip(move |window, cx| {
                                        gpui_component::tooltip::Tooltip::new(text.clone())
                                            .build(window, cx)
                                    })
                                }),
                        )
                    })
                    .child(
                        h_flex()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .gap(px(4.))
                            .child(self.render_compact_pv(
                                &row,
                                index,
                                (&label, label_colour),
                                expanded,
                                cx,
                            ))
                            .child(
                                icon_btn(
                                    ("expand-pv", index),
                                    if expanded {
                                        "icons/chevron-down.svg"
                                    } else {
                                        "icons/chevron-right.svg"
                                    },
                                )
                                .mini()
                                .w(20.)
                                .pad(0.)
                                .kind(ui::widgets::BtnKind::Ghost)
                                .tooltip(ui::i18n::t(if expanded { "Collapse" } else { "Expand" }))
                                .on_click(this.listen(
                                    move |screen, _, cx| {
                                        cx.stop_propagation();
                                        if expanded {
                                            screen.expanded_pv = None;
                                        } else {
                                            screen.select_row(expand.clone(), cx);
                                            screen.expanded_pv = Some(expand.clone());
                                        }
                                        cx.notify();
                                    },
                                )),
                            ),
                    )
                    .when(self.online.open, |el| {
                        el.child(
                            mono(cloud_text)
                                .col_w(COL_CLOUD)
                                .whitespace_nowrap()
                                .text_as(12., cloud_colour),
                        )
                    })
                    .child(
                        div()
                            .col_w(COL_ADD)
                            .opacity(if selected { 1. } else { 0. })
                            .group_hover(group, |style| style.opacity(1.))
                            .child(
                                icon_btn(("add-line", index), "icons/plus.svg")
                                    .mini()
                                    .w(24.)
                                    .pad(0.)
                                    .kind(ui::widgets::BtnKind::Ghost)
                                    .tooltip("Add this line to the game")
                                    .on_click(this.listen(move |screen, _, cx| {
                                        screen.add_line(&add);
                                        screen.model.toast("Line added to the game", cx);
                                        cx.notify();
                                    })),
                            ),
                    ),
            );
            if expanded {
                lines = lines.child(self.render_line_preview(cx));
            }
        }
        lines
    }
}
