//! The repertoire tree: adding lines by typing or from a file, and finding its gaps.

use super::*;

/// Parses a typed line such as "1.e4 c5 2.Nf3" into plain SAN tokens.
pub fn parse_line_text(text: &str) -> Vec<String> {
    text.split_ascii_whitespace()
        .filter_map(|token| {
            let san = token.rsplit(['.', '…']).next().unwrap_or(token).trim();
            (!san.is_empty()).then(|| san.to_owned())
        })
        .collect()
}

/// Every line of a game tree as SAN paths, one per leaf.
pub(super) fn tree_lines(tree: &chess_core::GameTree) -> Vec<Vec<String>> {
    let mut lines = Vec::new();
    let mut stack = vec![chess_core::GameTree::ROOT];
    while let Some(id) = stack.pop() {
        let node = tree.node(id);
        if node.children.is_empty() && id != chess_core::GameTree::ROOT {
            lines.push(
                tree.path_to(id)
                    .iter()
                    .map(|&step| tree.node(step).san.clone())
                    .collect(),
            );
        }
        stack.extend(node.children.iter().rev());
    }
    lines
}

impl TrainScreen {
    pub(super) fn submit_line(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        let line = parse_line_text(text);
        let refs: Vec<&str> = line.iter().map(String::as_str).collect();
        let result = self.model.update(cx, |model, cx| {
            let result = model.repertoire.add_line(&refs);
            if result.is_ok() {
                model.save_repertoire(cx);
                model.refresh_leaks(cx);
            }
            result
        });
        match result {
            Ok(()) => {
                self.adding = false;
                self.add_line
                    .update(cx, |input, cx| input.set_value("", window, cx));
                self.reload_queue(cx);
            }
            Err(err) => self
                .model
                .toast(format!("Could not add the line: {err}"), cx),
        }
    }

    pub(super) fn find_gaps(&mut self, cx: &mut Context<Self>) {
        let gaps = self.model.read(cx).repertoire.gaps();
        let text = match gaps.first() {
            None => ui::i18n::t("No gaps: every line ends on an answered move.").to_owned(),
            Some(first) => format!(
                "{} positions need an answer, for example after {}.",
                gaps.len(),
                format_pv(
                    first.len().saturating_sub(4) as u32,
                    &first[first.len().saturating_sub(4)..]
                )
            ),
        };
        self.model.toast(text, cx);
    }

    /// Imports lines from a PGN into the active repertoire (variations included).
    pub fn prompt_open(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let this = cx.entity().downgrade();
        pick_path(
            self.model.clone(),
            "Add lines from PGN",
            None,
            cx,
            move |path, cx| {
                let Some(this) = this.upgrade() else { return };
                this.update(cx, |this, cx| {
                    this.add_lines_from(path, cx);
                });
            },
        );
    }

    /// Adds every line of the games in a PGN file to the repertoire. The file is read and
    /// split into lines in the background.
    pub(super) fn add_lines_from(&mut self, path: std::path::PathBuf, cx: &mut Context<Self>) {
        let read = cx.background_spawn(async move {
            let text = std::fs::read_to_string(&path)
                .map_err(|err| format!("Could not read the file: {err}"))?;
            let games = parse_pgn(&text).map_err(|err| format!("Could not read the PGN: {err}"))?;
            Ok::<_, String>(games.iter().flat_map(tree_lines).collect::<Vec<_>>())
        });
        cx.spawn(async move |this, cx| {
            let lines = read.await;
            let _ = this.update(cx, |this, cx| {
                let message = match lines {
                    Ok(lines) => {
                        let mut added = 0;
                        this.model.update(cx, |model, cx| {
                            for line in &lines {
                                let refs: Vec<&str> = line.iter().map(String::as_str).collect();
                                if model.repertoire.add_line(&refs).is_ok() {
                                    added += 1;
                                }
                            }
                            model.save_repertoire(cx);
                            model.refresh_leaks(cx);
                        });
                        this.reload_queue(cx);
                        format!("Added {added} lines to the repertoire.")
                    }
                    Err(message) => message,
                };
                this.model.toast(message, cx);
            });
        })
        .detach();
    }

