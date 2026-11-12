//! The game table, its header and the summary above it.

use super::*;

impl DatabaseScreen {
    /// Separates game searching from less frequent review, export and collection actions.
    pub(super) fn render_main_header(
        &mut self,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        use gpui_component::{button::Button, menu::DropdownMenu as _};
        let this = cx.entity();
        let filtered = self.active_filter_count(cx);
        let selected = self.selected_db.clone();
        let renamable = selected
            .as_ref()
            .is_some_and(|path| self.can_rename(path, cx));
        let reimport = self.db_snapshot.as_ref().and_then(|entry| {
            entry
                .source_file
                .clone()
                .map(|file| (entry.path.clone(), file))
        });
        let review_running = self.reviewing.is_some();
        let review_disabled = !review_running
            && (selected.is_none() || self.review_selected_only && self.selected.is_none());
        let accounts: Vec<_> = self
            .model
            .read(cx)
            .config
            .accounts
            .iter()
            .enumerate()
            .filter(|(_, account)| Some(&account.database) == selected.as_ref())
            .map(|(index, account)| (index, account.site, account.name.clone()))
            .collect();
        h_flex()
            .flex_wrap()
            .justify_between()
            .gap(px(8.))
            .min_h(px(28.))
            .child(
                h_flex()
                    .gap(px(10.))
                    .child(
                        mono(ui::i18n::tf(
                            "{} matching games",
                            &[&format_count(self.stats.games)],
                        ))
                        .text_size(px(14.))
                        .font_weight(FontWeight::SEMIBOLD),
                    )
                    .child({
                        btn(
                            "toggle-filters",
                            if filtered > 0 {
                                ui::i18n::tf("Filters ({})", &[&filtered.to_string()])
                            } else {
                                ui::i18n::t("Filters").to_owned()
                            },
                        )
                        .compact()
                        .selected(self.filters_open)
                        .on_click(this.listen(move |screen, _, cx| {
                            screen.filters_open = !screen.filters_open;
                            cx.notify();
                        }))
                    })
                    .when(filtered > 0, |row| {
                        row.child({
                            let this = this.clone();
                            btn("clear-filters", "Clear filters").compact().on_click(
                                move |window, cx| {
                                    this.update(cx, |screen, cx| screen.clear_filters(window, cx));
                                },
                            )
                        })
                    }),
            )
            .child(
                h_flex()
                    .gap(px(6.))
                    .when_some(self.reviewing.as_ref(), |row, reviewing| {
                        row.when(reviewing.engines == 1, |row| {
                            row.child(
                                mono(ui::i18n::tf(
                                    "Position {} / {}",
                                    &[&reviewing.ply.to_string(), &reviewing.plies.to_string()],
                                ))
                                .text_size(px(12.)),
                            )
                        })
                        .child(
                            mono(ui::i18n::tf(
                                "Review {} / {}",
                                &[
                                    &reviewing
                                        .finished
                                        .load(std::sync::atomic::Ordering::Relaxed)
                                        .min(reviewing.total)
                                        .to_string(),
                                    &reviewing.total.to_string(),
                                ],
                            ))
                            .text_size(px(12.)),
                        )
                        .child(
                            btn(
                                "review-pause",
                                if reviewing.paused.load(std::sync::atomic::Ordering::Relaxed) {
                                    "Resume"
                                } else {
                                    "Pause"
                                },
                            )
                            .compact()
                            .on_click(
                                this.listen(move |screen, _, cx| screen.toggle_review_pause(cx)),
                            ),
                        )
                    })
                    .child({
                        btn("import-games", "Import games").compact().on_click(
                            this.listen(move |screen, window, cx| screen.prompt_import(window, cx)),
                        )
                    })
                    .child(
                        Button::new("database-more-actions")
                            .label(ui::i18n::t("More actions"))
                            .compact()
                            .h(px(28.))
                            .dropdown_menu(move |menu, _, _| {
                                let settings = this.clone();
                                let review = this.clone();
                                let rename = this.clone();
                                let duplicate = this.clone();
                                let rename_path = selected.clone();
                                let duplicate_path = selected.clone();
                                let mut menu = menu
                                    .item(
                                        PopupMenuItem::new(ui::i18n::t("Review settings"))
                                            .on_click(settings.listen_event(
                                                move |screen, _, _, cx| {
                                                    screen.set_review_settings_open(true, cx)
                                                },
                                            )),
                                    )
                                    .item(
                                        PopupMenuItem::new(ui::i18n::t(if review_running {
                                            "Stop review"
                                        } else {
                                            "Analyse all"
                                        }))
                                        .disabled(review_disabled)
                                        .on_click(
                                            move |_, _, cx| {
                                                review.update(cx, |screen, cx| {
                                                    screen.analyse_all(cx)
                                                });
                                            },
                                        ),
                                    )
                                    .item(
                                        PopupMenuItem::new(ui::i18n::t("Export matching games"))
                                            .disabled(selected.is_none())
                                            .on_click(this.listen_event(
                                                move |screen, _, _, cx| screen.export_pgn(cx),
                                            )),
                                    )
                                    .item(
                                        PopupMenuItem::new(ui::i18n::t("Rename database"))
                                            .disabled(!renamable)
                                            .on_click(move |_, window, cx| {
                                                if let Some(path) = &rename_path {
                                                    rename.update(cx, |screen, cx| {
                                                        screen.start_rename(
                                                            path.clone(),
                                                            window,
                                                            cx,
                                                        )
                                                    });
                                                }
                                            }),
                                    )
                                    .item(
                                        PopupMenuItem::new(ui::i18n::t("Duplicate database"))
                                            .disabled(selected.is_none())
                                            .on_click(move |_, _, cx| {
                                                if let Some(path) = &duplicate_path {
                                                    duplicate.update(cx, |screen, cx| {
                                                        screen.duplicate_db(path.clone(), cx)
                                                    });
                                                }
                                            }),
                                    );
                                if let Some(target) = &reimport {
                                    let target = target.clone();
                                    menu = menu.item(
                                        PopupMenuItem::new(ui::i18n::t("Re-import from file…"))
                                            .on_click(this.listen_event(
                                                move |screen, _, _, cx| {
                                                    screen.confirm_reimport = Some(target.clone());
                                                    cx.notify();
                                                },
                                            )),
                                    );
                                }
                                for (index, site, name) in &accounts {
                                    let index = *index;
                                    menu = menu.item(
                                        PopupMenuItem::new(ui::i18n::tf(
                                            "Update {} from {}",
                                            &[name, site.name()],
                                        ))
                                        .on_click(
                                            this.listen_event(move |screen, _, _, cx| {
                                                screen.update_account(index, cx)
                                            }),
                                        ),
                                    );
                                }
                                menu
                            }),
                    ),
            )
    }

