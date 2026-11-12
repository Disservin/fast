//! The left column: databases, the position pattern filter and the filters.

use super::*;
use gpui::Div;

impl DatabaseScreen {
    /// Keeps game-search controls together beside the results table.
    pub(super) fn render_left(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let this = cx.entity();
        let reached = self.reached;
        let hidden = self.hidden_filters_set(cx);
        let saved_list = self.render_saved_searches(cx);
        v_flex()
            .id("database-left")
            .w(px(281.))
            .h_full()
            .flex_none()
            .overflow_y_scroll()
            .px(px(16.))
            .pt(px(16.))
            .pb(px(16.))
            .bg(rgb(T.surface))
            .border_r_as(T.border)
            .child(
                h_flex()
                    .justify_between()
                    .items_center()
                    .mb(px(6.))
                    .child(section_label("Filter"))
                    .child(
                        btn("save-search", "Save search")
                            .mini()
                            .tooltip("Save these filters under a name")
                            .on_click(this.listen(move |screen, window, cx| {
                                screen.start_saving_search(window, cx)
                            })),
                    ),
            )
            .when_some(self.saving_search.clone(), |column, input| {
                column.child(div().mb(px(6.)).child(text_input(&input).height(28.)))
            })
            .children(saved_list)
            .child({
                let (keys_this, suggestions) = (this.clone(), self.render_player_suggestions(cx));
                let open = suggestions.is_some();
                div()
                    .mb(px(7.))
                    // Arrow keys move through the suggestions instead of the text cursor.
                    .capture_key_down(move |event: &gpui::KeyDownEvent, _, cx| {
                        let key = event.keystroke.key.as_str();
                        if open && (key == "down" || key == "up") {
                            cx.stop_propagation();
                            keys_this
                                .update(cx, |screen, cx| screen.step_suggestion(key == "down", cx));
                        }
                    })
                    .child(field("Player", text_input(&self.player)))
                    .children(suggestions)
            })
            .child(
                div()
                    .grid()
                    .grid_cols(2)
                    .gap_x(px(10.))
                    .mb(px(7.))
                    .child(field("Colour", select_el(&self.colour)))
                    .child(field("Result", select_el(&self.result))),
            )
            .child(
                div()
                    .mb(px(10.))
                    .child(field("Date", self.render_dates(cx))),
            )
            .child(
                div().mb(px(8.)).child(
                    check("reached", reached, "Reached the Analyse board position")
                        .text(13.)
                        .on_change(this.listen_with(move |screen, value, _, cx| {
                            screen.toggle_reached(value, cx)
                        })),
                ),
            )
            .child(
                h_flex()
                    .gap(px(6.))
                    .mb(px(6.))
                    .ml(px(-10.))
                    .child(
                        btn("more-filters", "More filters")
                            .kind(ui::widgets::BtnKind::Ghost)
                            .compact()
                            .icon(if self.more_filters {
                                "icons/chevron-down.svg"
                            } else {
                                "icons/chevron-right.svg"
                            })
                            .on_click(this.listen(move |screen, _, cx| {
                                screen.more_filters = !screen.more_filters;
                                cx.notify();
                            })),
                    )
                    .when(hidden > 0, |row| row.child(badge(hidden.to_string()))),
            )
            .when(self.more_filters, |column| {
                column
                    .child(self.render_more_filters(cx))
                    .child(self.render_pattern_filter(cx))
            })
    }

    /// The date range, with a hint when either end is not a date.
    fn render_dates(&self, cx: &mut Context<Self>) -> Div {
        let date_text = |input: &Entity<InputState>| input.read(cx).value().trim().to_owned();
        let (from_text, to_text) = (date_text(&self.date_from), date_text(&self.date_to));
        let from_bad = !from_text.is_empty() && date_bound(&from_text, false).is_none();
        let to_bad = !to_text.is_empty() && date_bound(&to_text, true).is_none();
        v_flex()
            .gap(px(4.))
            .child(
                h_flex()
                    .gap(px(6.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(text_input(&self.date_from).invalid(from_bad)),
                    )
                    .child(div().text_color(rgb(T.muted)).child("–"))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(text_input(&self.date_to).invalid(to_bad)),
                    ),
            )
            .when(from_bad || to_bad, |dates| {
                dates.child(
                    div()
                        .text_as(12., T.warn)
                        .child(ui::i18n::t("Use 2020, 2020.05 or 2020.05.17")),
                )
            })
    }

