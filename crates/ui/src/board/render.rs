//! Drawing the board: squares, highlights, pieces, arrows and the promotion picker.

use super::*;
use crate::widgets::StyleExt as _;
use gpui::{Div, Entity};

/// Lichess' premove highlight, rgba(20, 30, 85, 0.5).
pub(super) fn premove_tint() -> gpui::Rgba {
    gpui::Rgba {
        r: 20. / 255.,
        g: 30. / 255.,
        b: 85. / 255.,
        a: 0.5,
    }
}

impl Render for BoardView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        // The parent decides the element size; this reads the last laid-out bounds.
        let cell = (self.bounds.size.width / 8.).max(px(8.));
        let hidden = self
            .drag
            .as_ref()
            .filter(|drag| drag.moved)
            .map(|drag| drag.from);
        let editing = self.edit.is_some();
        let checked = (!editing).then(|| checked_king(&self.pos)).flatten();
        let targets: Vec<Square> = if self.prefs.show_legal && self.interactive && !editing {
            self.selected
                .map(|from| {
                    if self.premove_mode() {
                        premove_targets(&self.pos, from)
                    } else {
                        self.legal_targets(from)
                    }
                })
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        let last = self
            .last_move
            .filter(|_| self.prefs.highlight_last && !editing);
        let mut root = div()
            .id(self.id.clone())
            .relative()
            .size_full()
            // One device pixel, so the squares inside start on a pixel boundary.
            .border(px(1.0 / window.scale_factor()))
            .border_color(rgb(T.border_strong))
            .child(self.render_squares(last, checked))
            .child(self.render_pieces(cell, hidden))
            .children(self.render_hints(cell, targets))
            .child(self.render_arrow_canvas(entity, editing))
            .children((!editing).then(|| self.render_move_tag(cell)).flatten());
        if self.prefs.coordinates == CoordinateStyle::Outside {
            root = root.child(outside_coordinates(self.flipped, cell));
        }
        // The dragged piece floats above everything else.
        if let Some(drag) = self.drag.as_ref().filter(|drag| drag.moved) {
            let offset = drag.position - self.bounds.origin - point(cell / 2., cell / 2.);
            root = root.child(deferred(
                div()
                    .absolute()
                    .left(offset.x)
                    .top(offset.y)
                    .size(cell)
                    .child(self.piece_element(drag.piece, cell, "dragged")),
            ));
        }

        if let Some(promotion) = &self.promotion {
            root = root.child(deferred(self.promotion_picker(promotion, cell, cx)));
        }
        root
    }
}

