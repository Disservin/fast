//! Highlighting part of an engine line and peeking at the position after a move.

use super::*;

/// Side of the board that follows the cursor over a hovered PV move.
pub(super) const PEEK_SIZE: f32 = 200.;

/// How far the peek board sits below and right of the cursor, so it never covers the move.
pub(super) const PEEK_OFFSET: f32 = 14.;

/// A highlighted run of moves in the selected line. The line is copied so engine
/// updates cannot move it while it is being read.
pub struct PvView {
    pub sans: Vec<String>,
    /// The source line's figures stay available while the main engine searches on.
    pub source: Option<MoveRow>,
    pub anchor: usize,
    pub head: usize,
}

impl PvView {
    /// Keeps the preview's source figures when its move leaves the live table.
    pub fn source_row<'a>(&'a self, live: Option<&'a MoveRow>) -> Option<&'a MoveRow> {
        live.filter(|row| row.score.is_some())
            .or(self.source.as_ref())
            .or(live)
    }

    /// Inclusive first and last highlighted move.
    pub fn range(&self) -> (usize, usize) {
        (self.anchor.min(self.head), self.anchor.max(self.head))
    }

    pub fn is_range(&self) -> bool {
        self.anchor != self.head
    }

    /// How many of the line's moves the board has played: through a single highlighted
    /// move, or up to the start of a range, whose moves are drawn as arrows instead.
    pub fn played(&self) -> usize {
        let (start, end) = self.range();
        if start == end { end + 1 } else { start }
    }

    pub(super) fn contains(&self, index: usize) -> bool {
        let (start, end) = self.range();
        (start..=end).contains(&index)
    }
}

pub(super) fn move_squares(mv: &Move) -> Option<(chess_core::Square, chess_core::Square)> {
    Some((mv.from()?, mv.to()))
}

impl AnalyseScreen {
    /// The moves being looked at beyond the game position, which the engines and the
    /// table follow; none while the kibitz analyses the preview instead.
    pub(super) fn preview_moves(&self) -> &[String] {
        match &self.pv_view {
            Some(view) if !self.pv_kibitz => &view.sans[..view.played()],
            _ => &[],
        }
    }

    pub(super) fn begin_pv_highlight(
        &mut self,
        sans: Vec<String>,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.pv_focus.focus(window, cx);
        self.pv_hover = None;
        // A second click on the only highlighted move goes back to the game.
        self.pv_toggle_off = self
            .pv_view
            .as_ref()
            .is_some_and(|view| view.anchor == index && view.head == index);
        let source = if self.pv_kibitz {
            self.pv_view
                .as_ref()
                .and_then(|view| view.source_row(self.selected_row()))
                .cloned()
        } else {
            self.selected_row().cloned()
        };
        self.pv_view = Some(PvView {
            sans,
            source,
            anchor: index,
            head: index,
        });
        self.pv_dragging = true;
        self.show_pv_preview(cx);
    }

    pub(super) fn extend_pv_highlight(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(view) = &mut self.pv_view else {
            return;
        };
        if view.head != index {
            view.head = index;
            self.pv_toggle_off = false;
            self.show_pv_preview(cx);
        }
    }

    pub(super) fn end_pv_highlight(&mut self, index: usize, cx: &mut Context<Self>) {
        self.pv_dragging = false;
        let single = self
            .pv_view
            .as_ref()
            .is_some_and(|view| view.anchor == index && view.head == index);
        if std::mem::take(&mut self.pv_toggle_off) && single {
            self.exit_pv_preview(cx);
        }
    }

