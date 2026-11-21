//! The Explorer panel: the moves played from the analysed position in a database, in
//! the user's own games or in their repertoire, and what to do once the game has left
//! known theory.

use chess_core::{display_san, shakmaty::san::San};
use db::MoveStat;

use super::database_explorer::{results_bar, total_stats};
use super::*;
use crate::format::format_count;

/// Where the Explorer's moves come from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum ExplorerSource {
    /// The database chosen in the panel's picker.
    #[default]
    Database,
    /// The user's own games.
    MyGames,
    /// The user's repertoire.
    Repertoire,
}

impl AnalyseScreen {
    /// The Explorer panel: header with the sources, then the moves from the chosen one.
    pub(super) fn render_explorer(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let this = cx.entity();
        let source = self.explorer_source;
        let mut sources = h_flex().gap(px(4.));
        for (index, (value, label)) in [
            (ExplorerSource::Database, "Database"),
            (ExplorerSource::MyGames, "My games"),
            (ExplorerSource::Repertoire, "Repertoire"),
        ]
        .into_iter()
        .enumerate()
        {
            sources = sources.child(
                btn(("explorer-source", index), label)
                    .mini()
                    .selected(source == value)
                    .on_click(this.listen(move |screen, _, cx| {
                        screen.explorer_source = value;
                        screen.explorer_menu = None;
                        cx.notify();
                    })),
            );
        }
        let body = match source {
            ExplorerSource::Database => self.render_database_moves(cx),
            ExplorerSource::MyGames => self.render_my_games_moves(cx),
            ExplorerSource::Repertoire => self.render_repertoire_moves(cx),
        };
        panel()
            .rounded_none()
            .flex_none()
            .overflow_hidden()
            .child(
                h_flex()
                    .h(px(44.))
                    .px(px(16.))
                    .gap(px(8.))
                    .border_b_as(T.border)
                    .child(section_label("Explorer"))
                    .child(div().flex_1())
                    .child(sources),
            )
            .child(body)
            .into_any_element()
    }

    /// The chosen database's moves, with the picker and the number of games above.
    fn render_database_moves(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let total = total_stats(&self.reference);
        let loading = self.db_task.is_some();
        let database = self.explorer_database(cx);
        let picker = h_flex()
            .px(px(16.))
            .h(px(36.))
            .justify_between()
            .border_b_as(T.divider)
            .child(
                div()
                    .w(px(200.))
                    .child(self.render_explorer_picker(200., cx)),
            )
            .child(
                mono(if loading {
                    ui::i18n::t("Loading…").to_owned()
                } else {
                    ui::i18n::tf("{} games", &[&format_count(total.games)])
                })
                .text_as(12., T.muted),
            );
        let content = if let Some(error) = &self.reference_error {
            div()
                .p(px(16.))
                .text_as(13., T.warn)
                .child(ui::i18n::tf("Could not query database: {}", &[error]))
                .into_any_element()
        } else if database.is_none() {
            ui::widgets::note("Add a database in Settings › Databases.").into_any_element()
        } else if self.reference.is_empty() && !loading {
            self.render_out_of_book(cx).into_any_element()
        } else {
            self.render_stat_table("database", &self.reference, database.map(|e| e.path), cx)
        };
        v_flex().child(picker).child(content).into_any_element()
    }

    /// The moves from the user's own games.
    fn render_my_games_moves(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let mine = self
            .model
            .read(cx)
            .my_games()
            .map(|entry| entry.path.clone());
        if mine.is_none() {
            return ui::widgets::note(
                "Download or import your games in the Database tab to see what you played here.",
            )
            .into_any_element();
        }
        if self.mine.is_empty() {
            return ui::widgets::note("You have not reached this position in your games.")
                .into_any_element();
        }
        self.render_stat_table("mine", &self.mine, mine, cx)
    }

