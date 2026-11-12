//! The preview of the selected game.

use super::*;

/// "13.Ne2 Ne8 14.h4?!" from a stored game and its deviation like "14.h4".
pub(super) fn leak_context(left: &str, sans: &[String]) -> String {
    let Some((number, san)) = left.split_once(['.', '…']) else {
        return left.to_owned();
    };
    let Ok(number) = number.parse::<usize>() else {
        return left.to_owned();
    };
    let black = left.contains('…');
    let ply = (number - 1) * 2 + usize::from(black);
    if ply >= sans.len() {
        return left.to_owned();
    }
    let start = ply.saturating_sub(2);
    let line: Vec<String> = sans[start..ply].to_vec();
    let mut text = chess_core::format_pv(start as u32, &line);
    if !text.is_empty() {
        text.push(' ');
    }
    format!("{text}{number}{}{san}?!", if black { "…" } else { "." })
}

/// The ply to show in the preview: where the game left the repertoire, or the point
/// where the opening's named line ends. Games shorter than that show the final move.
pub(super) fn key_moment(details: &GameDetails) -> usize {
    if let Some(left) = &details.row.left_rep {
        let san = left.rsplit(['.', '…']).next().unwrap_or("");
        let number = left
            .split(['.', '…'])
            .next()
            .and_then(|n| n.parse::<usize>().ok());
        if let Some(number) = number {
            let ply = (number - 1) * 2 + usize::from(left.contains('…'));
            if details
                .sans
                .get(ply)
                .is_some_and(|played| played.trim_end_matches(['+', '#']) == san)
            {
                return ply;
            }
        }
    }
    // Otherwise a middlegame moment just past the named opening.
    let mut last_book = 0;
    let mut previous = None;
    for ply in 0..details.sans.len() {
        let opening = classify(details.sans[..=ply].iter().map(String::as_str));
        if opening != previous {
            last_book = ply + 1;
        }
        previous = opening;
    }
    const SHOWN_PLIES: usize = 26;
    details.sans.len().min(SHOWN_PLIES.max(last_book))
}

/// The comment and side variations that follow one mainline move.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct MoveNotes {
    /// The comment without `[%clk]`, `[%eval]` and other commands.
    comment: String,
    /// Each variation replacing this move, as PGN movetext.
    variations: Vec<String>,
}

/// The comments and variations after each mainline move of an annotated game, indexed
/// like `sans`. Empty for plain games.
pub(super) fn move_notes(details: &GameDetails) -> Vec<MoveNotes> {
    if details.annotated.is_none() {
        return Vec::new();
    }
    let Some(tree) = chess_core::parse_pgn(&details.to_pgn())
        .ok()
        .and_then(|mut games| (!games.is_empty()).then(|| games.remove(0)))
    else {
        return Vec::new();
    };
    tree.mainline()
        .into_iter()
        .map(|id| {
            let node = tree.node(id);
            let siblings = node
                .parent
                .map(|parent| tree.node(parent).children.clone())
                .unwrap_or_default();
            MoveNotes {
                comment: chess_core::strip_commands(&node.comment),
                variations: siblings
                    .into_iter()
                    .skip(1)
                    .map(|variation| chess_core::write_pgn_from(&tree, variation))
                    .collect(),
            }
        })
        .collect()
}

/// Shortens a comment for the preview so one long note does not push the board away.
pub(super) fn short_comment(text: &str) -> String {
    const LIMIT: usize = 140;
    if text.chars().count() <= LIMIT {
        return text.to_owned();
    }
    let cut: String = text.chars().take(LIMIT).collect();
    format!("{}…", cut.trim_end())
}

