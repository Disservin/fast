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
    pub anchor: usize,
    pub head: usize,
}

impl PvView {
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
        self.pv_view = Some(PvView {
            sans,
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

    /// Hovering a move draws it on the board and marks it in the line.
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
        let arrows: Vec<Arrow> = squares
            .map(|(from, to)| Arrow::new(from, to))
            .into_iter()
            .collect();
        self.board
            .update(cx, |board, cx| board.set_arrows(arrows, cx));
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
            .text_size(px(14.))
            .line_height(px(26.))
            .text_color(rgb(T.fg))
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
        let back_this = cx.entity();
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
                        .child(
                            div()
                                .flex_1()
                                .text_size(px(12.))
                                .text_color(rgb(T.muted))
                                .child(ui::i18n::t(
                                    "Drag across moves to highlight. Ctrl+C copies, Esc returns.",
                                )),
                        )
                        .child(
                            btn("pv-back", "Back to game")
                                .compact()
                                .on_click(move |_, cx| {
                                    back_this.update(cx, |screen, cx| screen.exit_pv_preview(cx));
                                }),
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