    /// Hovering a move marks it in the line and shows the position after it beside the
    /// cursor. The next move to play is also drawn as an arrow on the board.
    pub(super) fn hover_pv_move(
        &mut self,
        index: usize,
        hovering: bool,
        sans: &[String],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // The move being previewed is already on the board.
        let on_highlight = self
            .pv_view
            .as_ref()
            .is_some_and(|view| view.contains(index));
        if self.pv_dragging || on_highlight || self.pv_view.as_ref().is_some_and(PvView::is_range) {
            return;
        }
        if !hovering {
            // Moving to the next move reports its hover first, so only clear our own.
            if self.pv_hover == Some(index) {
                self.pv_hover = None;
                self.update_arrows(cx);
                cx.notify();
            }
            return;
        }
        self.pv_hover = Some(index);
        let base = &self.tree.node(self.current).pos;
        let (after, moves) = replay(base, &sans[..sans.len().min(index + 1)]);
        // A line that stops applying before this move has no position to show.
        self.pv_peek_at = (moves.len() > index).then(|| window.mouse_position());
        let squares = moves.get(index).and_then(move_squares);
        self.update_arrows(cx);
        // The peek board shows the position after the move, the way the main board faces.
        let flipped = self.board.read(cx).is_flipped();
        let mut prefs = self.model.read(cx).config.board.prefs();
        prefs.coordinates = CoordinateStyle::Off;
        prefs.animation_ms = 0;
        self.pv_peek.update(cx, |peek, cx| {
            peek.set_prefs(prefs, cx);
            peek.set_flipped(flipped, cx);
            peek.set_position(after, squares, false, cx);
        });
        cx.notify();
    }

    /// Esc leaves a preview or a log selection and Ctrl+C copies it, wherever focus has
    /// gone. A log selection comes first. Returns whether the key was used.
    pub fn handle_pv_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) -> bool {
        if self.handle_log_key(event, cx) {
            return true;
        }
        if self.pv_view.is_none() {
            return false;
        }
        let keystroke = &event.keystroke;
        if keystroke.key == "escape" {
            self.exit_pv_preview(cx);
            true
        } else if keystroke.key == "c" && keystroke.modifiers.secondary(cx) {
            self.copy_pv(cx);
            true
        } else {
            false
        }
    }
}

