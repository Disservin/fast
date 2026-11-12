//! The results: a summary per engine and a table with each position's outcome.

use super::*;

pub(super) fn render_cell(cell: Option<&Cell>) -> Div {
    let base = div()
        .flex_1()
        .min_w(px(140.))
        .overflow_hidden()
        .text_ellipsis()
        .whitespace_nowrap()
        .font_family(MONO);
    match cell {
        None | Some(Cell::Waiting) => base.text_color(rgb(T.faint)).child("—"),
        Some(Cell::Running(search)) => base
            .text_color(rgb(T.muted))
            .child(SharedString::from(format!("… {}", search_text(search)))),
        Some(Cell::Solved(search)) => {
            // The move in green, then where it was found in muted text beside it.
            let found = search
                .found
                .map(|found| format!("d{} {}", found.depth, format_ms(found.time_ms)));
            base.flex()
                .flex_row()
                .gap(px(8.))
                .child(
                    div()
                        .flex_none()
                        .text_color(rgb(T.good))
                        .child(SharedString::from(format!(
                            "✓ {}",
                            search.best.clone().unwrap_or_default()
                        ))),
                )
                .children(found.map(|found| {
                    div()
                        .min_w_0()
                        .overflow_hidden()
                        .text_ellipsis()
                        .text_color(rgb(T.muted))
                        .child(SharedString::from(found))
                }))
        }
        Some(Cell::Failed(search)) => base
            .text_color(rgb(T.q_blunder))
            .child(SharedString::from(format!("✗ {}", search_text(search)))),
        Some(Cell::Error(err)) => base
            .text_color(rgb(T.danger_fg))
            .child(SharedString::from(err.clone())),
    }
}

/// "Qg6 +3.20 d18".
pub(super) fn search_text(search: &Search) -> String {
    let mut parts = Vec::new();
    if let Some(best) = &search.best {
        parts.push(best.clone());
    }
    if let Some(score) = search.score {
        parts.push(score.display());
    }
    if search.depth > 0 {
        parts.push(format!("d{}", search.depth));
    }
    parts.join(" ")
}

/// "850 ms", "4.2 s", "3m 05s".
pub(super) fn format_ms(ms: u64) -> String {
    if ms < 1000 {
        format!("{ms} ms")
    } else if ms < 60_000 {
        format!("{:.1} s", ms as f64 / 1000.)
    } else {
        crate::format::format_duration(ms / 1000)
    }
}

