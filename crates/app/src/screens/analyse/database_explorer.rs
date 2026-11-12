//! Local game frequencies and results for the position being analysed.

use chess_core::{display_san, shakmaty::san::San};
use db::MoveStat;
use gpui::{Context, IntoElement, div, px, rgb};
use ui::{
    T,
    widgets::{btn, h_flex, mono, section_label, v_flex},
};

use super::*;
use crate::format::format_count;

/// Adds the move counts and results for the explorer's total row.
fn total_stats(moves: &[MoveStat]) -> MoveStat {
    moves.iter().fold(
        MoveStat {
            san: String::new(),
            games: 0,
            white_wins: 0,
            draws: 0,
            black_wins: 0,
        },
        |mut total, stat| {
            total.games += stat.games;
            total.white_wins += stat.white_wins;
            total.draws += stat.draws;
            total.black_wins += stat.black_wins;
            total
        },
    )
}

/// Shares of all recorded games, keeping unfinished games separate from draws.
fn result_shares(stat: &MoveStat) -> [f64; 4] {
    let unknown = stat
        .games
        .saturating_sub(stat.white_wins + stat.draws + stat.black_wins);
    [stat.white_wins, stat.draws, stat.black_wins, unknown]
        .map(|count| count as f64 / stat.games.max(1) as f64)
}

/// Shows White wins, draws and Black wins with exact percentages available on hover.
fn results_bar(stat: &MoveStat) -> gpui::AnyElement {
    let shares = result_shares(stat);
    let tooltip = ui::i18n::tf(
        "White wins: {}%, draws: {}%, Black wins: {}%, unfinished: {}%",
        &shares
            .map(|share| format!("{:.1}", share * 100.))
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
    );
    h_flex()
        .id(gpui::SharedString::from(format!(
            "database-results-{}",
            stat.san
        )))
        .flex_1()
        .min_w_0()
        .h(px(20.))
        .border_1()
        .border_color(rgb(T.border_strong))
        .rounded(px(4.))
        .overflow_hidden()
        .tooltip(move |window, cx| {
            gpui_component::tooltip::Tooltip::new(tooltip.clone()).build(window, cx)
        })
        .children(
            shares
                .into_iter()
                .enumerate()
                .filter(|(_, share)| *share > 0.)
                .map(|(index, share)| {
                    let (bg, fg) = match index {
                        0 => (T.fg, T.bg),
                        1 => (T.selected, T.fg),
                        2 => (T.bg, T.fg),
                        _ => (T.surface_2, T.fg_2),
                    };
                    h_flex()
                        .h_full()
                        .flex_grow(share as f32)
                        .flex_basis(px(0.))
                        .min_w_0()
                        .overflow_hidden()
                        .justify_center()
                        .bg(rgb(bg))
                        .text_color(rgb(fg))
                        .when(share >= 0.2, |part| {
                            part.child(
                                mono(if index == 3 {
                                    "—".to_owned()
                                } else {
                                    format!("{:.1}%", share * 100.)
                                })
                                .text_size(px(10.)),
                            )
                        })
                }),
        )
        .into_any_element()
}

