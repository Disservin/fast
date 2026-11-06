//! The chess board: rendering, click and drag moves, arrows, circles, promotion.

use std::{collections::HashSet, time::Duration};

use chess_core::{
    Chess, Color, Move, Position, Role, Square,
    shakmaty::{File, Piece, Rank},
};
use gpui::{
    Animation, AnimationExt as _, App, Bounds, Context, ElementId, EventEmitter, FontWeight,
    IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PathBuilder, Pixels,
    Point, Render, SharedString, Styled, Window, canvas, deferred, div, img, point, prelude::*, px,
    rgb, svg,
};

use crate::theme::{MONO, SANS, T};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BoardTheme {
    #[default]
    Stone,
    Walnut,
    Moss,
    Graphite,
}

impl BoardTheme {
    pub const ALL: [Self; 4] = [Self::Stone, Self::Walnut, Self::Moss, Self::Graphite];

    pub fn colors(self) -> (u32, u32) {
        match self {
            Self::Stone => (0xDCD8D0, 0x9A948A),
            Self::Walnut => (0xE9D7B4, 0xA97A50),
            Self::Moss => (0xE6E9D8, 0x7F956B),
            Self::Graphite => (0xC9CBCE, 0x55595F),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Stone => "Stone",
            Self::Walnut => "Walnut",
            Self::Moss => "Moss",
            Self::Graphite => "Graphite",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PieceSet {
    #[default]
    Classic,
    Letters,
    Flat,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CoordinateStyle {
    #[default]
    OnSquares,
    Outside,
    Off,
}

/// User preferences that affect board rendering and input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BoardPrefs {
    pub theme: BoardTheme,
    pub pieces: PieceSet,
    pub coordinates: CoordinateStyle,
    pub animation_ms: u32,
    pub show_legal: bool,
    pub highlight_last: bool,
    pub auto_queen: bool,
}

impl Default for BoardPrefs {
    fn default() -> Self {
        Self {
            theme: BoardTheme::Stone,
            pieces: PieceSet::Classic,
            coordinates: CoordinateStyle::OnSquares,
            animation_ms: 150,
            show_legal: true,
            highlight_last: true,
            auto_queen: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Arrow {
    pub from: Square,
    pub to: Square,
}

pub enum BoardEvent {
    Moved(Move),
}

struct Drag {
    from: Square,
    piece: Piece,
    position: Point<Pixels>,
    moved: bool,
}

struct Promotion {
    from: Square,
    to: Square,
}

pub struct BoardView {
    id: ElementId,
    pos: Chess,
    prefs: BoardPrefs,
    flipped: bool,
    interactive: bool,
    /// Restricts which side may move (`None` = whoever's turn it is).
    player: Option<Color>,
    last_move: Option<(Square, Square)>,
    /// Arrows set by the owning screen (engine or table selection).
    arrows: Vec<Arrow>,
    user_arrows: Vec<Arrow>,
    circles: HashSet<Square>,
    selected: Option<Square>,
    drag: Option<Drag>,
    right_from: Option<Square>,
    promotion: Option<Promotion>,
    bounds: Bounds<Pixels>,
    animate_serial: u64,
    animate: Option<(Square, Square)>,
}

impl EventEmitter<BoardEvent> for BoardView {}

impl BoardView {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            pos: Chess::default(),
            prefs: BoardPrefs::default(),
            flipped: false,
            interactive: true,
            player: None,
            last_move: None,
            arrows: Vec::new(),
            user_arrows: Vec::new(),
            circles: HashSet::new(),
            selected: None,
            drag: None,
            right_from: None,
            promotion: None,
            bounds: Bounds::default(),
            animate_serial: 0,
            animate: None,
        }
    }

    pub fn read_only(mut self) -> Self {
        self.interactive = false;
        self
    }

    pub fn is_flipped(&self) -> bool {
        self.flipped
    }

    pub fn set_flipped(&mut self, flipped: bool, cx: &mut Context<Self>) {
        self.flipped = flipped;
        cx.notify();
    }

    pub fn set_prefs(&mut self, prefs: BoardPrefs, cx: &mut Context<Self>) {
        if self.prefs != prefs {
            self.prefs = prefs;
            cx.notify();
        }
    }

    pub fn set_player(&mut self, player: Option<Color>, cx: &mut Context<Self>) {
        self.player = player;
        cx.notify();
    }

    pub fn set_interactive(&mut self, interactive: bool, cx: &mut Context<Self>) {
        self.interactive = interactive;
        cx.notify();
    }

    pub fn set_arrows(&mut self, arrows: Vec<Arrow>, cx: &mut Context<Self>) {
        if self.arrows != arrows {
            self.arrows = arrows;
            cx.notify();
        }
    }

    /// Updates the position. Non-adjacent jumps pass `animate = false`.
    pub fn set_position(
        &mut self,
        pos: Chess,
        last_move: Option<(Square, Square)>,
        animate: bool,
        cx: &mut Context<Self>,
    ) {
        self.pos = pos;
        self.last_move = last_move;
        self.selected = None;
        self.drag = None;
        self.promotion = None;
        self.user_arrows.clear();
        self.circles.clear();
        if animate && self.prefs.animation_ms > 0 {
            self.animate = last_move;
            self.animate_serial += 1;
        } else {
            self.animate = None;
        }
        cx.notify();
    }

    fn square_at(&self, position: Point<Pixels>) -> Option<Square> {
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

    fn can_move_piece(&self, square: Square) -> bool {
        if !self.interactive {
            return false;
        }
        let turn = self.pos.turn();
        self.player.is_none_or(|player| player == turn)
            && self.pos.board().color_at(square) == Some(turn)
    }

    fn legal_targets(&self, from: Square) -> Vec<Square> {
        self.pos
            .legal_moves()
            .iter()
            .filter(|mv| mv.from() == Some(from))
            .map(board_target)
            .collect()
    }

    fn attempt_move(&mut self, from: Square, to: Square, cx: &mut Context<Self>) -> bool {
        let candidates: Vec<Move> = self
            .pos
            .legal_moves()
            .iter()
            .filter(|mv| mv.from() == Some(from) && board_target(mv) == to)
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

    fn mouse_down(&mut self, event: &MouseDownEvent, cx: &mut Context<Self>) {
        let Some(square) = self.square_at(event.position) else {
            return;
        };
        if self.promotion.is_some() {
            return;
        }
        match event.button {
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

    fn mouse_move(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        if let Some(drag) = &mut self.drag {
            drag.position = event.position;
            drag.moved = true;
            cx.notify();
        }
    }

    fn mouse_up(&mut self, event: &MouseUpEvent, cx: &mut Context<Self>) {
        match event.button {
            MouseButton::Left => {
                let Some(drag) = self.drag.take() else {
                    return;
                };
                if drag.moved
                    && let Some(target) = self.square_at(event.position)
                    && target != drag.from
                {
                    self.attempt_move(drag.from, target, cx);
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
                    let arrow = Arrow { from, to };
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

    fn cell_origin(&self, square: Square, cell: Pixels) -> Point<Pixels> {
        let (x, y) = self.grid(square);
        point(cell * x as f32, cell * y as f32)
    }

    /// Grid coordinates of a square given orientation, y from the top.
    fn grid(&self, square: Square) -> (u32, u32) {
        let file = u32::from(square.file());
        let rank = u32::from(square.rank());
        if self.flipped {
            (7 - file, rank)
        } else {
            (file, 7 - rank)
        }
    }

    fn piece_element(
        &self,
        piece: Piece,
        cell: Pixels,
        id: impl Into<ElementId>,
    ) -> gpui::AnyElement {
        let name = piece_file(piece);
        match self.prefs.pieces {
            PieceSet::Classic => img(SharedString::from(format!("pieces/merida/{name}.svg")))
                .id(id)
                .size(cell * 0.8)
                .m(cell * 0.1)
                .into_any_element(),
            PieceSet::Flat => svg()
                .path(SharedString::from(format!("pieces/merida/{name}.svg")))
                .size(cell * 0.86)
                .m(cell * 0.07)
                .text_color(rgb(if piece.color == Color::White {
                    0xFFFFFF
                } else {
                    0x14161A
                }))
                .into_any_element(),
            PieceSet::Letters => div()
                .size(cell)
                .flex()
                .items_center()
                .justify_center()
                .font_family(SANS)
                .font_weight(FontWeight::BOLD)
                .text_size(cell * 0.62)
                .text_color(rgb(if piece.color == Color::White {
                    0xFFFFFF
                } else {
                    0x14161A
                }))
                .child(piece.role.upper_char().to_string())
                .into_any_element(),
        }
    }
}

fn board_target(mv: &Move) -> Square {
    // Castling is entered by moving the king two squares.
    if let Move::Castle { king, rook } = mv {
        let file = if rook.file() > king.file() {
            File::G
        } else {
            File::C
        };
        Square::from_coords(file, king.rank())
    } else {
        mv.to()
    }
}

fn piece_file(piece: Piece) -> String {
    format!(
        "{}{}",
        if piece.color == Color::White {
            'w'
        } else {
            'b'
        },
        piece.role.upper_char()
    )
}

fn arrow_path(from: Point<Pixels>, to: Point<Pixels>, cell: Pixels) -> Option<gpui::Path<Pixels>> {
    let dx = f32::from(to.x - from.x);
    let dy = f32::from(to.y - from.y);
    let length = (dx * dx + dy * dy).sqrt();
    if length < 1.0 {
        return None;
    }
    let unit = (dx / length, dy / length);
    let normal = (-unit.1, unit.0);
    let cell = f32::from(cell);
    let shaft = cell * 0.095;
    let head_half = cell * 0.27;
    let head_len = cell * 0.56;
    let at = |along: f32, across: f32| {
        point(
            from.x + px(unit.0 * along + normal.0 * across),
            from.y + px(unit.1 * along + normal.1 * across),
        )
    };
    let start = cell * 0.18;
    let neck = length - head_len;
    let mut builder = PathBuilder::fill();
    builder.add_polygon(
        &[
            at(start, -shaft),
            at(neck, -shaft),
            at(neck, -head_half),
            at(length, 0.0),
            at(neck, head_half),
            at(neck, shaft),
            at(start, shaft),
        ],
        true,
    );
    builder.build().ok()
}

impl Render for BoardView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        // The parent decides the element size; this reads the last laid-out bounds.
        let cell = (self.bounds.size.width / 8.).max(px(8.));
        let (light, dark) = self.prefs.theme.colors();
        let hidden = self
            .drag
            .as_ref()
            .filter(|drag| drag.moved)
            .map(|drag| drag.from);
        let targets: Vec<Square> = if self.prefs.show_legal && self.interactive {
            self.selected
                .map(|from| self.legal_targets(from))
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        let mut root = div()
            .id(self.id.clone())
            .relative()
            .size_full()
            .border_1()
            .border_color(rgb(T.border_strong));
        let last = self.last_move.filter(|_| self.prefs.highlight_last);
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
                    (true, true) => T.last_move_light,
                    (false, true) => T.last_move_dark,
                };
                let coordinate_color = if is_light { 0x5C5851 } else { 0xF0EEE9 };
                let mut sq = div().relative().flex_1().bg(rgb(color));
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
        root = root.child(grid);

        // Pieces, absolutely positioned so moves can animate.
        let mut pieces_el = div().absolute().inset_0();
        for (square, piece) in self.pos.board().clone() {
            if Some(square) == hidden {
                continue;
            }
            let origin = self.cell_origin(square, cell);
            let wrapper = div()
                .absolute()
                .size(cell)
                .left(origin.x)
                .top(origin.y)
                .child(self.piece_element(piece, cell, ("piece", u32::from(square))));
            match self.animate.filter(|(_, to)| *to == square) {
                Some((from, _)) => {
                    let start = self.cell_origin(from, cell);
                    let duration = Duration::from_millis(u64::from(self.prefs.animation_ms));
                    pieces_el = pieces_el.child(wrapper.with_animation(
                        ElementId::Name(format!("move-{}", self.animate_serial).into()),
                        Animation::new(duration).with_easing(gpui::ease_out_quint()),
                        move |el, delta| {
                            el.left(start.x + (origin.x - start.x) * delta)
                                .top(start.y + (origin.y - start.y) * delta)
                        },
                    ));
                }
                None => pieces_el = pieces_el.child(wrapper),
            }
        }
        root = root.child(pieces_el);

        // Legal move hints over pieces.
        for target in targets {
            let origin = self.cell_origin(target, cell);
            let occupied = self.pos.board().piece_at(target).is_some();
            let hint = div()
                .absolute()
                .left(origin.x)
                .top(origin.y)
                .size(cell)
                .flex()
                .items_center()
                .justify_center()
                .child(if occupied {
                    div()
                        .size(cell)
                        .rounded_full()
                        .border(cell * 0.08)
                        .border_color(gpui::hsla(0., 0., 0.08, 0.42))
                } else {
                    div()
                        .size(cell * 0.28)
                        .rounded_full()
                        .bg(gpui::hsla(0., 0., 0.08, 0.42))
                });
            root = root.child(hint);
        }

        // Arrows and pointer capture. Listeners are window-level so drags keep
        // tracking outside the board.
        let mut arrows: Vec<Arrow> = self.arrows.clone();
        arrows.extend(self.user_arrows.iter().copied());
        let centers: Vec<(Point<Pixels>, Point<Pixels>)> = arrows
            .iter()
            .map(|arrow| {
                let half = point(cell / 2., cell / 2.);
                (
                    self.cell_origin(arrow.from, cell) + half,
                    self.cell_origin(arrow.to, cell) + half,
                )
            })
            .collect();
        let capture = entity.clone();
        root = root.child(
            canvas(
                move |bounds, _, cx| {
                    capture.update(cx, |board, cx| {
                        if board.bounds != bounds {
                            let first_layout = board.bounds.size != bounds.size;
                            board.bounds = bounds;
                            if first_layout {
                                cx.notify();
                            }
                        }
                    });
                },
                {
                    let entity = entity.clone();
                    let interactive = self.interactive;
                    move |bounds, _, window, cx| {
                        let color: gpui::Hsla = {
                            let mut color: gpui::Hsla = rgb(T.arrow).into();
                            color.a = 0.88;
                            color
                        };
                        let cell = bounds.size.width / 8.;
                        for (from, to) in &centers {
                            if let Some(path) =
                                arrow_path(bounds.origin + *from, bounds.origin + *to, cell)
                            {
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
            .inset_0(),
        );

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
        let _ = window;
        root
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
        let mut column = div()
            .absolute()
            .left(cell * x as f32)
            .top(if from_top { px(0.) } else { cell * 4. })
            .w(cell)
            .flex()
            .flex_col()
            .bg(rgb(T.surface))
            .border_1()
            .border_color(rgb(T.fg));
        let order = [Role::Queen, Role::Knight, Role::Rook, Role::Bishop];
        for role in order {
            let piece = Piece { color, role };
            column = column.child(
                div()
                    .id(("promote", role as u32))
                    .size(cell)
                    .cursor_pointer()
                    .hover(|style| style.bg(rgb(T.hover)))
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
                    )
                    .child(self.piece_element(piece, cell, ("promotion-piece", role as u32))),
            );
        }
        column
    }
}

fn outside_coordinates(flipped: bool, cell: Pixels) -> impl IntoElement {
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
                    .text_size(px(11.))
                    .font_family(MONO)
                    .text_color(rgb(T.muted))
                    .child(rank.to_string()),
            )
            .child(
                div()
                    .absolute()
                    .bottom(px(-16.))
                    .left(cell * index as f32 + cell / 2. - px(3.))
                    .text_size(px(11.))
                    .font_family(MONO)
                    .text_color(rgb(T.muted))
                    .child(((b'a' + file as u8) as char).to_string()),
            );
    }
    layer
}

/// A 2×2 swatch of a board theme for Settings.
pub fn theme_swatch(theme: BoardTheme, size: Pixels, _cx: &App) -> impl IntoElement {
    let (light, dark) = theme.colors();
    let half = size / 2.;
    let square = |color| div().size(half).bg(rgb(color));
    div()
        .flex()
        .flex_col()
        .size(size)
        .child(div().flex().child(square(light)).child(square(dark)))
        .child(div().flex().child(square(dark)).child(square(light)))
}
