//! Changing the game: annotating moves and adding, playing, promoting and deleting lines.

use super::*;

impl AnalyseScreen {
    pub(super) fn annotate(&mut self, nag: u8, cx: &mut Context<Self>) {
        if self.current != GameTree::ROOT {
            self.tree.toggle_nag(self.current, nag);
            cx.notify();
        }
    }

    pub(super) fn promote(&mut self, id: NodeId, cx: &mut Context<Self>) {
        self.tree.promote(id);
        self.context_menu = None;
        cx.notify();
    }

    pub(super) fn delete_from(&mut self, id: NodeId, window: &mut Window, cx: &mut Context<Self>) {
        let on_deleted_line = self.tree.path_to(self.current).contains(&id);
        let parent = self.tree.delete_from(id);
        self.context_menu = None;
        if on_deleted_line {
            self.current = parent;
            self.position_changed(false, window, cx);
        }
        cx.notify();
    }

    /// Adds SAN moves as a line from the current node, returning each new node.
    pub(super) fn add_line(&mut self, sans: &[String]) -> Vec<NodeId> {
        let mut nodes = Vec::with_capacity(sans.len());
        let mut node = self.current;
        for san in sans {
            match self.tree.add_san(node, san) {
                Ok(next) => {
                    nodes.push(next);
                    node = next;
                }
                Err(err) => {
                    log::warn!("could not add {san}: {err}");
                    break;
                }
            }
        }
        nodes
    }

    /// The line the Line preview buttons act on, from the game position.
    pub(super) fn action_line(&self) -> Option<Vec<String>> {
        if let Some(view) = &self.pv_view {
            return Some(view.sans.clone());
        }
        let row = self.selected_row()?;
        Some(if row.pv.is_empty() {
            vec![row.san.clone()]
        } else {
            row.pv.clone()
        })
    }

    pub(super) fn play_line(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(line) = self.action_line() else {
            return;
        };
        // From a preview, stop at the move being looked at; otherwise play it all.
        let target = self.pv_view.as_ref().map(PvView::played);
        let nodes = self.add_line(&line);
        let index = target.unwrap_or(nodes.len()).saturating_sub(1);
        if let Some(&node) = nodes.get(index) {
            self.jump(node, window, cx);
        }
    }

    pub(super) fn add_variation(&mut self, cx: &mut Context<Self>) {
        if let Some(line) = self.action_line() {
            self.add_line(&line);
            cx.notify();
        }
    }

    pub(super) fn add_to_repertoire(&mut self, cx: &mut Context<Self>) {
        let Some(line) = self.action_line() else {
            return;
        };
        let mut path = self.base_san_path();
        path.extend(line);
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
    }

    pub(super) fn render_context_menu(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement + use<>> {
        let (id, position) = self.context_menu?;
        let this = cx.entity();
        let item = |index: u32, label: &'static str, handler: MenuHandler| {
            div()
                .id(("context-item", index))
                .px(px(12.))
                .py(px(7.))
                .text_size(px(14.))
                .cursor_pointer()
                .hover(|style| style.bg(rgb(T.hover)))
                .on_click(move |_, window, cx| handler(window, cx))
                .child(ui::i18n::t(label).to_owned())
        };
        let delete_this = this.clone();
        let delete_sidelines_this = this.clone();
        let promote_this = this.clone();
        let copy_this = this.clone();
        Some(deferred(
            anchored().position(position).child(
                v_flex()
                    .id("scoresheet-context")
                    // Keep the parent dismissal handler from removing the menu
                    // on mouse-down before its items receive their click.
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .w(px(190.))
                    .py(px(4.))
                    .bg(rgb(T.surface))
                    .border_as(T.border_strong)
                    .rounded(px(6.))
                    .on_mouse_down_out(cx.listener(|screen, _, _, cx| {
                        screen.context_menu = None;
                        cx.notify();
                    }))
                    .child(item(
                        0,
                        ui::i18n::t("Delete from here"),
                        Box::new(move |window, cx| {
                            delete_this.update(cx, |screen, cx| screen.delete_from(id, window, cx));
                        }),
                    ))
                    .child(item(
                        3,
                        ui::i18n::t("Delete all sidelines"),
                        Box::new(move |window, cx| {
                            delete_sidelines_this.update(cx, |screen, cx| {
                                let selected = screen.tree.delete_all_sidelines(screen.current);
                                screen.context_menu = None;
                                if selected != screen.current {
                                    screen.current = selected;
                                    screen.position_changed(false, window, cx);
                                }
                                cx.notify();
                            });
                        }),
                    ))
                    .child(item(
                        1,
                        ui::i18n::t("Promote variation"),
                        Box::new(move |_, cx| {
                            promote_this.update(cx, |screen, cx| screen.promote(id, cx));
                        }),
                    ))
                    .child(item(
                        2,
                        ui::i18n::t("Copy PGN from here"),
                        Box::new(move |_, cx| {
                            copy_this.update(cx, |screen, cx| {
                                let text = write_pgn_from(&screen.annotated_tree(), id);
                                cx.write_to_clipboard(ClipboardItem::new_string(text));
                                screen.context_menu = None;
                                screen.model.toast("PGN copied.", cx);
                            });
                        }),
                    )),
            ),
        ))
    }
}