/// The moves around the key moment, with the next move marked as a pill. Comments
/// follow their move in `fg_2` and variations in `muted`, as in the scoresheet.
pub(super) fn move_snippet(
    preview: &GameDetails,
    notes: &[MoveNotes],
    key: usize,
) -> impl IntoElement + use<> {
    let start = key.saturating_sub(2);
    let end = (key + 6).min(preview.sans.len());
    let mut row = div()
        .flex()
        .flex_row()
        .flex_wrap()
        .gap_x(px(6.))
        .mt(px(10.))
        .mb(px(12.))
        .font_family(MONO)
        .text_as(14., T.fg_2)
        .line_height(px(24.));
    for (index, san) in preview.sans[start..end].iter().enumerate() {
        let ply = start + index;
        let label = if ply.is_multiple_of(2) {
            format!("{}.{san}", ply / 2 + 1)
        } else {
            san.clone()
        };
        let is_key = ply == key;
        row = row.child(
            div()
                .when(is_key, |pill| {
                    pill.px(px(5.))
                        .rounded(px(4.))
                        .bg(rgb(T.accent))
                        .text_color(rgb(T.accent_fg))
                })
                .child(ui::i18n::t(&label).to_owned()),
        );
        let Some(note) = notes.get(ply) else {
            continue;
        };
        // Notes take their own wrapped line so the snippet stays inside the panel.
        if !note.comment.is_empty() {
            row = row.child(
                div()
                    .w_full()
                    .font_family(ui::SANS)
                    .text_as(13., T.fg_2)
                    .line_height(px(19.))
                    .child(short_comment(&note.comment)),
            );
        }
        for variation in &note.variations {
            row = row.child(
                div()
                    .w_full()
                    .text_as(13., T.muted)
                    .line_height(px(19.))
                    .child(format!("({})", short_comment(variation))),
            );
        }
    }
    row
}

