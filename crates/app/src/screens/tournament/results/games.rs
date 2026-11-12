//! The finished games table: filtering, sorting and opening a game in Analyse.

use super::*;

impl TournamentScreen {
    /// A games table heading that orders the table by its column: ascending, then
    /// descending, then back to newest first.
    pub(super) fn sort_header(
        &self,
        column: GameSort,
        label: &'static str,
        cx: &mut Context<Self>,
    ) -> ui::widgets::SortHeading {
        let sorted = match self.games_sort {
            Some((c, descending)) if c == column => Some(descending),
            _ => None,
        };
        let this = cx.entity();
        ui::widgets::sort_heading(
            ("games-sort", column as usize),
            label,
            sorted,
            move |_, cx| {
                this.update(cx, |screen, cx| {
                    screen.games_sort = match screen.games_sort {
                        Some((c, false)) if c == column => Some((column, true)),
                        Some((c, true)) if c == column => None,
                        _ => Some((column, false)),
                    };
                    cx.notify();
                })
            },
        )
    }

    /// The finished games, filterable, with a row per game that opens it in Analyse.
    pub(super) fn render_games(&self, run: &Run, cx: &mut Context<Self>) -> Div {
        let filter = self.games_filter;
        let counts = GameFilter::ALL.map(|f| {
            let count = run.games.iter().filter(|row| f.matches(row)).count();
            (f, format!("{} ({count})", f.label()))
        });
        let bar = choice("tournament-filter", counts, filter, cx, |screen, f| {
            screen.games_filter = f
        });
        let mut rows: Vec<&GameRow> = run.games.iter().filter(|row| filter.matches(row)).collect();
        if let Some((column, descending)) = self.games_sort {
            rows.sort_by(|a, b| {
                let order = column.compare(a, b);
                if descending { order.reverse() } else { order }
            });
        }
        rows.truncate(200);
        let body = if run.games.is_empty() {
            small("No finished games yet.").into_any_element()
        } else if rows.is_empty() {
            small("No games match this filter.").into_any_element()
        } else {
            let mut table = v_flex().w_full().gap(px(2.)).child(
                h_flex()
                    .gap(px(12.))
                    .px(px(10.))
                    .py(px(6.))
                    .text_size(px(12.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(T.faint))
                    .child(
                        div()
                            .w(px(72.))
                            .child(self.sort_header(GameSort::Number, "#", cx)),
                    )
                    .child(div().flex_1().min_w_0().child(self.sort_header(
                        GameSort::White,
                        "White",
                        cx,
                    )))
                    .child(div().flex_1().min_w_0().child(self.sort_header(
                        GameSort::Black,
                        "Black",
                        cx,
                    )))
                    .child(
                        div()
                            .w(px(96.))
                            .child(self.sort_header(GameSort::Result, "Result", cx)),
                    )
                    .child(div().flex_1().min_w_0().child(self.sort_header(
                        GameSort::Termination,
                        "Termination",
                        cx,
                    )))
                    .child(div().w(px(96.)).child("")),
            );
            for row in rows {
                let this = cx.entity();
                let (number, index) = (row.game, row.pgn_index);
                let (chip_bg, chip_fg) = chip_colours(row, &run.names);
                let decisive = row.result != "1/2-1/2";
                table = table.child(
                    h_flex()
                        .id(("tournament-game", row.game as usize))
                        .gap(px(12.))
                        .px(px(10.))
                        .py(px(7.))
                        .rounded(px(6.))
                        .items_center()
                        .cursor_pointer()
                        .bg(rgb(if decisive { T.surface_2 } else { T.tour_card }))
                        .hover(|r| r.bg(rgb(T.hover)))
                        .text_size(px(13.))
                        .on_click(this.listen_event(move |screen, _, window, cx| {
                            screen.open_game(number, index, window, cx)
                        }))
                        .child(
                            div()
                                .w(px(72.))
                                .font_family(MONO)
                                .text_color(rgb(T.muted))
                                .child(SharedString::from(format!("#{}", row.game))),
                        )
                        .child(name_cell(&row.white, &run.names))
                        .child(name_cell(&row.black, &run.names))
                        .child(
                            div().w(px(96.)).child(
                                div()
                                    .px(px(8.))
                                    .py(px(2.))
                                    .rounded(px(6.))
                                    .font_family(MONO)
                                    .bg(rgb(chip_bg))
                                    .text_color(rgb(chip_fg))
                                    .child(SharedString::from(row.result.clone())),
                            ),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .text_color(rgb(T.muted))
                                .child(SharedString::from(row.reason.clone())),
                        )
                        .child(
                            div()
                                .w(px(96.))
                                .text_color(rgb(T.engine_a))
                                .child(ui::i18n::t("Analyse →")),
                        ),
                );
            }
            div()
                .id("tournament-games-table")
                .w_full()
                .max_h(px(GAMES_TABLE_H))
                .overflow_y_scrollbar()
                .child(table)
                .into_any_element()
        };
        card()
            .child(
                h_flex()
                    .justify_between()
                    .items_center()
                    .flex_wrap()
                    .gap(px(12.))
                    .child(heading("Games (click to open in Analyse)"))
                    .child(bar),
            )
            .child(body)
    }
}