impl AnalyseScreen {
    /// Lists every recorded continuation, with the same database choice as the moves table.
    pub(super) fn render_database_explorer(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let total = total_stats(&self.reference);
        let loading = self.db_task.is_some();
        let database = self.explorer_database(cx);
        let pos = self.analysed_pos();
        let notation = self.model.read(cx).config.board.notation;
        let mut body = v_flex()
            .min_w_0()
            .child(
                h_flex()
                    .px(px(16.))
                    .h(px(40.))
                    .justify_between()
                    .child(self.render_explorer_picker(180., cx))
                    .child(
                        mono(if loading {
                            ui::i18n::t("Loading…").to_owned()
                        } else {
                            ui::i18n::tf("{} games", &[&format_count(total.games)])
                        })
                        .text_size(px(12.))
                        .text_color(rgb(T.muted)),
                    ),
            )
            .child(
                h_flex()
                    .h(px(28.))
                    .px(px(16.))
                    .gap(px(8.))
                    .bg(rgb(T.surface_2))
                    .child(div().w(px(56.)).flex_none().child(section_label("Move")))
                    .child(div().w(px(48.)).flex_none().child(section_label("Played")))
                    .child(div().w(px(82.)).flex_none().child(section_label("Games")))
                    .child(section_label("White / Draw / Black")),
            );
        let mut rows = v_flex()
            .id("database-explorer-rows")
            .max_h(px(280.))
            .overflow_y_scroll();
        for (index, stat) in self.reference.iter().enumerate() {
            let mv = stat
                .san
                .parse::<San>()
                .ok()
                .and_then(|san| san.to_move(&pos).ok());
            let label = display_san(&stat.san, mv.as_ref(), notation);
            let san = stat.san.clone();
            let this = cx.entity();
            rows = rows.child(
                h_flex()
                    .id(("database-move", index))
                    .h(px(32.))
                    .px(px(16.))
                    .gap(px(8.))
                    .border_b_1()
                    .border_color(rgb(T.divider))
                    .hover(|row| row.bg(rgb(T.hover)))
                    .child(
                        btn(("database-play", index), label)
                            .mini()
                            .mono()
                            .w(56.)
                            .disabled(mv.is_none())
                            .on_click(move |window, cx| {
                                this.update(cx, |screen, cx| {
                                    let mut line: Vec<String> = screen
                                        .analysed_san_path()
                                        .into_iter()
                                        .skip(screen.base_san_path().len())
                                        .collect();
                                    line.push(san.clone());
                                    if let Some(&node) = screen.add_line(&line).last() {
                                        screen.jump(node, window, cx);
                                    }
                                });
                            }),
                    )
                    .child(
                        mono(format!(
                            "{:.1}%",
                            stat.games as f64 * 100. / total.games.max(1) as f64
                        ))
                        .text_size(px(12.))
                        .w(px(48.))
                        .flex_none()
                        .whitespace_nowrap(),
                    )
                    .child(
                        mono(format_count(stat.games))
                            .text_size(px(12.))
                            .w(px(82.))
                            .flex_none()
                            .whitespace_nowrap(),
                    )
                    .child(results_bar(stat)),
            );
        }
        if let Some(error) = &self.reference_error {
            rows = rows.child(
                div()
                    .p(px(16.))
                    .text_size(px(13.))
                    .text_color(rgb(T.warn))
                    .child(ui::i18n::tf("Could not query database: {}", &[error])),
            );
        } else if self.reference.is_empty() {
            rows = rows.child(
                div()
                    .p(px(16.))
                    .text_size(px(13.))
                    .text_color(rgb(T.muted))
                    .child(ui::i18n::t(if loading {
                        "Loading…"
                    } else if database.is_none() {
                        "Add a database in Settings › Databases."
                    } else {
                        "No games reach this position."
                    })),
            );
        }
        body = body.child(rows);
        if total.games > 0 {
            body = body.child(
                h_flex()
                    .h(px(32.))
                    .px(px(16.))
                    .gap(px(8.))
                    .border_t_1()
                    .border_color(rgb(T.border))
                    .child(div().w(px(112.)).flex_none().child(section_label("Total")))
                    .child(
                        mono(format_count(total.games))
                            .text_size(px(12.))
                            .w(px(82.))
                            .flex_none()
                            .whitespace_nowrap(),
                    )
                    .child(results_bar(&total)),
            );
        }
        body.into_any_element()
    }
}

/// A database name short enough for the Reference column's heading: "Reference: OTB
/// masters" becomes "masters", other names stay whole and are truncated if needed.
pub(super) fn explorer_label(name: &str) -> String {
    if name.contains(':') {
        reference_column_title(name)
    } else {
        name.to_owned()
    }
}

impl AnalyseScreen {
    /// The database the Reference column counts: this tab's choice, the open game's
    /// database, the last database picked anywhere, then the first reference database.
    pub fn explorer_database(&self, cx: &gpui::App) -> Option<DbEntry> {
        let model = self.model.read(cx);
        let paths: Vec<std::path::PathBuf> = model
            .databases
            .iter()
            .map(|entry| entry.path.clone())
            .collect();
        let pick = crate::explorer::pick_database(
            self.explorer_db.as_deref(),
            self.db_game.as_ref().map(|game| game.path.as_path()),
            model.config.explorer_default.as_deref(),
            model.reference_db().map(|entry| entry.path.as_path()),
            &paths,
        )?;
        model
            .databases
            .iter()
            .find(|entry| entry.path == pick)
            .cloned()
    }

