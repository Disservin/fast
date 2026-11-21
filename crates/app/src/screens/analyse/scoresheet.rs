//! The scoresheet beside the board, its collapsed rail and the stored game strip above it.

use super::*;

/// A White-relative evaluation as the scoresheet shows it beside a move: pawns
/// ("+0.35"), or a mate ("#3", "#-2", and "#" or "-#" when its distance
/// is not known).
pub(super) fn eval_label(cp: i32, mate: Option<i32>) -> String {
    match mate {
        Some(mate) => format!("#{mate}"),
        None if cp >= chess_core::annotation::MATE_CP => "#".to_owned(),
        None if cp <= -chess_core::annotation::MATE_CP => "-#".to_owned(),
        None => Score::Cp(cp).display(),
    }
}

impl AnalyseScreen {
    /// How many main-line moves are opening theory, and the opening's name, for a game
    /// from the standard starting position.
    fn book_line(&self) -> Option<(usize, SharedString)> {
        if self.tree.variant != chess_core::Variant::Standard || self.tree.header("FEN").is_some() {
            return None;
        }
        let main = self.tree.mainline();
        chess_core::classify_depth(main.iter().map(|&id| self.tree.node(id).san.as_str()))
            .map(|(opening, plies)| (plies, opening.name.into()))
    }

    pub(super) fn timeline_ids(&self) -> Vec<NodeId> {
        self.tree.line_through(self.current)
    }

    /// Folds the scoresheet to a narrow strip or opens it again, and remembers it.
    pub(super) fn set_scoresheet_collapsed(&mut self, collapsed: bool, cx: &mut Context<Self>) {
        self.model.update(cx, |model, cx| {
            model.config.panels.scoresheet_collapsed = collapsed;
            model.schedule_config_save(cx);
        });
        cx.notify();
    }

    /// The folded scoresheet: a button to open it again and, for a game from a
    /// database, a Save button that shows when there are unsaved changes.
    pub(super) fn render_scoresheet_rail(
        &self,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let this = cx.entity();
        let expand = this.clone();
        v_flex()
            .w_full()
            .h_full()
            .items_center()
            .pt(px(12.))
            .gap(px(6.))
            .bg(rgb(T.surface))
            .border_r_as(T.border)
            .child(
                icon_btn("sheet-expand", "icons/chevron-right.svg")
                    .compact()
                    .tooltip(ui::i18n::t("Show the scoresheet"))
                    .on_click(
                        expand.listen(move |screen, _, cx| {
                            screen.set_scoresheet_collapsed(false, cx)
                        }),
                    ),
            )
            .when_some(self.db_game.as_ref(), |rail, origin| {
                let dirty = self.db_game_dirty();
                let tip = if dirty {
                    format!("{}: {}", origin.name, ui::i18n::t("Unsaved changes"))
                } else {
                    format!("{}: {}", origin.name, ui::i18n::t("Saved in this database"))
                };
                rail.child(
                    icon_btn("sheet-rail-save", "icons/database.svg")
                        .compact()
                        .when(dirty, |button| button.dot(T.warn))
                        .disabled(!dirty)
                        .tooltip(tip)
                        .on_click(this.listen(move |screen, _, cx| screen.save_game(cx))),
                )
            })
    }