    pub(super) fn render_tree(
        &mut self,
        session: gpui::Div,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let this = cx.entity();
        let today = today();
        let model = self.model.read(cx);
        let rep = &model.repertoire;
        let coverage = rep.coverage(today);
        let (known, learning, new) = coverage.percents();
        let rows = rep.tree_rows(today);
        let selected = self.selected_path.clone();
        let recalling =
            self.current().is_some() && matches!(self.phase, Phase::Asking | Phase::Wrong);
        let mut list = v_flex().mb(px(14.));
        if recalling {
            let path = &self.current().unwrap().path;
            list = list.child(
                mono(if path.is_empty() {
                    ui::i18n::t("Starting position").to_owned()
                } else {
                    format_pv(0, path)
                })
                .px(px(16.))
                .text_as(13., T.fg),
            );
        }
        for (index, row) in rows.into_iter().enumerate().filter(|_| !recalling) {
            let is_selected = selected.as_ref().is_some_and(|path| {
                path.len() > row.path.len()
                    && path.starts_with(&row.path)
                    && row.status == RepStatus::Due
            });
            let (tag, color) = match row.status {
                RepStatus::Known => ("Known", T.good),
                RepStatus::Learning => ("Learning", T.fg_2),
                RepStatus::Due => ("Due", T.warn),
                RepStatus::New => ("New", T.muted),
            };
            list = list.child(
                h_flex()
                    .id(("rep-row", index))
                    .relative()
                    .h(px(32.))
                    .flex_none()
                    .pl(px(18. + row.depth as f32 * 14.))
                    .pr(px(18.))
                    .justify_between()
                    .border_b_as(T.divider)
                    .when(is_selected, |row_el| row_el.bg(rgb(T.selected)))
                    .when(is_selected, |row_el| {
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
                    .child(mono(row.label).text_as(14., T.fg))
                    .child(
                        div()
                            .text_size(px(12.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(color))
                            .child(tag),
                    ),
            );
        }
        let segment = |value: u32, color: u32| {
            div()
                .h_full()
                .flex_grow(value.max(1) as f32)
                .flex_basis(px(0.))
                .bg(rgb(color))
        };
        let import_this = this.clone();
        v_flex()
            .w(px(300.))
            .h_full()
            .flex_none()
            .bg(rgb(T.surface))
            .border_r_as(T.border)
            .child(
                v_flex()
                    .flex_none()
                    .px(px(18.))
                    .pt(px(16.))
                    .pb(px(10.))
                    .child(section_label("Repertoire").mb(px(9.)))
                    .child(
                        h_flex()
                            .justify_between()
                            .mb(px(14.))
                            .child(
                                div()
                                    .text_size(px(16.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(SharedString::from(rep.name.clone())),
                            )
                            .child(mono(format!("{} pos.", rep.positions())).text_as(14., T.muted)),
                    )
                    .child(
                        h_flex()
                            .h(px(6.))
                            .w_full()
                            .overflow_hidden()
                            .child(segment(known, T.fg))
                            .child(segment(learning, T.muted))
                            .child(segment(new, T.bg)),
                    )
                    .child(
                        h_flex()
                            .mt(px(6.))
                            .gap(px(14.))
                            .text_as(13., T.muted)
                            .child(format!("{known}% known"))
                            .child(format!("{learning}% learning"))
                            .child(format!("{new}% new")),
                    )
                    .child(session),
            )
            .child(
                div()
                    .id("rep-tree")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(list)
                    .when(self.adding, |column| {
                        column.child(
                            div()
                                .px(px(18.))
                                .pb(px(8.))
                                .child(text_input(&self.add_line)),
                        )
                    })
                    .child(
                        h_flex()
                            .px(px(18.))
                            .gap(px(6.))
                            .child(btn("add-line", "Add line").compact().on_click(this.listen(
                                move |screen, window, cx| {
                                    screen.adding = !screen.adding;
                                    if screen.adding {
                                        screen.add_line.update(cx, |input, cx| {
                                            input.focus(window, cx);
                                        });
                                    }
                                    cx.notify();
                                },
                            )))
                            .child(
                                btn("find-gaps", "Find gaps").compact().on_click(
                                    this.listen(move |screen, _, cx| screen.find_gaps(cx)),
                                ),
                            )
                            .child(btn("import-pgn", "Import PGN").compact().on_click(
                                move |window, cx| {
                                    import_this
                                        .update(cx, |screen, cx| screen.prompt_open(window, cx));
                                },
                            )),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_lines_lose_their_numbers() {
        assert_eq!(parse_line_text("1.e4 c5 2.Nf3"), vec!["e4", "c5", "Nf3"]);
        assert_eq!(parse_line_text("1. e4 1…c5"), vec!["e4", "c5"]);
        assert!(parse_line_text("  ").is_empty());
    }

    #[test]
    fn tree_lines_cover_each_leaf() {
        let tree = parse_pgn("1. e4 e5 (1... c5 2. Nf3) 2. Nf3 *")
            .unwrap()
            .remove(0);
        let lines = tree_lines(&tree);
        assert_eq!(lines.len(), 2);
        assert!(lines.contains(&vec!["e4".to_owned(), "e5".to_owned(), "Nf3".to_owned()]));
        assert!(lines.contains(&vec!["e4".to_owned(), "c5".to_owned(), "Nf3".to_owned()]));
    }
}