    /// The repertoire's moves from here, main move first.
    fn render_repertoire_moves(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let this = cx.entity();
        let path = self.analysed_san_path();
        let model = self.model.read(cx);
        if self.tree.header("FEN").is_some() {
            return ui::widgets::note("Repertoires start from the initial position.")
                .into_any_element();
        }
        let moves: Vec<String> = model
            .repertoire
            .moves_after(&path)
            .map(|moves| moves.iter().map(|node| node.san.clone()).collect())
            .unwrap_or_default();
        if moves.is_empty() {
            return v_flex()
                .child(ui::widgets::note(
                    "Your repertoire has no moves from this position.",
                ))
                .child(
                    h_flex().px(px(16.)).pb(px(12.)).child(
                        btn("explorer-add-rep", "Add position to repertoire")
                            .compact()
                            .on_click(
                                this.listen(|screen, _, cx| screen.add_position_to_repertoire(cx)),
                            ),
                    ),
                )
                .into_any_element();
        }
        let pos = self.analysed_pos();
        let notation = model.config.board.notation;
        let mut rows = v_flex().child(
            table_header()
                .h(px(28.))
                .px(px(16.))
                .gap(px(8.))
                .child(col_heading(px(72.), "Move"))
                .child(section_label("Status")),
        );
        for (index, san) in moves.into_iter().enumerate() {
            let mv = san
                .parse::<San>()
                .ok()
                .and_then(|san| san.to_move(&pos).ok());
            let label = display_san(&san, mv.as_ref(), notation);
            let (status, colour) = if index == 0 {
                ("Main move", T.good)
            } else {
                ("Side line", T.fg_2)
            };
            rows = rows.child(
                h_flex()
                    .h(px(32.))
                    .px(px(16.))
                    .gap(px(8.))
                    .border_b_as(T.divider)
                    .child(self.explorer_move_button(
                        ("rep-play", index),
                        label,
                        san,
                        mv.is_some(),
                        cx,
                    ))
                    .child(div().text_as(12., colour).child(ui::i18n::t(status))),
            );
        }
        rows.into_any_element()
    }

    /// A move that plays itself onto the board when clicked.
    fn explorer_move_button(
        &self,
        id: impl Into<gpui::ElementId>,
        label: String,
        san: String,
        legal: bool,
        cx: &mut Context<Self>,
    ) -> ui::widgets::Btn {
        let this = cx.entity();
        btn(id, label)
            .mini()
            .mono()
            .w(64.)
            .disabled(!legal)
            .on_click(this.listen(move |screen, window, cx| {
                let mut line: Vec<String> = screen
                    .analysed_san_path()
                    .into_iter()
                    .skip(screen.base_san_path().len())
                    .collect();
                line.push(san.clone());
                if let Some(&node) = screen.add_line(&line).last() {
                    screen.jump(node, window, cx);
                }
            }))
    }

    /// The moves with their games, results and average rating, and a total row. The
    /// game counts open those games in the Database tab.
    fn render_stat_table(
        &self,
        id: &'static str,
        stats: &[MoveStat],
        database: Option<std::path::PathBuf>,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let pos = self.analysed_pos();
        let notation = self.model.read(cx).config.board.notation;
        let total = total_stats(stats);
        let header = table_header()
            .h(px(28.))
            .px(px(16.))
            .gap(px(8.))
            .child(col_heading(px(64.), "Move"))
            .child(col_heading(px(96.), "Games"))
            .child(div().flex_1().child(section_label("White / Draw / Black")))
            .child(
                div()
                    .col_w(56.)
                    .flex()
                    .justify_end()
                    .child(section_label("Avg Elo")),
            );
        let mut rows = v_flex()
            .id(gpui::SharedString::from(format!("{id}-explorer-rows")))
            .max_h(px(300.))
            .overflow_y_scroll();
        for (index, stat) in stats.iter().enumerate() {
            let mv = stat
                .san
                .parse::<San>()
                .ok()
                .and_then(|san| san.to_move(&pos).ok());
            let label = display_san(&stat.san, mv.as_ref(), notation);
            let target = database.as_ref().zip(mv).map(|(path, mv)| {
                let mut after = pos.clone();
                after.play_unchecked(mv);
                let numbered = chess_core::notation::numbered(self.analysed_ply() + 1, &label);
                (path.clone(), position_hash(&after), numbered)
            });
            let share = stat.games as f64 * 100. / total.games.max(1) as f64;
            rows = rows.child(
                h_flex()
                    .h(px(32.))
                    .px(px(16.))
                    .gap(px(8.))
                    .border_b_as(T.divider)
                    .hover(|row| row.bg(rgb(T.hover)))
                    .child(self.explorer_move_button(
                        (id, index),
                        label,
                        stat.san.clone(),
                        mv.is_some(),
                        cx,
                    ))
                    .child(
                        h_flex()
                            .col_w(96.)
                            .gap(px(6.))
                            .child(self.games_link((id, index), stat.games, target))
                            .child(mono(format!("{share:.0}%")).text_as(11., T.muted)),
                    )
                    .child(results_bar(stat))
                    .child(elo_cell(stat.avg_elo)),
            );
        }
        let mut table = v_flex().child(header).child(rows);
        if total.games > 0 && stats.len() > 1 {
            table = table.child(
                h_flex()
                    .h(px(32.))
                    .px(px(16.))
                    .gap(px(8.))
                    .border_t_as(T.border)
                    .child(col_heading(px(64.), "Total"))
                    .child(
                        mono(format_count(total.games))
                            .col_w(96.)
                            .text_as(12., T.fg_2)
                            .whitespace_nowrap(),
                    )
                    .child(results_bar(&total))
                    .child(div().col_w(56.)),
            );
        }
        table.into_any_element()
    }