impl AnalyseScreen {
    /// The line as individual moves: click one to go there, drag to highlight several.
    pub(crate) fn render_pv(
        &mut self,
        selected: Option<&MoveRow>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let sans = self.preview_line(selected);
        // Numbered from the game position, wherever the preview has got to.
        let text = format_pv(self.tree.node(self.current).ply, &sans);
        let labels: Vec<&str> = if text.is_empty() {
            Vec::new()
        } else {
            text.split(' ').collect()
        };
        let mut flow = div()
            .id("pv-moves")
            .key_context("FastButton")
            .track_focus(&self.pv_focus)
            .flex()
            .flex_row()
            .flex_wrap()
            .min_h(px(26.))
            .font_family(MONO)
            .text_as(14., T.fg)
            .line_height(px(26.))
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _, _, _| this.pv_dragging = false),
            );
        for (index, label) in labels.into_iter().enumerate() {
            let highlighted = self
                .pv_view
                .as_ref()
                .is_some_and(|view| view.contains(index));
            let hovered = self.pv_hover == Some(index) && !highlighted;
            let begin_sans = sans.clone();
            let hover_sans = sans.clone();
            flow = flow.child(
                div()
                    .id(("pv-move", index))
                    .px(px(3.))
                    .cursor_pointer()
                    .rounded(px(3.))
                    .when(highlighted, |chip| {
                        chip.bg(rgb(T.accent)).text_color(rgb(T.accent_fg))
                    })
                    .when(hovered, |chip| {
                        chip.bg(rgb(T.hover_strong)).text_color(rgb(T.fg))
                    })
                    .on_hover(cx.listener(move |this, hovering: &bool, window, cx| {
                        this.hover_pv_move(index, *hovering, &hover_sans, window, cx);
                    }))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, window, cx| {
                            this.begin_pv_highlight(begin_sans.clone(), index, window, cx);
                            cx.stop_propagation();
                        }),
                    )
                    .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
                        if this.pv_dragging && event.dragging() {
                            this.extend_pv_highlight(index, cx);
                        } else if this.pv_hover == Some(index) {
                            // The peek board follows the cursor across the move.
                            this.pv_peek_at = Some(event.position);
                            cx.notify();
                        }
                    }))
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(move |this, _, _, cx| this.end_pv_highlight(index, cx)),
                    )
                    .child(label.to_owned()),
            );
        }
        let peek = self
            .pv_peek_at
            .filter(|_| self.pv_hover.is_some())
            .map(|at| {
                let offset = px(PEEK_OFFSET);
                deferred(
                    anchored()
                        .position(at + point(offset, offset))
                        .snap_to_window_with_margin(px(8.))
                        .child(
                            div()
                                .size(ui::board::snap_board_size(px(PEEK_SIZE), self.scale))
                                .shadow_lg()
                                .child(self.pv_peek.clone()),
                        ),
                )
                .with_priority(1)
            });
        // Long lines scroll inside the fixed-height inset, so the buttons below stay put.
        v_flex()
            .size_full()
            .child(
                div()
                    .id("pv-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scrollbar()
                    .child(flow),
            )
            .children(peek)
            .when(self.pv_view.is_some(), |column| {
                column.child(
                    h_flex()
                        .flex_none()
                        .mt(px(6.))
                        .gap(px(10.))
                        .child(div().flex_1().text_as(12., T.muted).child(ui::i18n::t(
                            "Drag across moves to highlight. Ctrl+C copies, Esc returns.",
                        )))
                        .child(
                            btn("pv-back", "Back to game").compact().on_click(
                                cx.entity()
                                    .listen(move |screen, _, cx| screen.exit_pv_preview(cx)),
                            ),
                        ),
                )
            })
    }

    /// Puts the highlight on the board and points the engine and table at that position.
    /// One move: the position after it. Several: the position before them, each drawn
    /// as an arrow.
    pub(crate) fn show_pv_preview(&mut self, cx: &mut Context<Self>) {
        let Some(view) = &self.pv_view else {
            return;
        };
        let (start, end) = view.range();
        let played = view.played();
        let base = self.tree.node(self.current).pos.clone();
        let (_, moves) = replay(&base, &view.sans[..=end.min(view.sans.len() - 1)]);
        let (shown, _) = replay(&base, &view.sans[..played]);
        let last = played
            .checked_sub(1)
            .and_then(|index| moves.get(index))
            .and_then(move_squares);
        let arrows: Vec<Arrow> = if start == end {
            Vec::new()
        } else {
            moves[start.min(moves.len())..]
                .iter()
                .filter_map(move_squares)
                .map(|(from, to)| Arrow::new(from, to))
                .collect()
        };
        self.board.update(cx, |board, cx| {
            board.set_position(shown, last, false, cx);
            board.set_arrows(arrows, cx);
            board.set_interactive(false, cx);
        });
        // With a kibitz engine the preview is its search alone: the main analysis keeps
        // the game position, and the table keeps its rows and selection.
        if self.pv_played != Some(played)
            && (self.pv_kibitz || self.pv_played.is_none())
            && let Some(config) = self.model.read(cx).kibitz_engine()
        {
            self.pv_played = Some(played);
            self.pv_kibitz = true;
            let settings = self.model.read(cx).config.kibitz.clone();
            let extra: Vec<Move> = moves.iter().copied().take(played).collect();
            let game = self.live.read(cx).game;
            let (tree, current) = (self.tree.clone(), self.current);
            self.kibitz.update(cx, |kibitz, cx| {
                kibitz.analyse(&config, &settings, &tree, current, &extra, game, cx)
            });
            self.update_arrows(cx);
        }
        // A new position needs a new search, and a new best line in the table.
        if self.pv_played != Some(played) {
            self.pv_played = Some(played);
            self.selected = None;
            self.row_chosen = false;
            let extra: Vec<Move> = moves.into_iter().take(played).collect();
            let (tree, current) = (self.tree.clone(), self.current);
            self.live
                .update(cx, |live, cx| live.set_line(&tree, current, &extra, cx));
            self.query_databases(cx);
        }
        cx.notify();
    }
}

impl AnalyseScreen {
    /// The moves Line preview lists: the previewed line, or else the selected row's.
    pub(crate) fn preview_line(&self, selected: Option<&MoveRow>) -> Vec<String> {
        match (&self.pv_view, selected) {
            (Some(view), _) => view.sans.clone(),
            (None, Some(row)) if !row.pv.is_empty() => row.pv.clone(),
            (None, Some(row)) => vec![row.san.clone()],
            (None, None) => Vec::new(),
        }
    }

