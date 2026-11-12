//! Pointer input: picking up, dragging and dropping pieces, and drawing arrows.

use super::*;

impl BoardView {
    pub(super) fn square_at(&self, position: Point<Pixels>) -> Option<Square> {
        let bounds = self.bounds;
        if !bounds.contains(&position) || bounds.size.width <= px(0.) {
            return None;
        }
        let cell = bounds.size.width / 8.;
        let x = (((position.x - bounds.origin.x) / cell) as u32).min(7);
        let y = (((position.y - bounds.origin.y) / cell) as u32).min(7);
        let (file, rank) = if self.flipped { (7 - x, y) } else { (x, 7 - y) };
        Some(Square::from_coords(File::new(file), Rank::new(rank)))
    }

    pub(super) fn can_move_piece(&self, square: Square) -> bool {
        if !self.interactive || self.edit.is_some() {
            return false;
        }
        let turn = self.pos.turn();
        self.player.is_none_or(|player| player == turn)
            && self.pos.board().color_at(square) == Some(turn)
    }

    pub(super) fn legal_targets(&self, from: Square) -> Vec<Square> {
        self.pos
            .legal_moves()
            .iter()
            .filter(|mv| mv.from() == Some(from))
            .flat_map(|mv| {
                let rook = match mv {
                    Move::Castle { rook, .. } => Some(*rook),
                    _ => None,
                };
                std::iter::once(board_target(mv)).chain(rook)
            })
            .collect()
    }

    pub(super) fn attempt_move(
        &mut self,
        from: Square,
        to: Square,
        cx: &mut Context<Self>,
    ) -> bool {
        let candidates: Vec<Move> = self
            .pos
            .legal_moves()
            .iter()
            .filter(|mv| mv.from() == Some(from) && unambiguous_target(&self.pos, mv, to))
            .cloned()
            .collect();
        match candidates.as_slice() {
            [] => false,
            [mv] => {
                cx.emit(BoardEvent::Moved(*mv));
                self.selected = None;
                true
            }
            many => {
                // Several moves on one target are always promotions.
                if self.prefs.auto_queen
                    && let Some(queen) = many.iter().find(|mv| mv.promotion() == Some(Role::Queen))
                {
                    cx.emit(BoardEvent::Moved(*queen));
                } else {
                    self.promotion = Some(Promotion { from, to });
                }
                self.selected = None;
                true
            }
        }
    }

    pub(super) fn mouse_down(&mut self, event: &MouseDownEvent, cx: &mut Context<Self>) {
        let Some(square) = self.square_at(event.position) else {
            return;
        };
        if self.promotion.is_some() {
            return;
        }
        if self.edit.is_some() {
            if matches!(event.button, MouseButton::Left | MouseButton::Right) {
                let erase = event.button == MouseButton::Right;
                cx.emit(BoardEvent::Square { square, erase });
            }
            return;
        }
        match event.button {
            MouseButton::Left if self.premove_mode() => {
                self.user_arrows.clear();
                self.circles.clear();
                if let Some(from) = self.selected
                    && from != square
                    && premove_targets(&self.pos, from).contains(&square)
                {
                    self.queue_premove(Some((from, square)), cx);
                    return;
                }
                if self.can_pick(square) {
                    if let Some(piece) = self.pos.board().piece_at(square) {
                        self.drag = Some(Drag {
                            from: square,
                            piece,
                            position: event.position,
                            moved: false,
                        });
                    }
                    self.selected = Some(square);
                } else {
                    self.selected = None;
                    if self.premove.is_some() {
                        self.queue_premove(None, cx);
                    }
                }
                cx.notify();
            }
            // A right click drops a queued premove, as on Lichess.
            MouseButton::Right if self.premove.is_some() => {
                self.queue_premove(None, cx);
            }
            MouseButton::Left => {
                self.user_arrows.clear();
                self.circles.clear();
                if let Some(from) = self.selected
                    && from != square
                    && self.attempt_move(from, square, cx)
                {
                    cx.notify();
                    return;
                }
                if !self.can_move_piece(square)
                    && self.prefs.click_arrows
                    && self.interactive
                    && let Some((from, to)) = self.arrow_at(event.position)
                    && self.attempt_move(from, to, cx)
                {
                    cx.notify();
                    return;
                }
                if self.can_move_piece(square) {
                    if let Some(piece) = self.pos.board().piece_at(square) {
                        self.drag = Some(Drag {
                            from: square,
                            piece,
                            position: event.position,
                            moved: false,
                        });
                    }
                    self.selected = Some(square);
                } else {
                    self.selected = None;
                }
                cx.notify();
            }
            MouseButton::Right => {
                self.right_from = Some(square);
            }
            _ => {}
        }
    }

    /// The topmost engine arrow under a point. Arrows drawn by the user are not moves.
    pub(super) fn arrow_at(&self, position: Point<Pixels>) -> Option<(Square, Square)> {
        let cell = f32::from(self.bounds.size.width) / 8.;
        let origin = self.bounds.origin;
        let center = |square: Square| {
            let (x, y) = self.grid(square);
            (
                f32::from(origin.x) + cell * (x as f32 + 0.5),
                f32::from(origin.y) + cell * (y as f32 + 0.5),
            )
        };
        let point = (f32::from(position.x), f32::from(position.y));
        // Where the bands of several arrows overlap, the one whose centre line is
        // nearest wins; on a tie, the last, since it is drawn on top.
        self.arrows
            .iter()
            .enumerate()
            .filter_map(|(index, arrow)| {
                arrow_distance(point, center(arrow.from), center(arrow.to), cell)
                    .map(|distance| (distance, index, arrow))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0).then(b.1.cmp(&a.1)))
            .map(|(_, _, arrow)| (arrow.from, arrow.to))
    }

    pub(super) fn mouse_move(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        if let Some(drag) = &mut self.drag {
            drag.position = event.position;
            drag.moved = true;
            cx.notify();
        }
    }

    pub(super) fn mouse_up(&mut self, event: &MouseUpEvent, cx: &mut Context<Self>) {
        match event.button {
            MouseButton::Left => {
                let Some(drag) = self.drag.take() else {
                    return;
                };
                if drag.moved
                    && let Some(target) = self.square_at(event.position)
                    && target != drag.from
                {
                    if self.premove_mode() {
                        if premove_targets(&self.pos, drag.from).contains(&target) {
                            self.queue_premove(Some((drag.from, target)), cx);
                        }
                    } else {
                        self.attempt_move(drag.from, target, cx);
                    }
                }
                cx.notify();
            }
            MouseButton::Right => {
                let Some(from) = self.right_from.take() else {
                    return;
                };
                let Some(to) = self.square_at(event.position) else {
                    return;
                };
                if from == to {
                    if !self.circles.remove(&to) {
                        self.circles.insert(to);
                    }
                } else {
                    let arrow = Arrow::new(from, to);
                    if let Some(index) = self.user_arrows.iter().position(|a| *a == arrow) {
                        self.user_arrows.remove(index);
                    } else {
                        self.user_arrows.push(arrow);
                    }
                }
                cx.notify();
            }
            _ => {}
        }
    }
}