    /// Keeps the score and most common opening leak on a compact summary line.
    pub(super) fn render_stats(&self) -> impl IntoElement + use<> {
        h_flex()
            .flex_wrap()
            .flex_none()
            .gap(px(16.))
            .text_size(px(13.))
            .child(
                h_flex()
                    .gap(px(8.))
                    .child(section_label("Score"))
                    .child(mono(if self.stats.games == 0 {
                        "—".to_owned()
                    } else {
                        format!("{:.1}%", self.stats.score_percent)
                    })),
            )
            .child(
                h_flex()
                    .min_w_0()
                    .gap(px(8.))
                    .child(section_label("Most common leak"))
                    .child(
                        mono(self.leak_text.clone().unwrap_or_else(|| "—".to_owned()))
                            .text_color(rgb(T.warn)),
                    ),
            )
    }

    /// A column heading that sorts the table by its column. The sorted one is `fg`
    /// with an arrow in `accent`; the arrow's space is kept on every heading so
    /// widths never change.
    pub(super) fn sort_heading(
        &self,
        key: SortKey,
        label: &str,
        width: Option<f32>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let this = cx.entity();
        let sorted = (self.sort.key == key).then_some(self.sort.descending);
        let heading =
            ui::widgets::sort_heading(("sort", key as usize), label, sorted, move |_, cx| {
                this.update(cx, |screen, cx| screen.sort_by(key, cx));
            });
        let cell = div().flex_none().pr(px(10.));
        match width {
            Some(width) => cell.w(px(width)).child(heading),
            None => cell.flex_1().min_w_0().child(heading),
        }
    }