impl TestPositionsScreen {
    pub(super) fn open_line(&self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Ok(record)) = self.lines.get(index).map(|line| &line.parsed) else {
            return;
        };
        let Ok(tree) = chess_core::GameTree::from_fen(&record.fen) else {
            return;
        };
        if let Some(workspace) = self.workspace.upgrade() {
            workspace.update(cx, |workspace, cx| {
                workspace.open_analysis(tree, window, cx)
            });
        }
    }

    pub(super) fn render_summary(&self, run: &Run) -> Div {
        let mut tiles = h_flex().gap(px(12.)).flex_wrap();
        for (slot, name) in run.engines.iter().enumerate() {
            let tally = test_suite::tally(&run.cells[slot]);
            let share = if tally.done == 0 {
                0.
            } else {
                tally.solved as f32 / tally.done as f32
            };
            tiles =
                tiles.child(
                    v_flex()
                        .min_w(px(200.))
                        .flex_1()
                        .gap(px(6.))
                        .p(px(14.))
                        .rounded(px(10.))
                        .bg(rgb(T.tour_tile))
                        .border_1()
                        .border_color(rgb(T.tour_tile_border))
                        .child(
                            div()
                                .text_size(px(13.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .overflow_hidden()
                                .text_ellipsis()
                                .whitespace_nowrap()
                                .child(SharedString::from(name.clone())),
                        )
                        .child(
                            h_flex()
                                .gap(px(8.))
                                .items_baseline()
                                .child(
                                    div()
                                        .text_size(px(26.))
                                        .font_weight(FontWeight::BOLD)
                                        .child(SharedString::from(format!(
                                            "{}/{}",
                                            tally.solved, run.positions
                                        ))),
                                )
                                .child(div().text_as(13., T.muted).child(SharedString::from(
                                    format!("{:.1}% of run", share * 100.),
                                ))),
                        )
                        .child(meter(
                            tally.done as f32 / run.positions.max(1) as f32,
                            T.good,
                        ))
                        .child(
                            h_flex()
                                .justify_between()
                                .text_as(12., T.muted)
                                .child(SharedString::from(format!(
                                    "{} of {} run",
                                    tally.done, run.positions
                                )))
                                .child(
                                    div()
                                        .font_family(MONO)
                                        .child(SharedString::from(format_ms(tally.time_ms))),
                                ),
                        ),
                );
        }
        let status = match run.finished {
            Some(took) if run.cancel.load(Ordering::Relaxed) => {
                format!("Stopped after {}", format_ms(took.as_millis() as u64))
            }
            Some(took) => format!("Finished in {}", format_ms(took.as_millis() as u64)),
            None if run.cancel.load(Ordering::Relaxed) => "Stopping…".to_owned(),
            None => format!(
                "Running for {}",
                format_ms(run.started.elapsed().as_millis() as u64)
            ),
        };
        v_flex()
            .gap(px(10.))
            .child(
                h_flex()
                    .justify_between()
                    .child(section_label(match run.goal {
                        TestGoal::BestMove => "Best move results",
                        TestGoal::Mate => "Mate results",
                    }))
                    .child(
                        div()
                            .text_size(px(12.5))
                            .text_color(rgb(T.muted))
                            .child(SharedString::from(status)),
                    ),
            )
            .child(tiles)
    }

    pub(super) fn render_table(&self, cx: &mut Context<Self>) -> Div {
        let engines: Vec<String> = match &self.run {
            Some(run) => run.engines.clone(),
            None => {
                let config = &self.model.read(cx).config;
                self.settings
                    .engines
                    .iter()
                    .filter_map(|id| config.engines.iter().find(|e| e.id == *id))
                    .map(|e| e.name.clone())
                    .collect()
            }
        };
        let mut header = h_flex()
            .h(px(32.))
            .px(px(12.))
            .gap(px(8.))
            .border_b_1()
            .border_color(rgb(T.border))
            .child(div().w(px(W_LINE)).flex_none().child(section_label("Line")))
            .child(div().w(px(W_ID)).flex_none().child(section_label("ID")))
            .child(
                div()
                    .w(px(W_EXPECT))
                    .flex_none()
                    .child(section_label("Expected")),
            );
        for name in &engines {
            header = header.child(
                div()
                    .flex_1()
                    .min_w(px(140.))
                    .overflow_hidden()
                    .text_ellipsis()
                    .whitespace_nowrap()
                    .text_size(px(11.5))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(T.muted))
                    .child(SharedString::from(name.clone())),
            );
        }
        let count = self.lines.len();
        let list = uniform_list(
            "test-positions-table",
            count,
            cx.processor(|this, range: Range<usize>, _window, cx| {
                range
                    .map(|index| this.render_row(index, cx).into_any_element())
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(&self.scroll)
        .flex_1()
        .min_h_0();
        v_flex()
            .flex_1()
            .min_h(px(240.))
            .rounded(px(10.))
            .bg(rgb(T.surface))
            .border_1()
            .border_color(rgb(T.border))
            .overflow_hidden()
            .child(header)
            .child(list)
    }

    pub(super) fn render_row(
        &mut self,
        index: usize,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let line = &self.lines[index];
        let problem = self.problem(line);
        let this = cx.entity();
        let mut row = h_flex()
            .id(("test-row", index))
            .w_full()
            .h(px(ROW_H))
            .px(px(12.))
            .gap(px(8.))
            .text_size(px(12.5))
            .border_b_1()
            .border_color(rgb(T.divider))
            .child(
                div()
                    .w(px(W_LINE))
                    .flex_none()
                    .font_family(MONO)
                    .text_color(rgb(T.muted))
                    .child(SharedString::from(line.line.to_string())),
            );
        let ellipsis = |width: f32| {
            div()
                .w(px(width))
                .flex_none()
                .overflow_hidden()
                .text_ellipsis()
                .whitespace_nowrap()
        };
        match (&line.parsed, problem) {
            (_, Some(problem)) => {
                // Skipped: shown in red with the reason, never sent to an engine.
                let id = line
                    .parsed
                    .as_ref()
                    .ok()
                    .and_then(|record| record.id.clone())
                    .unwrap_or_default();
                row = row
                    .bg(rgb(T.danger_bg))
                    .text_color(rgb(T.danger_fg))
                    .child(ellipsis(W_ID).child(SharedString::from(id)))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .child(SharedString::from(format!("Skipped: {problem}")))
                            .id(("test-row-problem", index))
                            .tooltip({
                                let text: SharedString = line.text.clone().into();
                                move |window, cx| {
                                    gpui_component::tooltip::Tooltip::new(text.clone())
                                        .build(window, cx)
                                }
                            }),
                    );
            }
            (Ok(record), None) => {
                row = row
                    .cursor_pointer()
                    .hover(|row| row.bg(rgb(T.hover)))
                    .on_click(this.listen_event(move |screen, _, window, cx| {
                        screen.open_line(index, window, cx)
                    }))
                    .child(ellipsis(W_ID).child(SharedString::from(
                        record.id.clone().unwrap_or_else(|| "—".into()),
                    )))
                    .child(
                        ellipsis(W_EXPECT)
                            .font_family(MONO)
                            .child(SharedString::from(record.expectation())),
                    );
                let run = self.run.as_ref();
                let position = run.and_then(|run| run.position_of.get(index).copied().flatten());
                let columns = run.map_or(self.settings.engines.len(), |run| run.engines.len());
                for slot in 0..columns {
                    let cell = run
                        .zip(position)
                        .and_then(|(run, position)| run.cells.get(slot)?.get(position));
                    row = row.child(render_cell(cell));
                }
            }
            (Err(_), None) => unreachable!("a parse error is always a problem"),
        }
        row
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_read_naturally() {
        assert_eq!(format_ms(850), "850 ms");
        assert_eq!(format_ms(4200), "4.2 s");
        assert_eq!(format_ms(185_000), "3m 05s");
    }
}
