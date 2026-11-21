//! The moves table: one row per candidate move with its engine, database and repertoire figures.

use super::*;

/// Search time as "850 ms", "12.4 s" or "3:05".
pub(crate) fn search_time(ms: u64) -> String {
    match ms {
        0..1_000 => format!("{ms} ms"),
        1_000..60_000 => format!("{:.1} s", ms as f64 / 1000.),
        _ => format!("{}:{:02}", ms / 60_000, (ms / 1000) % 60),
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
        self.expanded_pv = None;
        self.row_chosen = true;
        self.selected = Some(key);
        self.update_arrows(cx);
        cx.notify();
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