    /// A game count that opens those games in the Database tab.
    fn games_link(
        &self,
        id: (&'static str, usize),
        games: u64,
        target: Option<(std::path::PathBuf, u64, String)>,
    ) -> impl IntoElement + use<> {
        let workspace = self.workspace.clone();
        mono(format_count(games))
            .id(gpui::SharedString::from(format!("{}-count-{}", id.0, id.1)))
            .text_as(12., T.fg_2)
            .whitespace_nowrap()
            .when_some(target, |cell, (path, hash, label)| {
                cell.cursor_pointer()
                    .hover(|style| style.text_color(rgb(T.accent)).underline())
                    .tooltip(|window, cx| {
                        gpui_component::tooltip::Tooltip::new(ui::i18n::t(
                            "Show these games in the Database tab",
                        ))
                        .build(window, cx)
                    })
                    .on_click(move |_, window, cx| {
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
    }

    /// No game in the database reaches the position: where the game left theory,
    /// whether the position is in the repertoire, and buttons to go back to theory or
    /// add the position to the repertoire.
    fn render_out_of_book(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let this = cx.entity();
        let last_book = self.last_book_node();
        let theory = last_book.map(|node| {
            ui::i18n::t("The game left known theory after {}.")
                .replace("{}", &self.tree.move_label(node))
        });
        let path = self.analysed_san_path();
        let in_repertoire = self.tree.header("FEN").is_none() && {
            let model = self.model.read(cx);
            match path.split_last() {
                None => true,
                Some((last, before)) => model
                    .repertoire
                    .moves_after(before)
                    .is_some_and(|moves| moves.iter().any(|node| &node.san == last)),
            }
        };
        let repertoire = if in_repertoire {
            "This position is in your repertoire."
        } else {
            "This position is not in your repertoire."
        };
        v_flex()
            .px(px(16.))
            .py(px(14.))
            .gap(px(6.))
            .child(
                div()
                    .text_size(px(13.))
                    .font_weight(FontWeight::MEDIUM)
                    .child(ui::i18n::t("No games reach this position")),
            )
            .child(
                div()
                    .text_as(12., T.muted)
                    .children(theory.map(|text| format!("{text} ")))
                    .child(ui::i18n::t(repertoire)),
            )
            .child(
                h_flex()
                    .mt(px(4.))
                    .gap(px(6.))
                    .when_some(
                        last_book.filter(|node| *node != self.current),
                        |row, node| {
                            row.child(
                                btn("jump-to-book", "Jump to last book move")
                                    .compact()
                                    .on_click(this.listen(move |screen, window, cx| {
                                        screen.jump(node, window, cx)
                                    })),
                            )
                        },
                    )
                    .when(!in_repertoire && self.tree.header("FEN").is_none(), |row| {
                        row.child(
                            btn("explorer-add-rep", "Add to repertoire")
                                .compact()
                                .on_click(
                                    this.listen(|screen, _, cx| {
                                        screen.add_position_to_repertoire(cx)
                                    }),
                                ),
                        )
                    }),
            )
    }

    /// The last move on the way to the current position that is still named opening
    /// theory, for games from the standard starting position.
    fn last_book_node(&self) -> Option<NodeId> {
        if self.tree.variant != chess_core::Variant::Standard || self.tree.header("FEN").is_some() {
            return None;
        }
        let path = self.tree.path_to(self.current);
        let (_, plies) =
            chess_core::classify_depth(path.iter().map(|&id| self.tree.node(id).san.as_str()))?;
        path.get(plies.checked_sub(1)?).copied()
    }

    /// Adds the moves to the analysed position to the repertoire.
    fn add_position_to_repertoire(&mut self, cx: &mut Context<Self>) {
        let path = self.analysed_san_path();
        let refs: Vec<&str> = path.iter().map(String::as_str).collect();
        self.model
            .update(cx, |model, cx| match model.repertoire.add_line(&refs) {
                Ok(()) => {
                    model.save_repertoire(cx);
                    model.refresh_leaks(cx);
                    model.toast("Added to the repertoire.", cx);
                }
                Err(err) => model.toast(format!("Could not add the line: {err}"), cx),
            });
        self.rebuild_rows(cx);
        cx.notify();
    }
}

/// The average rating, or a dash when no game has both ratings.
fn elo_cell(elo: Option<u32>) -> impl IntoElement {
    mono(elo.map_or_else(|| "—".to_owned(), |elo| elo.to_string()))
        .col_w(56.)
        .flex()
        .justify_end()
        .whitespace_nowrap()
        .text_as(12., if elo.is_some() { T.fg_2 } else { T.faint })
}