    /// The databases the opening tree can use, the open game's database first.
    pub(super) fn explorer_choices(&self, cx: &gpui::App) -> Vec<DbEntry> {
        let mut entries = self.model.read(cx).databases.clone();
        if let Some(game) = &self.db_game
            && let Some(index) = entries.iter().position(|entry| entry.path == game.path)
        {
            let entry = entries.remove(index);
            entries.insert(0, entry);
        }
        entries
    }

    /// The database this tab chose for its opening tree, for saving the session.
    pub fn explorer_choice(&self) -> Option<&std::path::Path> {
        self.explorer_db.as_deref()
    }

    /// Restores a saved choice for the opening tree, ignoring a database that no
    /// longer exists.
    pub fn restore_explorer_choice(
        &mut self,
        path: Option<std::path::PathBuf>,
        cx: &mut Context<Self>,
    ) {
        self.explorer_db = path.filter(|path| {
            self.model
                .read(cx)
                .databases
                .iter()
                .any(|entry| &entry.path == path)
        });
        self.query_databases(cx);
    }

    /// Makes the opening tree count `path`, remembering it for this tab and as the
    /// default for tabs that have not chosen.
    pub(super) fn choose_explorer_db(&mut self, path: std::path::PathBuf, cx: &mut Context<Self>) {
        self.explorer_menu = None;
        self.explorer_db = Some(path.clone());
        self.model.update(cx, |model, cx| {
            if model.config.explorer_default.as_ref() != Some(&path) {
                model.config.explorer_default = Some(path);
                model.save_config(cx);
            }
        });
        self.query_databases(cx);
        cx.notify();
    }

    /// Steps the opening tree to the next database in the picker's order.
    pub fn next_explorer_db(&mut self, cx: &mut Context<Self>) {
        let choices = self.explorer_choices(cx);
        if choices.is_empty() {
            return;
        }
        let current = self.explorer_database(cx).map(|entry| entry.path);
        let index = choices
            .iter()
            .position(|entry| Some(&entry.path) == current.as_ref())
            .map_or(0, |index| (index + 1) % choices.len());
        let path = choices[index].path.clone();
        self.choose_explorer_db(path, cx);
    }

    /// A database was added, changed or removed: cached counts are dropped, and a
    /// removed opening-tree database falls back with a note saying which one is used.
    pub(super) fn databases_changed(&mut self, cx: &mut Context<Self>) {
        self.move_cache.clear();
        let known: Vec<std::path::PathBuf> = self
            .model
            .read(cx)
            .databases
            .iter()
            .map(|entry| entry.path.clone())
            .collect();
        if self
            .explorer_db
            .as_ref()
            .is_some_and(|path| !known.contains(path))
        {
            self.explorer_db = None;
        }
        let removed = self
            .explorer_used
            .as_ref()
            .is_some_and(|path| !known.contains(path));
        if removed && let Some(entry) = self.explorer_database(cx) {
            let message = ui::i18n::tf("Opening tree now uses {}", &[&entry.name]);
            self.model.toast(message, cx);
        }
        self.query_databases(cx);
    }