    /// The saved searches, one button each; right-click deletes one.
    fn render_saved_searches(&self, cx: &mut Context<Self>) -> Option<Div> {
        let this = cx.entity();
        let saved: Vec<String> = self
            .model
            .read(cx)
            .config
            .saved_filters
            .iter()
            .map(|saved| saved.name.clone())
            .collect();
        let mut saved_list = v_flex().mb(px(6.));
        for (index, name) in saved.iter().enumerate() {
            let (apply, delete) = (this.clone(), self.model.clone());
            saved_list = saved_list.child(
                div()
                    .id(("saved-search-row", index))
                    .child(
                        btn(("saved-search", index), name.clone())
                            .kind(ui::widgets::BtnKind::Ghost)
                            .compact()
                            .full()
                            .left()
                            .icon("icons/search.svg")
                            .tooltip("Right-click to delete")
                            .on_click(apply.listen(move |screen, window, cx| {
                                screen.apply_saved_search(index, window, cx)
                            })),
                    )
                    .context_menu(move |menu, _, _| {
                        let delete = delete.clone();
                        menu.item(
                            PopupMenuItem::new(ui::i18n::t("Delete saved search")).on_click(
                                delete.listen_event(move |model, _, _, cx| {
                                    if index < model.config.saved_filters.len() {
                                        model.config.saved_filters.remove(index);
                                        model.save_config(cx);
                                    }
                                }),
                            ),
                        )
                    }),
            );
        }
        (!saved.is_empty()).then_some(saved_list)
    }

    /// The filters under More filters: ECO, time control, ratings, moves, event and the
    /// annotated and repertoire checks.
    fn render_more_filters(&self, cx: &mut Context<Self>) -> Div {
        let this = cx.entity();
        let (only_annotated, only_left) = (self.only_annotated, self.only_left_rep);
        // Number pairs as one line: min – max.
        let pair = |low: &Entity<InputState>, high: &Entity<InputState>| {
            h_flex()
                .gap(px(6.))
                .child(div().flex_1().min_w_0().child(text_input(low)))
                .child(div().text_color(rgb(T.muted)).child("–"))
                .child(div().flex_1().min_w_0().child(text_input(high)))
        };
        v_flex()
            .gap(px(7.))
            .mb(px(10.))
            .child(
                div()
                    .grid()
                    .grid_cols(2)
                    .gap_x(px(10.))
                    .gap_y(px(7.))
                    .child(field("ECO", text_input(&self.eco)))
                    .child(field("Time control", select_el(&self.time_control)))
                    .child(field("Opp. rating ≥", text_input(&self.opp_rating)))
                    .child(field("Rating", pair(&self.min_rating, &self.max_rating))),
            )
            .child(field("Moves", pair(&self.min_moves, &self.max_moves)))
            .child(field("Event or site", text_input(&self.event)))
            .child(
                check("only-annotated", only_annotated, "Only annotated games").on_change(
                    this.listen_with(move |screen, value, _, cx| {
                        screen.only_annotated = value;
                        screen.schedule_filter(cx);
                    }),
                ),
            )
            .child(
                check(
                    "left-rep",
                    only_left,
                    "Only games where I left my repertoire",
                )
                .on_change(this.listen_with(move |screen, value, _, cx| {
                    screen.only_left_rep = value;
                    screen.schedule_filter(cx);
                })),
            )
    }