    /// Move `index` of the line Line preview lists now, if that line still reaches it.
    pub(crate) fn hovered_move(&self, index: usize) -> Option<Move> {
        let sans = self.preview_line(self.selected_row());
        let base = &self.tree.node(self.current).pos;
        let (_, moves) = replay(base, &sans[..sans.len().min(index + 1)]);
        moves.get(index).copied()
    }

    /// How many moves of the previewed line the board has played.
    pub(crate) fn shown_moves(&self) -> usize {
        self.pv_view.as_ref().map_or(0, PvView::played)
    }

    /// The position on the board, including a previewed line.
    pub(crate) fn shown_pos(&self) -> Chess {
        let moves = self
            .pv_view
            .as_ref()
            .map_or(&[][..], |view| &view.sans[..view.played()]);
        replay(&self.tree.node(self.current).pos, moves).0
    }

    /// The position on the board, which the engine and the table are also following.
    pub(crate) fn analysed_pos(&self) -> Chess {
        replay(&self.tree.node(self.current).pos, self.preview_moves()).0
    }

    pub(crate) fn analysed_ply(&self) -> u32 {
        self.tree.node(self.current).ply + self.preview_moves().len() as u32
    }

    pub(crate) fn analysed_san_path(&self) -> Vec<String> {
        let mut path = self.base_san_path();
        path.extend_from_slice(self.preview_moves());
        path
    }

    /// Picking a table row during a preview continues the line with that row. Returns
    /// false when there is no preview to continue.
    pub(crate) fn continue_pv_with(&mut self, key: &RowKey, cx: &mut Context<Self>) -> bool {
        let Some(played) = self.pv_view.as_ref().map(PvView::played) else {
            return false;
        };
        // The table shows the game position's moves, not the preview's.
        if played == 0 || self.pv_kibitz {
            self.exit_pv_preview(cx);
            return false;
        }
        let continuation = self
            .rows
            .iter()
            .find(|row| key.matches(row))
            .map(|row| {
                if row.pv.is_empty() {
                    vec![row.san.clone()]
                } else {
                    row.pv.clone()
                }
            })
            .unwrap_or_default();
        if let Some(view) = &mut self.pv_view {
            view.sans.truncate(played);
            view.sans.extend(continuation);
            view.anchor = played - 1;
            view.head = played - 1;
        }
        self.row_chosen = true;
        self.selected = Some(key.clone());
        self.update_arrows(cx);
        cx.notify();
        true
    }

    /// Ends any highlight and returns the board, engine and table to the game position.
    pub(crate) fn exit_pv_preview(&mut self, cx: &mut Context<Self>) {
        if self.pv_view.take().is_none() {
            return;
        }
        self.pv_dragging = false;
        self.pv_played = None;
        self.pv_hover = None;
        let kibitz = std::mem::take(&mut self.pv_kibitz);
        if !kibitz {
            self.selected = None;
            self.row_chosen = false;
        }
        let node = self.tree.node(self.current);
        let pos = node.pos.clone();
        let last = node.mv.as_ref().and_then(move_squares);
        self.board.update(cx, |board, cx| {
            board.set_position(pos, last, false, cx);
            board.set_arrows(Vec::new(), cx);
            board.set_interactive(true, cx);
        });
        if kibitz {
            // The main analysis never left the game position, but the table held its
            // selection for the preview: let it follow the best line again, and draw
            // the arrows for what the table shows now.
            self.kibitz.update(cx, |kibitz, cx| kibitz.stop(cx));
            self.rebuild_rows(cx);
        } else {
            let (tree, current) = (self.tree.clone(), self.current);
            self.live
                .update(cx, |live, cx| live.set_position(&tree, current, cx));
            self.query_databases(cx);
        }
        cx.notify();
    }