    pub(super) fn query_databases(&mut self, cx: &mut Context<Self>) {
        self.reference_error = None;
        let hash = position_hash(&self.analysed_pos());
        let reference = self.explorer_database(cx).map(|entry| entry.path);
        self.explorer_used = reference.clone();
        let model = self.model.read(cx);
        let mine = model.my_games().map(|entry| entry.path.clone());
        // All the user's names, one per line, as the cache key and the query's names.
        let player = model.config.my_names().join("\n");
        // Answers already known come from the cache; only the rest are queried.
        let cached = |path: &Option<std::path::PathBuf>, player: &str| match path {
            None => Some(Vec::new()),
            Some(path) => self.move_cache.get(path, hash, player).cloned(),
        };
        let (cached_reference, cached_mine) = (cached(&reference, ""), cached(&mine, &player));
        if let (Some(reference), Some(mine)) = (&cached_reference, &cached_mine) {
            self.db_task = None;
            self.reference = reference.clone();
            self.mine = mine.clone();
            self.rebuild_rows(cx);
            return;
        }
        let (query_reference, query_mine) = (
            reference.clone().filter(|_| cached_reference.is_none()),
            mine.clone().filter(|_| cached_mine.is_none()),
        );
        self.reference = cached_reference.clone().unwrap_or_default();
        self.mine = cached_mine.clone().unwrap_or_default();
        self.rebuild_rows(cx);
        let query_player = player.clone();
        let task = cx.background_spawn(async move {
            let query = |path: Option<std::path::PathBuf>, player: &str| {
                let path = path?;
                Some(
                    Database::open(path)
                        .and_then(|db| {
                            let names: Vec<String> = player
                                .split('\n')
                                .filter(|name| !name.is_empty())
                                .map(str::to_owned)
                                .collect();
                            if names.is_empty() {
                                db.moves_from(hash)
                            } else {
                                db.moves_from_players(hash, &names)
                            }
                        })
                        .map_err(|err| err.to_string()),
                )
            };
            (query(query_reference, ""), query(query_mine, &query_player))
        });
        // Dropping the previous task cancels a stale query.
        self.db_task = Some(cx.spawn(async move |this, cx| {
            let (found_reference, found_mine) = task.await;
            let _ = this.update(cx, |this, cx| {
                this.db_task = None;
                let found_reference = found_reference.and_then(|result| match result {
                    Ok(moves) => Some(moves),
                    Err(err) => {
                        this.reference_error = Some(err);
                        None
                    }
                });
                let found_mine = found_mine.and_then(|result| result.ok());
                if let (Some(path), Some(moves)) = (&reference, &found_reference) {
                    this.move_cache
                        .insert(path.clone(), hash, String::new(), moves.clone());
                }
                if let (Some(path), Some(moves)) = (&mine, &found_mine) {
                    this.move_cache
                        .insert(path.clone(), hash, player.clone(), moves.clone());
                }
                this.reference = found_reference.or(cached_reference).unwrap_or_default();
                this.mine = found_mine.or(cached_mine).unwrap_or_default();
                this.rebuild_rows(cx);
            });
        }));
    }