impl BoardView {
    /// The 64 squares with their highlights (last move, selection, premove, check),
    /// circles and, when chosen, coordinates drawn on the edge squares.
    fn render_squares(&self, last: Option<(Square, Square)>, checked: Option<Square>) -> Div {
        let (light, dark) = self.prefs.theme.colors();
        let editing = self.edit.is_some();
        let on_squares = self.prefs.coordinates == CoordinateStyle::OnSquares;
        let mut grid = div().absolute().inset_0().flex().flex_col();
        for y in 0..8u32 {
            let mut row = div().flex().flex_1().flex_row();
            for x in 0..8u32 {
                let (file, rank) = if self.flipped { (7 - x, y) } else { (x, 7 - y) };
                let square = Square::from_coords(File::new(file), Rank::new(rank));
                let is_light = (file + rank) % 2 == 1;
                let highlighted = last.is_some_and(|(from, to)| from == square || to == square);
                let color = match (is_light, highlighted) {
                    (true, false) => light,
                    (false, false) => dark,
                    (true, true) => last_move_color(light),
                    (false, true) => last_move_color(dark),
                };
                // Like Lichess: each coordinate takes the other square colour.
                let coordinate_color = if is_light { dark } else { light };
                let mut sq = div().relative().flex_1().bg(rgb(color));
                if self.selected == Some(square) && self.interactive && !editing {
                    sq = sq.child(div().absolute().inset_0().bg(dest_green()));
                }
                if self
                    .premove
                    .is_some_and(|(from, to)| from == square || to == square)
                {
                    sq = sq.child(div().absolute().inset_0().bg(premove_tint()));
                }
                if checked == Some(square) {
                    sq = sq.child(
                        div()
                            .absolute()
                            .inset_0()
                            .bg(rgba(0xD93636B8))
                            .border_2()
                            .border_color(rgba(0xF46B6BE0)),
                    );
                }
                if on_squares && x == 0 {
                    sq = sq.child(
                        div()
                            .absolute()
                            .top(px(3.))
                            .left(px(5.))
                            .font_family(SANS)
                            .text_size(px(11.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(coordinate_color))
                            .child((rank + 1).to_string()),
                    );
                }
                if on_squares && y == 7 {
                    sq = sq.child(
                        div()
                            .absolute()
                            .bottom(px(2.))
                            .right(px(5.))
                            .font_family(SANS)
                            .text_size(px(11.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(coordinate_color))
                            .child(((b'a' + file as u8) as char).to_string()),
                    );
                }
                if self.circles.contains(&square) {
                    sq = sq.child(
                        div()
                            .absolute()
                            .inset(px(3.))
                            .rounded_full()
                            .border(px(3.))
                            .border_color(rgb(T.arrow)),
                    );
                }
                row = row.child(sq);
            }
            grid = grid.child(row);
        }
        grid
    }

    /// The pieces, absolutely positioned so moves can animate. `hidden` is the square of
    /// a piece being dragged, which is drawn under the pointer instead.
    fn render_pieces(&self, cell: Pixels, hidden: Option<Square>) -> Div {
        let mut pieces_el = div().absolute().inset_0();
        let shown = self
            .edit
            .clone()
            .unwrap_or_else(|| self.pos.board().clone());
        for (square, piece) in shown {
            if Some(square) == hidden {
                continue;
            }
            let (x, y) = self.grid(square);
            let wrapper = div()
                .absolute()
                .size(relative(CELL))
                .left(relative(x as f32 * CELL))
                .top(relative(y as f32 * CELL))
                .child(self.piece_element(piece, cell, ("piece", u32::from(square))));
            match self.animate.iter().find(|(_, to)| *to == square).copied() {
                Some((from, _)) => {
                    let (from_x, from_y) = self.grid(from);
                    let duration = Duration::from_millis(u64::from(self.prefs.animation_ms));
                    pieces_el = pieces_el.child(wrapper.with_animation(
                        ElementId::Name(format!("move-{}-{square}", self.animate_serial).into()),
                        Animation::new(duration).with_easing(gpui::ease_out_quint()),
                        move |el, delta| {
                            let lerp = |start: u32, end: u32| {
                                (start as f32 + (end as f32 - start as f32) * delta) * CELL
                            };
                            el.left(relative(lerp(from_x, x)))
                                .top(relative(lerp(from_y, y)))
                        },
                    ));
                }
                None => pieces_el = pieces_el.child(wrapper),
            }
        }
        pieces_el
    }

    /// Legal-move hints over the pieces: a dot on empty squares, a ring around captures.
    fn render_hints(&self, cell: Pixels, targets: Vec<Square>) -> Vec<Div> {
        let mut hints = Vec::new();
        for target in targets {
            let (x, y) = self.grid(target);
            let occupied = self.pos.board().piece_at(target).is_some();
            let hint = div()
                .absolute()
                .left(relative(x as f32 * CELL))
                .top(relative(y as f32 * CELL))
                .size(relative(CELL))
                .flex()
                .items_center()
                .justify_center()
                // Lichess: a round dot on empty squares, a ring around capturable pieces.
                .child(if occupied {
                    div()
                        .size_full()
                        .rounded_full()
                        .border(cell * 0.09)
                        .border_color(dest_green())
                } else {
                    div().size(relative(0.3)).rounded_full().bg(dest_green())
                });
            hints.push(hint);
        }
        hints
    }

    /// A canvas over the board that paints the arrows and, on an interactive board,
    /// captures the pointer at window level so drags keep tracking outside the board.
    fn render_arrow_canvas(&self, entity: Entity<Self>, editing: bool) -> impl IntoElement {
        let mut arrows: Vec<Arrow> = if editing {
            Vec::new()
        } else {
            self.arrows.clone()
        };
        arrows.extend(self.user_arrows.iter().copied());
        let arrow_cells: Vec<ArrowCells> = arrows
            .iter()
            .map(|arrow| {
                (
                    self.grid(arrow.from),
                    self.grid(arrow.to),
                    arrow.color.unwrap_or(T.arrow),
                    f32::from(arrow.scale) / 1000.,
                )
            })
            .collect();
        let capture = entity.clone();
        let letters = self.prefs.pieces == PieceSet::Letters;
        canvas(
            move |bounds, window, cx| {
                let resized = capture.update(cx, |board, _| {
                    let resized = board.bounds.size != bounds.size;
                    board.bounds = bounds;
                    resized
                });
                // Everything scales proportionally on its own. Only sizes that need
                // real pixels (letter glyphs) wait for the next frame; a notify during
                // this one would be lost.
                if resized && letters {
                    let capture = capture.clone();
                    window.on_next_frame(move |_, cx| capture.update(cx, |_, cx| cx.notify()));
                }
            },
            {
                let entity = entity.clone();
                let interactive = self.interactive;
                move |bounds, _, window, cx| {
                    let cell = bounds.size.width / 8.;
                    let center = |(x, y): (u32, u32)| {
                        bounds.origin + point(cell * (x as f32 + 0.5), cell * (y as f32 + 0.5))
                    };
                    for (from, to, color, scale) in &arrow_cells {
                        if let Some(path) = arrow_path(center(*from), center(*to), cell, *scale) {
                            let mut color: gpui::Hsla = rgb(*color).into();
                            color.a = 0.85;
                            window.paint_path(path, color);
                        }
                    }
                    let _ = cx;
                    if !interactive {
                        return;
                    }
                    let down = entity.clone();
                    window.on_mouse_event(move |event: &MouseDownEvent, phase, _, cx| {
                        if phase.bubble() {
                            down.update(cx, |board, cx| board.mouse_down(event, cx));
                        }
                    });
                    let moved = entity.clone();
                    window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                        if phase.bubble() && moved.read(cx).drag.is_some() {
                            moved.update(cx, |board, cx| board.mouse_move(event, cx));
                        }
                    });
                    let up = entity.clone();
                    window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
                        if phase.bubble() {
                            up.update(cx, |board, cx| board.mouse_up(event, cx));
                        }
                    });
                }
            },
        )
        .absolute()
        .inset_0()
    }

    /// The move-quality badge on the last move's square, such as "?!" or "!!".
    fn render_move_tag(&self, cell: Pixels) -> Option<impl IntoElement + use<>> {
        let (Some(tag), Some((_, to))) = (self.move_tag, self.last_move) else {
            return None;
        };
        let (x, y) = self.grid(to);
        let (glyph, label, color) = tag.appearance();
        Some(
            div()
                .id("move-quality-tag")
                .absolute()
                .left(cell * (x as f32 + 0.65))
                .top(cell * y as f32)
                .size(cell * 0.36)
                .flex()
                .items_center()
                .justify_center()
                .border_2()
                .border_color(rgb(0xffffff))
                .shadow_sm()
                .rounded_full()
                .bg(rgb(color))
                .font_weight(FontWeight::BOLD)
                .text_size(cell * 0.20)
                .text_color(rgb(0xffffff))
                .child(glyph)
                .tooltip(move |window, cx| {
                    gpui_component::tooltip::Tooltip::new(label).build(window, cx)
                }),
        )
    }
}