    /// Runs a cancellable index build on a worker, leaving queries on the complete old index.
    pub(super) fn build_pattern_index(&mut self, cx: &mut Context<Self>) {
        if self.pattern_control.is_some() || self.importing.is_some() || self.downloading.is_some()
        {
            return;
        }
        let Some(path) = self.selected_db.clone() else {
            return;
        };
        let control = ImportControl::default();
        self.pattern_control = Some(control.clone());
        self.pattern_progress = (0, 0);
        let (send, receive) = smol::channel::unbounded();
        let worker = cx.background_spawn(async move {
            Database::open(&path)?.build_structure_index(&control, |done, total| {
                let _ = send.try_send((done, total));
            })
        });
        self.pattern_task = Some(cx.spawn(async move |this, cx| {
            while let Ok(progress) = receive.recv().await {
                let _ = this.update(cx, |screen, cx| {
                    screen.pattern_progress = progress;
                    cx.notify();
                });
            }
            let result = worker.await;
            let _ = this.update(cx, |screen, cx| {
                screen.pattern_control = None;
                match result {
                    Ok(true) => screen.model.toast("Structure index ready", cx),
                    Ok(false) => screen.model.toast("Structure indexing cancelled", cx),
                    Err(err) => screen
                        .model
                        .toast(format!("Could not build structure index: {err}"), cx),
                }
                screen.apply_filter(cx);
            });
        }));
        cx.notify();
    }

    /// Exact pattern controls use a reference FEN and the optional database index.
    pub(super) fn render_pattern_filter(&self, cx: &mut Context<Self>) -> AnyElement {
        let this = cx.entity();
        let build = this.clone();
        let cancel = this.clone();
        let valid = GameTree::from_fen(self.pattern_fen.read(cx).value().trim()).is_ok();
        let mut body = v_flex()
            .gap(px(8.))
            .mt(px(12.))
            .child(section_label("Position patterns"))
            .child(text_input(&self.pattern_fen).mono())
            .when(
                !valid && !self.pattern_fen.read(cx).value().is_empty(),
                |body| {
                    body.child(
                        div()
                            .text_as(13., T.warn)
                            .child(ui::i18n::t("Enter a valid reference FEN")),
                    )
                },
            )
            .child({
                let this = this.clone();
                check("pattern-pawns", self.pattern_pawns, "Same pawn structure").on_change(
                    this.listen_with(move |screen, value, _, cx| {
                        screen.pattern_pawns = value;
                        screen.schedule_filter(cx);
                    }),
                )
            })
            .child({
                let this = this.clone();
                check("pattern-material", self.pattern_material, "Same material").on_change(
                    this.listen_with(move |screen, value, _, cx| {
                        screen.pattern_material = value;
                        screen.schedule_filter(cx);
                    }),
                )
            })
            .child(
                div()
                    .text_as(13., T.muted)
                    .child(ui::i18n::t("Exact matches, either side to move")),
            );
        if self.pattern_control.is_some() {
            let (done, total) = self.pattern_progress;
            body = body
                .child(
                    h_flex()
                        .gap(px(8.))
                        .child(mono(format!("{done} / {total}")))
                        .child(btn("cancel-structure-index", "Cancel").compact().on_click(
                            cancel.listen(move |screen, _, cx| {
                                if let Some(control) = &screen.pattern_control {
                                    control.cancel();
                                }
                                cx.notify();
                            }),
                        )),
                )
                .child(
                    div().h(px(2.)).w_full().bg(rgb(T.divider)).child(
                        div()
                            .h_full()
                            .w(gpui::relative(if total == 0 {
                                0.
                            } else {
                                done as f32 / total as f32
                            }))
                            .bg(rgb(T.accent)),
                    ),
                );
        } else if !self.pattern_indexed {
            body = body
                .child(
                    div()
                        .text_as(13., T.warn)
                        .child(ui::i18n::t("Build the structure index to search")),
                )
                .child(
                    btn("build-structure-index", "Build structure index")
                        .compact()
                        .on_click(
                            build.listen(move |screen, _, cx| screen.build_pattern_index(cx)),
                        ),
                );
        }
        body.into_any_element()
    }

    pub(super) fn toggle_reached(&mut self, value: bool, cx: &mut Context<Self>) {
        self.reached = value;
        if value && let Some(workspace) = self.workspace.upgrade() {
            // Capture whatever is on the Analyse board right now.
            let analyse = workspace.read(cx).analyse.read(cx);
            let node = analyse.tree.node(analyse.current);
            let path = analyse.tree.path_to(analyse.current);
            let label = classify(path.iter().map(|&id| analyse.tree.node(id).san.as_str()))
                .map(|opening| opening.name.to_owned())
                .unwrap_or_else(|| "Analyse board position".to_owned());
            self.reached_position = Some((position_hash(&node.pos), label));
        }
        self.schedule_filter(cx);
    }
}