    /// Opens or closes the opening tree's database list, keeping keyboard focus on
    /// the picker so the arrow keys move through it.
    pub(super) fn toggle_explorer_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.explorer_menu = match self.explorer_menu {
            Some(_) => None,
            None => Some(self.explorer_current_index(cx)),
        };
        self.explorer_focus.focus(window, cx);
        cx.notify();
    }

    /// Where the database in use sits in the picker's list.
    pub(super) fn explorer_current_index(&self, cx: &gpui::App) -> usize {
        let current = self.explorer_database(cx).map(|entry| entry.path);
        self.explorer_choices(cx)
            .iter()
            .position(|entry| Some(&entry.path) == current.as_ref())
            .unwrap_or(0)
    }

    /// Keys on the focused picker: Enter, Space or Down open it; the arrows move,
    /// Enter picks and Escape closes. Returns whether the key was used.
    pub(super) fn explorer_key(&mut self, key: &str, cx: &mut Context<Self>) -> bool {
        let count = self.explorer_choices(cx).len();
        match (self.explorer_menu, key) {
            (None, "enter" | "space" | "down") if count > 0 => {
                self.explorer_menu = Some(self.explorer_current_index(cx));
            }
            (Some(index), "down") => self.explorer_menu = Some((index + 1) % count.max(1)),
            (Some(index), "up") => {
                self.explorer_menu = Some((index + count.max(1) - 1) % count.max(1))
            }
            (Some(index), "enter" | "space") => {
                if let Some(entry) = self.explorer_choices(cx).get(index) {
                    let path = entry.path.clone();
                    self.choose_explorer_db(path, cx);
                }
            }
            (Some(_), "escape" | "tab") => {
                self.explorer_menu = None;
                cx.notify();
                return key == "escape";
            }
            _ => return false,
        }
        cx.notify();
        true
    }

    /// The Reference column's heading: the opening tree's database, which opens a
    /// list of every database to count instead. It keeps the column's width.
    pub(super) fn render_explorer_picker(
        &self,
        width: f32,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let this = cx.entity();
        let name = self.explorer_database(cx).map_or_else(
            || ui::i18n::t("Reference").to_owned(),
            |entry| explorer_label(&entry.name),
        );
        let open = self.explorer_menu;
        let (click, keys) = (this.clone(), this.clone());
        let button = h_flex()
            .id("explorer-picker")
            .key_context("FastButton")
            .track_focus(&self.explorer_focus)
            .w_full()
            .h(px(24.))
            .pl(px(4.))
            .pr(px(2.))
            .gap(px(4.))
            .rounded(px(4.))
            .border_1()
            .border_color(gpui::transparent_black())
            .cursor_pointer()
            .hover(|style| style.bg(rgb(T.hover)))
            .focus_visible(|style| style.border_2().border_color(rgb(T.accent)))
            .when(open.is_none(), |button| {
                button.tooltip(|window, cx| {
                    gpui_component::tooltip::Tooltip::new(ui::i18n::t(
                        "Choose the database for the opening tree",
                    ))
                    .build(window, cx)
                })
            })
            .on_click(move |event, window, cx| {
                // Enter and Space are handled with the arrow keys in `explorer_key`.
                if !event.is_keyboard() {
                    click.update(cx, |screen, cx| screen.toggle_explorer_menu(window, cx));
                }
            })
            .on_key_down(move |event: &gpui::KeyDownEvent, _, cx| {
                if event.keystroke.modifiers.modified() {
                    return;
                }
                let key = event.keystroke.key.clone();
                if keys.update(cx, |screen, cx| screen.explorer_key(&key, cx)) {
                    cx.stop_propagation();
                }
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(11.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .font_family(ui::SANS)
                    .text_color(rgb(if open.is_some() { T.fg } else { T.muted }))
                    .child(name.to_uppercase()),
            )
            .child(ui::widgets::icon(
                "icons/chevron-down.svg",
                px(12.),
                T.muted,
            ));
        let menu = open.map(|highlight| {
            let game_db = self.db_game.as_ref().map(|game| game.path.clone());
            let mut list = v_flex()
                .id("explorer-menu")
                .w(px(260.))
                .py(px(4.))
                .rounded(px(6.))
                .bg(rgb(T.surface))
                .border_1()
                .border_color(rgb(T.border_strong))
                .on_mouse_down_out({
                    let this = this.clone();
                    move |_, _, cx| {
                        this.update(cx, |screen, cx| {
                            screen.explorer_menu = None;
                            cx.notify();
                        })
                    }
                });
            for (index, entry) in self.explorer_choices(cx).into_iter().enumerate() {
                let this = this.clone();
                let highlighted = highlight == index;
                let is_game_db = game_db.as_ref() == Some(&entry.path);
                let path = entry.path.clone();
                list = list.child(
                    h_flex()
                        .id(("explorer-choice", index))
                        .min_h(px(30.))
                        .px(px(10.))
                        .gap(px(8.))
                        .justify_between()
                        .cursor_pointer()
                        .font_family(ui::SANS)
                        .text_size(px(13.))
                        .text_color(rgb(T.fg))
                        .when(highlighted, |row| row.bg(rgb(T.selected)))
                        .when(!highlighted, |row| {
                            row.hover(|style| style.bg(rgb(T.hover)))
                        })
                        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                            cx.stop_propagation();
                            let path = path.clone();
                            this.update(cx, |screen, cx| screen.choose_explorer_db(path, cx));
                        })
                        .child(
                            v_flex()
                                .min_w_0()
                                .py(px(4.))
                                .child(div().truncate().child(entry.name.clone()))
                                .when(is_game_db, |cell| {
                                    cell.child(
                                        div()
                                            .text_size(px(11.))
                                            .text_color(rgb(T.muted))
                                            .child(ui::i18n::t("This game's database")),
                                    )
                                }),
                        )
                        .child(
                            mono(crate::format::format_count(entry.games))
                                .flex_none()
                                .text_size(px(12.))
                                .text_color(rgb(T.muted)),
                        ),
                );
            }
            gpui::deferred(gpui::anchored().child(div().mt(px(2.)).occlude().child(list)))
                .with_priority(1)
        });
        div()
            .w(px(width))
            .flex_none()
            .child(button)
            .children(menu)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Totals preserve every outcome and leave unfinished games out of draws.
    #[test]
    fn totals_and_result_shares_include_unfinished_games() {
        let total = total_stats(&[
            MoveStat {
                san: "e4".into(),
                games: 10,
                white_wins: 4,
                draws: 3,
                black_wins: 2,
            },
            MoveStat {
                san: "d4".into(),
                games: 10,
                white_wins: 2,
                draws: 3,
                black_wins: 4,
            },
        ]);
        assert_eq!(total.games, 20);
        assert_eq!(result_shares(&total), [0.3, 0.3, 0.3, 0.1]);
        assert_eq!(result_shares(&total_stats(&[])), [0.; 4]);
    }
}