impl BoardView {
    fn promotion_picker(
        &self,
        promotion: &Promotion,
        cell: Pixels,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let (x, y) = self.grid(promotion.to);
        let color = self.pos.turn();
        let from_top = y == 0;
        let (from, to) = (promotion.from, promotion.to);
        // Light discs keep both colours of piece readable on any board theme.
        let (light, _) = self.prefs.theme.colors();
        let mut column = div()
            .absolute()
            .left(cell * x as f32)
            .top(if from_top { px(0.) } else { cell * 4. })
            .w(cell)
            .flex()
            .flex_col()
            .when(!from_top, |column| column.flex_col_reverse());
        let order = [Role::Queen, Role::Knight, Role::Rook, Role::Bishop];
        for role in order {
            let piece = Piece { color, role };
            column = column.child(
                div()
                    .id(("promote", role as u32))
                    .size(cell)
                    .p(cell * 0.04)
                    .cursor_pointer()
                    .child(
                        div()
                            .id(("promote-disc", role as u32))
                            .size_full()
                            .rounded_full()
                            .bg(rgb(light))
                            .border_1()
                            .border_color(rgb(PROMOTION_RING))
                            .shadow_md()
                            .hover(|style| style.rounded(px(4.)).bg(rgb(T.last_move_light)))
                            .child(self.piece_element(
                                piece,
                                cell,
                                ("promotion-piece", role as u32),
                            )),
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |board, _, _, cx| {
                            cx.stop_propagation();
                            board.promotion = None;
                            let chosen = board
                                .pos
                                .legal_moves()
                                .iter()
                                .find(|mv| {
                                    mv.from() == Some(from)
                                        && mv.to() == to
                                        && mv.promotion() == Some(role)
                                })
                                .cloned();
                            if let Some(mv) = chosen {
                                cx.emit(BoardEvent::Moved(mv));
                            }
                            cx.notify();
                        }),
                    ),
            );
        }
        // The rest of the board dims; clicking it puts the pawn back.
        div()
            .id("promotion-scrim")
            .absolute()
            .inset_0()
            .bg(rgba(PROMOTION_SCRIM))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|board, _, _, cx| {
                    cx.stop_propagation();
                    board.promotion = None;
                    cx.notify();
                }),
            )
            .child(column)
    }
}

pub(super) fn outside_coordinates(flipped: bool, cell: Pixels) -> impl IntoElement {
    let mut layer = div().absolute().inset_0();
    for index in 0..8u32 {
        let file = if flipped { 7 - index } else { index };
        let rank = if flipped { index + 1 } else { 8 - index };
        layer = layer
            .child(
                div()
                    .absolute()
                    .left(px(-16.))
                    .w(px(14.))
                    .top(cell * index as f32 + cell / 2. - px(8.))
                    .text_as(11., T.muted)
                    .font_family(MONO)
                    .child(rank.to_string()),
            )
            .child(
                div()
                    .absolute()
                    .bottom(px(-16.))
                    .left(cell * index as f32 + cell / 2. - px(3.))
                    .text_as(11., T.muted)
                    .font_family(MONO)
                    .child(((b'a' + file as u8) as char).to_string()),
            );
    }
    layer
}

impl BoardView {
    /// Grid coordinates of a square given orientation, y from the top.
    pub(super) fn grid(&self, square: Square) -> (u32, u32) {
        let file = u32::from(square.file());
        let rank = u32::from(square.rank());
        if self.flipped {
            (7 - file, rank)
        } else {
            (file, 7 - rank)
        }
    }
}