impl DatabaseScreen {
    pub(super) fn render_preview(
        &mut self,
        scale: f32,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let this = cx.entity();
        let mut column = v_flex().p(px(16.));
        column = column.child(section_label(ui::i18n::t("Preview")).mb(px(9.)));
        if let Some(preview) = &self.preview {
            let row = &preview.row;
            let mut meta = Vec::new();
            if !row.event.is_empty() && row.event != "?" {
                let round = if row.round.is_empty() || row.round == "?" {
                    String::new()
                } else {
                    format!(", Round {}", row.round)
                };
                meta.push(format!("{}{round}", row.event));
            }
            meta.push(result_text(&row.result));
            meta.push(format!("{} moves", row.moves_count()));
            column = column
                .child(
                    div()
                        .text_size(px(14.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .mb(px(6.))
                        .child(format!("{} – {}", row.white, row.black)),
                )
                .child(
                    div()
                        .text_as(12., T.muted)
                        .mb(px(9.))
                        .child(meta.join(", ")),
                );
            if let Some(review) = &preview.review_summary {
                column = column.child(
                    h_flex()
                        .flex_wrap()
                        .gap(px(6.))
                        .mb(px(8.))
                        .child(section_label("Review").mr(px(4.)))
                        .child(stat(
                            ui::i18n::t("White"),
                            review
                                .white_accuracy
                                .map_or_else(|| "—".to_owned(), |value| format!("{value:.1}%")),
                        ))
                        .child(stat(
                            ui::i18n::t("Black"),
                            review
                                .black_accuracy
                                .map_or_else(|| "—".to_owned(), |value| format!("{value:.1}%")),
                        ))
                        .child(badge(format!(
                            "{} ({})",
                            review.engine, review.search_limit
                        ))),
                );
            }
            let findings = review_findings(preview);
            if !findings.is_empty() {
                let mut list = v_flex().mt(px(2.)).mb(px(10.)).border_t_as(T.divider);
                for finding in findings.into_iter().take(6) {
                    let this = this.clone();
                    let ply = finding.ply;
                    list = list.child(FindingRow {
                        finding,
                        on_click: std::rc::Rc::new(move |window, cx| {
                            this.update(cx, |screen, cx| {
                                screen.preview_key = ply;
                                screen.open_in_analyse(window, cx);
                            });
                        }),
                    });
                }
                column = column
                    .child(section_label("Review findings").mt(px(6.)))
                    .child(list);
            }
        }
        column
            .child(
                div()
                    .size(ui::board::snap_board_size(px(249.), scale))
                    .flex_none()
                    .child(self.preview_board.clone()),
            )
            .when_some(self.preview.as_ref(), |column, preview| {
                column.child(move_snippet(preview, &self.preview_notes, self.preview_key))
            })
            .child({
                let previewed = self.preview.as_ref().map(|preview| preview.row.id);
                let delete = this.clone();
                h_flex()
                    .gap(px(6.))
                    .child(
                        btn("open-analyse", "Open in Analyse")
                            .flex_1()
                            .disabled(previewed.is_none())
                            .tooltip(ui::i18n::t(
                                "Saving in Analyse updates this game. Double-click a row to open it.",
                            ))
                            .on_click(this.listen(move |screen, window, cx| {
                                screen.open_in_analyse(window, cx)
                            })),
                    )
                    .child(
                        icon_btn("edit-game", "icons/pencil.svg")
                            .disabled(previewed.is_none())
                            .tooltip(ui::i18n::t("Edit players, result and other headers"))
                            .on_click(this.listen(move |screen, window, cx| {
                                if let Some(id) = previewed {
                                    screen.open_editor(id, window, cx);
                                }
                            })),
                    )
                    .child(
                        icon_btn("delete-game", "icons/trash.svg")
                            .disabled(previewed.is_none())
                            .tooltip(ui::i18n::t("Delete this game"))
                            .on_click(move |_, cx| {
                                if let Some(id) = previewed {
                                    delete.update(cx, |screen, cx| screen.ask_delete(vec![id], cx));
                                }
                            }),
                    )
            })
    }
}

/// One review finding in the preview: the move on the left and the score it lost on the
/// right, on a single line. Hovering shows the engine's explanation, and clicking or
/// pressing Enter opens the move in Analyse.
#[derive(IntoElement)]
pub(super) struct FindingRow {
    finding: ReviewFinding,
    on_click: ClickHandler,
}

/// A click handler that a row shares between mouse and keyboard.
pub(super) type ClickHandler = std::rc::Rc<dyn Fn(&mut Window, &mut gpui::App)>;

impl RenderOnce for FindingRow {
    fn render(self, window: &mut Window, cx: &mut gpui::App) -> impl IntoElement {
        let id = gpui::ElementId::from(("review-finding", self.finding.ply));
        let focus = window
            .use_keyed_state(id.clone(), cx, |_, cx| cx.focus_handle().tab_stop(true))
            .read(cx)
            .clone();
        let on_key = self.on_click.clone();
        let on_click = self.on_click;
        let text = SharedString::from(format!(
            "{} {}",
            self.finding.text,
            ui::i18n::t("Click to open it in Analyse.")
        ));
        h_flex()
            .id(id)
            .track_focus(&focus)
            .h(px(28.))
            .px(px(6.))
            .gap(px(8.))
            .justify_between()
            .border_b_as(T.divider)
            .cursor_pointer()
            .hover(|style| style.bg(rgb(T.hover)))
            .focus_visible(|style| style.border_2().border_color(rgb(T.accent)))
            .text_size(px(13.))
            .whitespace_nowrap()
            .on_click(move |_, window, cx| on_click(window, cx))
            .on_key_down(move |event, window, cx| {
                let key = event.keystroke.key.as_str();
                if (key == "enter" || key == "space") && !event.keystroke.modifiers.modified() {
                    on_key(window, cx);
                    cx.stop_propagation();
                }
            })
            .tooltip(move |window, cx| {
                gpui_component::tooltip::Tooltip::new(text.clone()).build(window, cx)
            })
            .child(
                mono(self.finding.label)
                    .min_w_0()
                    .overflow_hidden()
                    .text_ellipsis()
                    .text_color(rgb(T.fg)),
            )
            .child(mono(self.finding.loss).flex_none().text_color(rgb(T.warn)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leak_context_includes_preceding_moves() {
        let sans: Vec<String> = "e4 c5 Nf3 d6 d4 cxd4 Nxd4 Nf6 Nc3 a6 Be3 e5 Nb3 Be6 f3 Be7 Qd2 O-O O-O-O Nbd7 g4 b5 g5 b4 Ne2 Ne8 h4"
            .split(' ')
            .map(str::to_owned)
            .collect();
        assert_eq!(leak_context("14.h4", &sans), "13.Ne2 Ne8 14.h4?!");
    }
}