    /// Copies the highlighted moves, or the whole line if none are highlighted.
    pub(crate) fn copy_pv(&mut self, cx: &mut Context<Self>) {
        let ply = self.tree.node(self.current).ply;
        let text = match &self.pv_view {
            Some(view) => {
                let (start, end) = view.range();
                format_pv(ply + start as u32, &view.sans[start..=end])
            }
            None => match self.selected_row() {
                Some(row) if !row.pv.is_empty() => format_pv(ply, &row.pv),
                Some(row) => format_pv(ply, std::slice::from_ref(&row.san)),
                None => return,
            },
        };
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.model.toast("Line copied.", cx);
    }

    /// The kibitz engine's view of the previewed position, apart from the main analysis:
    /// its name, score, depth and best line.
    pub(crate) fn render_kibitz_row(&self, cx: &gpui::App) -> Option<AnyElement> {
        let view = self.pv_view.as_ref().filter(|_| self.pv_kibitz)?;
        let kibitz = self.kibitz.read(cx);
        let line = kibitz.line.as_ref();
        let ply = self.tree.node(self.current).ply + view.played() as u32;
        let score = line
            .and_then(|line| line.score)
            .map_or_else(|| "—".to_owned(), engine::Score::display);
        let depth = line.map_or_else(|| "—".to_owned(), |line| line.update.depth.to_string());
        let best = line
            .filter(|line| !line.sans.is_empty())
            .map(|line| format_pv(ply, &line.sans))
            .unwrap_or_else(|| {
                if kibitz.running {
                    ui::i18n::t("Searching…").to_owned()
                } else {
                    "—".to_owned()
                }
            });
        Some(
            h_flex()
                .mt(px(10.))
                .gap(px(14.))
                .items_center()
                .child(
                    v_flex().flex_none().child(section_label("Kibitz")).child(
                        div()
                            .text_as(12., T.muted)
                            .whitespace_nowrap()
                            .child(kibitz.name.clone()),
                    ),
                )
                .child(ui::widgets::figure("Eval", score))
                .child(ui::widgets::figure("Depth", depth))
                .child(
                    mono(best)
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_as(12., T.fg_2),
                )
                .into_any_element(),
        )
    }

    /// Heading text while the board shows something other than the game position.
    pub(crate) fn pv_preview_label(&self, ply: u32) -> Option<String> {
        let view = self.pv_view.as_ref()?;
        let (start, end) = view.range();
        let line = format_pv(ply + start as u32, &view.sans[start..=end]);
        Some(format!("previewing {line}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine::Score;

    /// A running main search cannot remove the figures of a kibitz preview's line.
    #[test]
    fn preview_keeps_eval_when_source_leaves_engine_lines() {
        let source = MoveRow {
            san: "e4".to_owned(),
            score: Some(Score::Cp(42)),
            depth: 20,
            ..Default::default()
        };
        let view = PvView {
            sans: vec!["e4".to_owned(), "e5".to_owned()],
            source: Some(source.clone()),
            anchor: 1,
            head: 1,
        };
        let database_only = MoveRow {
            score: None,
            depth: 0,
            ..source.clone()
        };
        for live in [None, Some(&database_only)] {
            let kept = view.source_row(live).unwrap();
            assert_eq!(kept.score, Some(Score::Cp(42)));
            assert_eq!(kept.depth, 20);
        }
        let updated = MoveRow {
            score: Some(Score::Cp(55)),
            depth: 24,
            ..source
        };
        assert_eq!(view.source_row(Some(&updated)).unwrap().depth, 24);
    }

    #[test]
    fn replay_stops_at_a_move_that_does_not_apply() {
        let line = ["e4", "e5", "e4", "Nf6"].map(String::from);
        let (_, moves) = replay(&Chess::default(), &line);
        assert_eq!(moves.len(), 2);
    }

    #[test]
    fn highlight_ranges_work_in_either_direction() {
        let view = PvView {
            sans: Vec::new(),
            source: None,
            anchor: 5,
            head: 2,
        };
        assert_eq!(view.range(), (2, 5));
        assert!(view.contains(3));
        assert!(!view.contains(6));
        assert!(view.is_range());
        assert_eq!(view.played(), 2);
        let single = PvView {
            sans: Vec::new(),
            source: None,
            anchor: 3,
            head: 3,
        };
        assert!(!single.is_range());
        assert_eq!(single.played(), 4);
    }
}