    pub(super) fn render_table(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let busy = self.querying.then(|| {
            div()
                .absolute()
                .left_0()
                .right_0()
                .bottom(px(-1.))
                .child(ui::widgets::busy_bar("games-query"))
        });
        let header = h_flex()
            .relative()
            .h(px(35.))
            .flex_none()
            .px(px(12.))
            .bg(rgb(T.surface_2))
            .border_b_1()
            .border_color(rgb(T.border))
            .children(busy)
            .child(self.sort_heading(SortKey::FileOrder, "#", Some(W_NUMBER), cx))
            .child(self.sort_heading(SortKey::White, "White", None, cx))
            .child(self.sort_heading(SortKey::Black, "Black", None, cx))
            .child(self.sort_heading(SortKey::Result, "Result", Some(W_RESULT), cx))
            .child(
                div()
                    .w(px(W_LEFT))
                    .flex_none()
                    .child(section_label("Left rep.")),
            )
            .child(self.sort_heading(SortKey::Moves, "Moves", Some(W_MOVES), cx))
            .child(self.sort_heading(SortKey::Date, "Date", Some(W_DATE), cx))
            .child(self.sort_heading(SortKey::Source, "Source", Some(W_SOURCE), cx))
            .child(self.sort_heading(SortKey::Accuracy, "Acc.", Some(W_ACC), cx));
        let count = self.ids.len();
        let list = uniform_list(
            "games-table",
            count,
            cx.processor(|this, range: Range<usize>, _window, cx| {
                this.ensure_rows(range.clone(), cx);
                range
                    .map(|index| this.render_row(index, cx).into_any_element())
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(&self.scroll)
        .size_full();
        let list = div()
            .relative()
            .flex_1()
            .min_h_0()
            .child(list)
            .vertical_scrollbar(&self.scroll);
        let rows_height = (count as f32 * ROW_H).min(450.) + 35.;
        panel()
            .flex()
            .flex_col()
            .max_h(px(rows_height + 2.))
            .h_full()
            .overflow_hidden()
            .child(header)
            .child(list)
    }

    pub(super) fn render_row(
        &mut self,
        index: usize,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let id = self.ids[index];
        let selected = self.selected == Some(id);
        let this = cx.entity();
        let row_el = h_flex()
            .id(("game", index))
            .relative()
            .w_full()
            .h(px(ROW_H))
            .px(px(12.))
            .cursor_pointer()
            .border_b_1()
            .border_color(rgb(T.divider))
            .text_size(px(13.))
            .when(selected, |row| row.bg(rgb(T.selected)))
            .when(!selected, |row| row.hover(|style| style.bg(rgb(T.hover))))
            .on_click({
                let this = this.clone();
                move |event, window, cx| {
                    this.update(cx, |screen, cx| {
                        if event.click_count() == 2 {
                            screen.open_game_in_analyse(id, window, cx);
                        } else {
                            screen.select_game(Some(id), cx);
                        }
                    });
                }
            })
            .context_menu({
                let shown = self.ids.len();
                move |menu, _, _| {
                    let (open, edit, delete, delete_all) =
                        (this.clone(), this.clone(), this.clone(), this.clone());
                    menu.item(PopupMenuItem::new(ui::i18n::t("Open in Analyse")).on_click(
                        move |_, window, cx| {
                            open.update(cx, |screen, cx| {
                                screen.open_game_in_analyse(id, window, cx)
                            })
                        },
                    ))
                    .item(PopupMenuItem::new(ui::i18n::t("Edit game…")).on_click(
                        move |_, window, cx| {
                            edit.update(cx, |screen, cx| screen.open_editor(id, window, cx))
                        },
                    ))
                    .separator()
                    .item(PopupMenuItem::new(ui::i18n::t("Delete game…")).on_click(
                        move |_, _, cx| {
                            delete.update(cx, |screen, cx| screen.ask_delete(vec![id], cx))
                        },
                    ))
                    .when(shown > 1, |menu| {
                        menu.item(
                            PopupMenuItem::new(format!(
                                "Delete all {} shown games…",
                                format_count(shown as u64)
                            ))
                            .on_click(delete_all.listen_event(
                                move |screen, _, _, cx| {
                                    let ids = screen.ids.clone();
                                    screen.ask_delete(ids, cx)
                                },
                            )),
                        )
                    })
                }
            })
            .when(selected, |row| {
                row.child(
                    div()
                        .absolute()
                        .left_0()
                        .top_0()
                        .bottom_0()
                        .w(px(3.))
                        .bg(rgb(T.accent)),
                )
            });
        let Some(game) = self.rows.get(&id) else {
            return row_el.child(div().text_color(rgb(T.faint)).child("…"));
        };
        let name = |name: &str, elo: Option<i64>, winner: bool, annotated: bool| {
            h_flex()
                .flex_1()
                .min_w_0()
                .pr(px(10.))
                .gap(px(5.))
                .overflow_hidden()
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_color(rgb(T.fg))
                        .when(winner, |text| text.font_weight(FontWeight::BOLD))
                        .child(SharedString::from(name.to_owned())),
                )
                .children(elo.map(|elo| mono(elo.to_string()).flex_none().text_as(14., T.muted)))
                .when(annotated, |cell| {
                    cell.child(
                        div()
                            .id(("annotated", id as usize))
                            .flex_none()
                            .child(ui::widgets::icon(
                                "icons/message-square.svg",
                                px(12.),
                                T.muted,
                            ))
                            .tooltip(|window, cx| {
                                gpui_component::tooltip::Tooltip::new(ui::i18n::t(
                                    "Has comments or variations",
                                ))
                                .build(window, cx)
                            }),
                    )
                })
        };
        let white_won = game.result == "1-0";
        let black_won = game.result == "0-1";
        let mono_cell = |text: String, width: f32, color: u32| {
            mono(text)
                .w(px(width))
                .flex_none()
                .text_size(px(13.))
                .text_color(rgb(color))
        };
        row_el
            .child(
                mono(game.id.to_string())
                    .w(px(W_NUMBER))
                    .flex_none()
                    .pr(px(14.))
                    .flex()
                    .justify_end()
                    .text_as(12., T.faint),
            )
            .child(name(&game.white, game.white_elo, white_won, game.annotated))
            .child(name(&game.black, game.black_elo, black_won, false))
            .child(mono_cell(result_text(&game.result), W_RESULT, T.fg))
            .child(match &game.left_rep {
                Some(left) => mono_cell(left.clone(), W_LEFT, T.warn),
                None => mono_cell("—".to_owned(), W_LEFT, T.muted),
            })
            .child(mono_cell(game.moves_count().to_string(), W_MOVES, T.fg_2))
            .child(mono_cell(game.date.clone(), W_DATE, T.muted))
            .child(
                div()
                    .w(px(W_SOURCE))
                    .flex_none()
                    .text_color(rgb(T.fg_2))
                    .child(SharedString::from(game.source.clone())),
            )
            .child(
                mono(
                    game.accuracy
                        .map(|accuracy| format!("{accuracy:.1}"))
                        .unwrap_or_else(|| "—".to_owned()),
                )
                .w(px(W_ACC))
                .flex_none()
                .text_as(13., T.fg),
            )
    }
}
