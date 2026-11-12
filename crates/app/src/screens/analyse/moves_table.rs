//! The moves table: one row per candidate move with its engine, database and repertoire figures.

use super::*;

/// Height of one row in Moves from here.
pub(super) const MOVE_ROW_H: f32 = 46.;

/// Search time as "850 ms", "12.4 s" or "3:05".
pub(crate) fn search_time(ms: u64) -> String {
    match ms {
        0..1_000 => format!("{ms} ms"),
        1_000..60_000 => format!("{:.1} s", ms as f64 / 1000.),
        _ => format!("{}:{:02}", ms / 60_000, (ms / 1000) % 60),
    }
}

/// The engine a table row's line comes from: its icon and short name, cut to fit
/// `width`, with the full name on hover. Rows without an engine line keep the space.
pub(super) fn engine_badge(
    config: Option<&engine::EngineConfig>,
    index: usize,
    width: f32,
) -> gpui::AnyElement {
    let Some(config) = config else {
        return div().w(px(width)).flex_none().into_any_element();
    };
    let full = format!("{} {}", config.name, config.version)
        .trim()
        .to_owned();
    h_flex()
        .id(("move-engine", index))
        .w(px(width))
        .flex_none()
        .pr(px(8.))
        .gap(px(6.))
        .tooltip(move |window, cx| {
            gpui_component::tooltip::Tooltip::new(full.clone()).build(window, cx)
        })
        .child(crate::controls::engine_icon(config, 14.))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .text_ellipsis()
                .font_family(ui::SANS)
                .text_size(px(12.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(rgb(T.fg_2))
                .child(short_engine(&config.name)),
        )
        .into_any_element()
}

pub(super) fn short_engine(name: &str) -> String {
    if name.eq_ignore_ascii_case("stockfish") {
        "SF".to_owned()
    } else {
        name.to_owned()
    }
}

pub(super) fn reference_column_title(name: &str) -> String {
    name.rsplit(' ')
        .next()
        .filter(|word| !word.is_empty())
        .unwrap_or("Reference")
        .to_owned()
}

/// Engine WDL is stored White-relative. Normalize by its reported total rather
/// than assuming every engine uses the UCI convention of 1000.
pub(super) fn wdl_percentages(wdl: Option<Wdl>) -> Option<[f64; 3]> {
    let wdl = wdl?;
    let total = u64::from(wdl.win) + u64::from(wdl.draw) + u64::from(wdl.loss);
    (total > 0)
        .then(|| [wdl.win, wdl.draw, wdl.loss].map(|value| f64::from(value) * 100.0 / total as f64))
}

/// White win / draw / Black win, in the eval bar's colours. Missing data is shown explicitly.
pub(super) fn wdl_bar(wdl: Option<Wdl>) -> gpui::AnyElement {
    let Some(wdl) = wdl.filter(|wdl| wdl_percentages(Some(*wdl)).is_some()) else {
        return div().text_as(11., T.muted).child("—").into_any_element();
    };
    h_flex()
        .h(px(10.))
        .w_full()
        .overflow_hidden()
        .rounded(px(1.))
        .children(
            [
                (wdl.win, T.fg),
                (wdl.draw, T.bar),
                (wdl.loss, T.tour_tile_border),
            ]
            .into_iter()
            .filter(|(value, _)| *value > 0)
            .map(|(value, color)| {
                div()
                    .h_full()
                    .flex_grow(value as f32)
                    .flex_basis(px(0.))
                    .min_w(px(0.))
                    .bg(rgb(color))
            }),
        )
        .into_any_element()
}

const COL_REF: f32 = 84.;
const COL_BADGE: f32 = 96.;
const COL_MOVE: f32 = 64.;
const COL_SF: f32 = 66.;
const COL_WDL: f32 = 112.;
const COL_MY: f32 = 80.;
const COL_TB: f32 = 96.;
const COL_CLOUD: f32 = 70.;

/// What the moves table shows for the analysed position, read once per draw.
struct TableView {
    notation: chess_core::Notation,
    engine_badges: bool,
    engine_configs: HashMap<EngineId, engine::EngineConfig>,
    show_wdl: bool,
    online: bool,
    quality_arrows: bool,
    my_moves: bool,
    ply: u32,
    white_to_move: bool,
    pos: chess_core::Chess,
    explorer: Option<DbEntry>,
}

impl TableView {
    /// The table's narrowest width, so its columns scroll sideways instead of squeezing.
    fn min_width(&self) -> f32 {
        let lead = if self.engine_badges { COL_BADGE } else { 0. };
        lead + COL_MOVE
            + COL_SF
            + if self.show_wdl { COL_WDL } else { 0. }
            + if self.online { COL_TB + COL_CLOUD } else { 0. }
            + COL_REF
            + COL_MY
            + 120.
    }
}

impl AnalyseScreen {
    pub(super) fn rebuild_rows(&mut self, cx: &mut Context<Self>) {
        let white_to_move = self.analysed_pos().turn().is_white();
        let model = self.model.read(cx);
        let live = self.live.read(cx);
        let mut lines: Vec<MoveRow> = Vec::new();
        // A row per engine line, so engines can be compared move by move in one
        // column. The primary engine goes first, so on equal scores its row leads.
        let primary_id = live.primary().map(|engine| engine.config.id);
        let mut engines: Vec<&crate::live::LiveEngine> = live.engines.iter().collect();
        engines.sort_by_key(|engine| Some(engine.config.id) != primary_id);
        for engine in engines {
            let show_wdl = crate::live::wdl_visible(&engine.config, true);
            for line in engine.lines.values() {
                let Some(san) = line.sans.first() else {
                    continue;
                };
                lines.push(MoveRow {
                    san: san.clone(),
                    score: line.score,
                    wdl: line.wdl.filter(|_| show_wdl),
                    pv: line.sans.clone(),
                    depth: line.update.depth,
                    engine_id: Some(engine.config.id),
                    ..Default::default()
                });
            }
        }
        let path = self.analysed_san_path();
        // Repertoires are stored as lines from the initial position.
        let from_start = self.tree.header("FEN").is_none();
        let rep_moves: Vec<(String, bool)> = model
            .repertoire
            .moves_after(&path)
            .filter(|_| from_start && model.config.view.my_moves)
            .map(|moves| {
                moves
                    .iter()
                    .enumerate()
                    .map(|(rank, node)| (node.san.clone(), rank == 0))
                    .collect()
            })
            .unwrap_or_default();
        // Online moves join the rows of the same move rather than listing them twice.
        self.merge_online(&mut lines, model.config.view.all_moves);
        let view = &model.config.view;
        let legal: Vec<String> = if view.all_moves {
            let pos = self.analysed_pos();
            pos.legal_moves()
                .iter()
                .map(|mv| {
                    chess_core::shakmaty::san::SanPlus::from_move(pos.clone(), *mv).to_string()
                })
                .collect()
        } else {
            Vec::new()
        };
        self.rows = merge_rows(
            lines,
            self.reference.clone(),
            // With My moves off, the user's own games add no rows and no figures.
            if view.my_moves {
                self.mine.clone()
            } else {
                Vec::new()
            },
            &rep_moves,
            &legal,
            RowOptions {
                white_to_move,
                user_to_move: white_to_move == model.repertoire.white,
                sort: view.sort,
            },
        );
        // The selection follows the best line until the user picks a row.
        // A kibitz preview holds the row whose line is being looked at.
        if !self.row_chosen
            && !self.pv_kibitz
            && let Some(first) = self.rows.iter().find(|row| !row.pv.is_empty())
        {
            self.selected = Some(RowKey::of(first));
        }
        self.update_arrows(cx);
        cx.notify();
    }

    pub(super) fn selected_row(&self) -> Option<&MoveRow> {
        let key = self.selected.as_ref()?;
        self.rows
            .iter()
            .find(|row| key.matches(row))
            // The chosen engine may have dropped the move; another row for it will do.
            .or_else(|| self.rows.iter().find(|row| row.san == key.san))
    }

    pub(super) fn select_row(&mut self, key: RowKey, cx: &mut Context<Self>) {
        if self.continue_pv_with(&key, cx) {
            return;
        }
        self.row_chosen = true;
        self.selected = Some(key);
        self.update_arrows(cx);
        cx.notify();
    }

    /// The moves table: its tools, the online status, a header row and one row per move.
    pub(super) fn render_moves_table(
        &mut self,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let this = cx.entity();
        let explorer = self.explorer_database(cx);
        let picker = if self.engine_tab == panels::EngineTab::Database {
            col_heading(px(COL_REF), "Database").into_any_element()
        } else {
            self.render_explorer_picker(COL_REF, cx)
        };
        let model = self.model.read(cx);
        let live = self.live.read(cx);
        let pos = self.analysed_pos();
        let view = TableView {
            notation: model.config.board.notation,
            // With several engines each row names the engine whose line it is.
            engine_badges: live.engines.len() > 1,
            engine_configs: live
                .engines
                .iter()
                .map(|engine| (engine.config.id, engine.config.clone()))
                .collect(),
            show_wdl: self.show_wdl,
            online: self.online.open,
            quality_arrows: model.config.view.quality_arrows,
            my_moves: model.config.view.my_moves,
            ply: self.analysed_ply(),
            white_to_move: pos.turn().is_white(),
            pos,
            explorer,
        };
        let no_engines = model.analysis_engines().is_empty();
        let searching = live.on && live.end.is_none();
        let header = Self::render_table_header(&view, picker);
        // Room for every requested line is kept while the engine runs, so lines
        // arriving one by one early in a search do not move what is below.
        // A position never has more lines than legal moves, however many are asked for.
        let legal = view.pos.legal_moves().len().max(1) as i64;
        let reserved = self
            .shown_lines(cx)
            .filter(|_| searching)
            .map_or(0., |lines| lines.clamp(1, legal) as f32 * MOVE_ROW_H);
        let mut rows = v_flex().w_full().min_h(px(reserved));
        for (index, row) in self.rows.iter().enumerate() {
            rows = rows.child(self.render_move_row(index, row, &view, &this));
        }
        if !self.rows.is_empty() && self.reference.is_empty() && view.explorer.is_some() {
            rows = rows.child(
                div()
                    .px(px(16.))
                    .py(px(8.))
                    .text_as(12., T.muted)
                    .child(ui::i18n::t("No games reach this position")),
            );
        }
        if self.rows.is_empty() {
            rows = rows.child(
                div()
                    .px(px(16.))
                    .py(px(20.))
                    .text_as(13., T.muted)
                    .child(if no_engines {
                        "No candidate moves yet. Add an engine in Settings › Engines, or import a database."
                    } else {
                        "Waiting for engine lines…"
                    }),
            );
        }
        let (toggles, settings) = self.render_table_tools(cx);
        panel()
            .rounded_none()
            .flex_none()
            .overflow_hidden()
            .child(
                v_flex().px(px(16.)).py(px(10.)).gap(px(8.)).child(
                    h_flex()
                        .flex_wrap()
                        .gap(px(8.))
                        .child(div().flex_1().child(settings))
                        .child(toggles),
                ),
            )
            .children(self.render_online_status())
            .child(
                div()
                    .id("candidate-table-scroll")
                    .overflow_x_scroll()
                    .child(
                        v_flex()
                            .min_w(px(view.min_width()))
                            .child(header)
                            .child(rows),
                    ),
            )
    }

    /// The header row of section labels, one per shown column.
    fn render_table_header(view: &TableView, picker: gpui::AnyElement) -> gpui::Div {
        table_header()
            .h(px(32.))
            .px(px(16.))
            .when(view.engine_badges, |header| {
                header.child(col_heading(px(COL_BADGE), "Engine"))
            })
            .child(col_heading(px(COL_MOVE), "Move"))
            .child(col_heading(px(COL_SF), "Eval"))
            .when(view.show_wdl, |header| {
                header.child(col_heading(px(COL_WDL), "W / D / L"))
            })
            .when(view.online, |header| {
                header
                    .child(col_heading(px(COL_TB), "Tablebase"))
                    .child(col_heading(px(COL_CLOUD), "Cloud"))
            })
            .child(picker)
            .when(view.my_moves, |header| {
                header
                    .child(col_heading(px(COL_MY), "My games"))
                    .child(div().flex_1().min_w_0().child(section_label("Repertoire")))
            })
    }

    /// One move's row: its engine, the move, the evaluation, the WDL bar, online,
    /// database and personal figures, and its repertoire tag. Clicking selects it.
    fn render_move_row(
        &self,
        index: usize,
        row: &MoveRow,
        view: &TableView,
        this: &Entity<Self>,
    ) -> impl IntoElement + use<> {
        let selected = self.selected.as_ref().is_some_and(|key| key.matches(row));
        let key = RowKey::of(row);
        let mv = row
            .san
            .parse::<chess_core::shakmaty::san::San>()
            .ok()
            .and_then(|parsed| parsed.to_move(&view.pos).ok());
        let shown = display_san(&row.san, mv.as_ref(), view.notation);
        let label = chess_core::notation::numbered(view.ply + 1, &shown);
        let label_color = row
            .quality
            .filter(|_| view.quality_arrows)
            .map_or(T.fg, quality_color);
        let select_this = this.clone();
        let score = row
            .score
            .map(Score::display)
            .unwrap_or_else(|| "—".to_owned());
        let badge = view.engine_badges.then(|| {
            let config = row.engine_id.and_then(|id| view.engine_configs.get(&id));
            engine_badge(config, index, COL_BADGE)
        });
        let stat_text = |stat: &Option<MoveStat>| {
            stat.as_ref()
                .map(|stat| {
                    format!(
                        "{} ({:.0}%)",
                        stat.games,
                        stat.score_percent(view.white_to_move)
                    )
                })
                .unwrap_or_else(|| "—".to_owned())
        };
        let (rep_text, rep_color) = match row.rep {
            RepTag::Main => ("Main", T.good),
            RepTag::Side => ("Side line", T.fg_2),
            RepTag::Deviate => ("You deviate", T.warn),
            RepTag::None => ("—", T.muted),
        };
        let dash = || ("—".to_owned(), T.muted);
        let (tb_text, tb_color) = row.tb.as_ref().map_or_else(dash, online::tb_cell);
        let (cloud_text, cloud_color) = row.cloud.as_ref().map_or_else(dash, online::cloud_cell);
        // A count opens the Database tab on the games reaching the position after it.
        let count_target = view
            .explorer
            .as_ref()
            .filter(|_| row.reference.is_some())
            .zip(mv)
            .map(|(entry, mv)| {
                let mut after = view.pos.clone();
                after.play_unchecked(mv);
                (entry.path.clone(), position_hash(&after), label.clone())
            });
        let workspace = self.workspace.clone();
        h_flex()
            .id(("move-row", index))
            .relative()
            .h(px(MOVE_ROW_H))
            .flex_none()
            .px(px(16.))
            .cursor_pointer()
            .border_b_1()
            .border_color(rgb(T.divider))
            .text_size(px(14.))
            .font_family(MONO)
            .whitespace_nowrap()
            .when(selected, |row_el| row_el.bg(rgb(T.selected)))
            .when(!selected, |row_el| {
                row_el.hover(|style| style.bg(rgb(T.hover)))
            })
            .on_click(move |_, _, cx| {
                let key = key.clone();
                select_this.update(cx, |screen, cx| screen.select_row(key, cx));
            })
            .when(selected, |row_el| {
                row_el.child(
                    div()
                        .absolute()
                        .left_0()
                        .top_0()
                        .bottom_0()
                        .w(px(3.))
                        .bg(rgb(T.accent)),
                )
            })
            .children(badge)
            .child(
                div()
                    .w(px(COL_MOVE))
                    .flex_none()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(label_color))
                    .child(ui::i18n::t(&label).to_owned()),
            )
            .child(
                div()
                    .w(px(COL_SF))
                    .flex_none()
                    .text_as(13., T.fg_2)
                    .child(score),
            )
            .when(view.show_wdl, |row_el| {
                row_el.child(div().w(px(COL_WDL)).flex_none().pr(px(12.)).child(
                    match wdl_percentages(row.wdl) {
                        Some([w, d, l]) => {
                            v_flex().gap(px(2.)).child(wdl_bar(row.wdl)).child(
                                mono(format!("{w:.0} / {d:.0} / {l:.0}%")).text_as(10., T.fg_2),
                            )
                        }
                        None => mono("—").text_as(13., T.fg_2),
                    },
                ))
            })
            .when(view.online, |row_el| {
                row_el
                    .child(
                        div()
                            .w(px(COL_TB))
                            .flex_none()
                            .pr(px(8.))
                            .overflow_hidden()
                            .text_ellipsis()
                            .font_family(ui::SANS)
                            .text_size(px(12.))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(rgb(tb_color))
                            .child(tb_text),
                    )
                    .child(
                        div()
                            .w(px(COL_CLOUD))
                            .flex_none()
                            .text_size(px(13.))
                            .text_color(rgb(cloud_color))
                            .child(cloud_text),
                    )
            })
            .child(
                div()
                    .id(("reference-count", index))
                    .w(px(COL_REF))
                    .flex_none()
                    .pl(px(4.))
                    .text_as(13., T.fg_2)
                    .when_some(count_target, |cell, (path, hash, label)| {
                        cell.hover(|style| style.text_color(rgb(T.accent)).underline())
                            .tooltip(|window, cx| {
                                gpui_component::tooltip::Tooltip::new(ui::i18n::t(
                                    "Show these games in the Database tab",
                                ))
                                .build(window, cx)
                            })
                            .on_click(move |_, window, cx| {
                                cx.stop_propagation();
                                let (path, label) = (path.clone(), label.clone());
                                if let Some(workspace) = workspace.upgrade() {
                                    workspace.update(cx, |workspace, cx| {
                                        workspace.show_position_in_database(
                                            path,
                                            hash,
                                            ui::i18n::tf("After {}", &[&label]),
                                            window,
                                            cx,
                                        );
                                    });
                                }
                            })
                    })
                    .child(stat_text(&row.reference)),
            )
            .when(view.my_moves, |row_el| {
                row_el
                    .child(
                        div()
                            .w(px(COL_MY))
                            .flex_none()
                            .text_as(13., T.fg_2)
                            .child(stat_text(&row.mine)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .font_family(ui::SANS)
                            .text_size(px(12.))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(rgb(rep_color))
                            .child(rep_text),
                    )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wdl_values_have_clear_white_relative_percentages() {
        let value = Wdl {
            win: 600,
            draw: 300,
            loss: 100,
        };
        assert_eq!(wdl_percentages(Some(value)), Some([60.0, 30.0, 10.0]));
        assert_eq!(
            wdl_percentages(Some(value.flip())),
            Some([10.0, 30.0, 60.0])
        );
        assert_eq!(
            wdl_percentages(Some(Wdl {
                win: 0,
                draw: 0,
                loss: 0
            })),
            None
        );
        assert_eq!(wdl_percentages(None), None);
        assert_eq!(
            wdl_percentages(Some(Wdl {
                win: 1,
                draw: 1,
                loss: 2
            })),
            Some([25.0, 25.0, 50.0])
        );
    }

    #[test]
    fn search_times_read_naturally() {
        assert_eq!(search_time(850), "850 ms");
        assert_eq!(search_time(12_400), "12.4 s");
        assert_eq!(search_time(185_000), "3:05");
    }
}