    /// Under the scoresheet header while the game came from a database: which database,
    /// whether there are unsaved changes, a Save button, and the review findings.
    pub(super) fn render_db_game_strip(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let origin = self.db_game.as_ref()?;
        let dirty = self.db_game_dirty();
        let this = cx.entity();
        let mut strip = v_flex()
            .flex_none()
            .px(px(18.))
            .py(px(10.))
            .gap(px(8.))
            .bg(rgb(T.surface_2))
            .border_b_as(T.border)
            .child(
                h_flex()
                    .gap(px(8.))
                    .child(ui::widgets::icon("icons/database.svg", px(16.), T.accent))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .truncate()
                                    .child(origin.name.clone()),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(rgb(if dirty { T.warn } else { T.muted }))
                                    .child(if dirty {
                                        ui::i18n::t("Unsaved changes")
                                    } else {
                                        ui::i18n::t("Saved in this database")
                                    }),
                            ),
                    )
                    .child(
                        btn("db-game-save", "Save")
                            .compact()
                            .when(dirty, |button| button.primary())
                            .disabled(!dirty)
                            .tooltip(ui::i18n::t(
                                "Save moves, comments and variations to the database (Ctrl+S)",
                            ))
                            .on_click(this.listen(move |screen, _, cx| screen.save_game(cx))),
                    ),
            );
        if !self.db_findings.is_empty() {
            let mut chips = h_flex().flex_wrap().gap(px(4.));
            for (index, (node, mark)) in self.db_findings.iter().enumerate() {
                let node = *node;
                let tip = if mark.best.is_empty() {
                    mark.loss.clone()
                } else {
                    format!(
                        "{}, {} {}",
                        mark.loss,
                        ui::i18n::t("engine line"),
                        mark.best
                    )
                };
                chips = chips.child(
                    btn(
                        ("db-finding", index),
                        format!("{} {}", mark.label, mark.loss),
                    )
                    .mini()
                    .mono()
                    .pad(6.)
                    .dot(quality_color(mark.quality))
                    .selected(node == self.current)
                    .tooltip(tip)
                    .on_click(this.listen(move |screen, window, cx| screen.jump(node, window, cx))),
                );
            }
            strip = strip.child(section_label("Review findings")).child(chips);
        }
        Some(strip.into_any_element())
    }

    pub(super) fn render_scoresheet(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let this = cx.entity();
        let notation = self.model.read(cx).config.board.notation;
        let context_this = this.clone();
        // Only a move drawn in this frame counts; the start position has none.
        self.scoresheet_current.set(None);
        let evals = self
            .evals
            .iter()
            .filter(|(id, _)| **id != GameTree::ROOT)
            .map(|(&id, &cp)| (id, eval_label(cp, self.mates.get(&id).copied()).into()))
            .collect();
        let props = ScoresheetProps {
            current: self.current,
            current_bounds: self.scoresheet_current.clone(),
            evals: Rc::new(evals),
            notation,
            on_select: Rc::new(
                this.listen_with(move |screen, id, window, cx| screen.jump(id, window, cx)),
            ),
            on_context: Rc::new(move |id, event, _, cx| {
                let position = event.position;
                context_this.update(cx, |screen, cx| {
                    screen.context_menu = Some((id, position));
                    cx.notify();
                });
            }),
            book: self.book_line(),
            unfinished: game_card::result_label(self.tree.header("Result")) == "Unfinished",
        };
        let style = self.model.read(cx).config.view.scoresheet;
        let sheet = match self.sheet_tab {
            SheetTab::Headers => self
                .render_header_form(cx)
                .unwrap_or_else(|| div().into_any_element()),
            SheetTab::Notes => self.render_notes_list(cx),
            SheetTab::Moves => match style {
                ScoresheetStyle::Table => scoresheet_body(&self.tree, &props).into_any_element(),
                ScoresheetStyle::Inline => scoresheet_inline(&self.tree, &props).into_any_element(),
            },
        };
        v_flex()
            .w_full()
            .h_full()
            .flex_none()
            .bg(rgb(T.surface))
            .border_r_as(T.border)
            .child(
                self.render_game_card(
                    h_flex().child(
                        icon_btn("sheet-collapse", "icons/chevron-left.svg")
                            .mini()
                            .w(24.)
                            .pad(0.)
                            .tooltip(ui::i18n::t("Hide the scoresheet"))
                            .on_click(this.listen(move |screen, _, cx| {
                                screen.set_scoresheet_collapsed(true, cx)
                            })),
                    ),
                    cx,
                ),
            )
            .children(self.render_db_game_strip(cx))
            .child(self.render_sheet_tabs(cx))
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .child(
                        div()
                            .id("scoresheet-scroll")
                            .size_full()
                            .overflow_y_scroll()
                            .track_scroll(&self.scoresheet_scroll)
                            .py(px(4.))
                            .child(sheet),
                    )
                    .child(self.scoresheet_follower())
                    .vertical_scrollbar(&self.scoresheet_scroll),
            )
            .when(self.editing_comment.is_some(), |column| {
                let comment = self.comment.clone();
                let save = this.clone();
                column.child(
                    v_flex()
                        .flex_none()
                        .gap(px(6.))
                        .p(px(12.))
                        .border_t_as(T.border)
                        .child(gpui_component::input::Textarea::new(&comment).h(px(80.)))
                        .child(
                            btn("comment-save", "Save comment")
                                .primary()
                                .h(30.)
                                .on_click(save.listen(move |screen, _, cx| {
                                    let text = screen.comment.read(cx).value().to_string();
                                    if let Some(node) = screen.editing_comment.take() {
                                        let updated = chess_core::with_comment_text(
                                            &screen.tree.node(node).comment,
                                            &text,
                                        );
                                        screen.tree.node_mut(node).comment = updated;
                                    }
                                    cx.notify();
                                })),
                        ),
                )
            })
            .child(self.render_annotation_bar(cx))
    }

    /// After the scoresheet is laid out, scrolls just far enough to show the current move
    /// when it has changed, and leaves the scroll position alone otherwise.
    pub(super) fn scoresheet_follower(&self) -> impl IntoElement + use<> {
        let scroll = self.scoresheet_scroll.clone();
        let current = self.scoresheet_current.clone();
        let follow = self.scoresheet_follow.clone();
        gpui::canvas(
            move |_, window, _| {
                if !follow.replace(false) {
                    return;
                }
                let mut offset = scroll.offset();
                let Some(pill) = current.get() else {
                    // The start position: show the top of the sheet.
                    if offset.y != px(0.) {
                        scroll.set_offset(gpui::point(offset.x, px(0.)));
                        window.refresh();
                    }
                    return;
                };
                let view = scroll.bounds();
                // Keep a row of context above and below the current move.
                let margin = px(28.);
                let top = view.top() + margin;
                let bottom = view.bottom() - margin;
                let shift = if pill.top() < top {
                    top - pill.top()
                } else if pill.bottom() > bottom {
                    bottom - pill.bottom()
                } else {
                    return;
                };
                let max = scroll.max_offset().y;
                offset.y = (offset.y + shift).clamp(-max, px(0.));
                scroll.set_offset(offset);
                window.refresh();
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_0()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scoresheet_evals_are_short() {
        assert_eq!(eval_label(35, None), "+0.35");
        assert_eq!(eval_label(-120, None), "-1.20");
        assert_eq!(eval_label(2000, Some(3)), "#3");
        assert_eq!(eval_label(-2000, None), "-#");
    }
}
