//! The chess board: rendering, click and drag moves, arrows, circles, promotion.

use std::{collections::HashSet, time::Duration};

use chess_core::{
    Chess, Color, Move, Position, Role, Square,
    shakmaty::{Board, File, Piece, Rank},
};
use gpui::{
    Animation, AnimationExt as _, App, Bounds, Context, ElementId, EventEmitter, FontWeight,
    IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PathBuilder, Pixels,
    Point, Render, SharedString, Styled, Window, canvas, deferred, div, img, point, prelude::*, px,
    relative, rgb, rgba, svg,
};

use crate::theme::{MONO, SANS, T};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BoardTheme {
    #[default]
    Blue,
    Walnut,
    Moss,
    Graphite,
    Paper,
    Plum,
}

impl BoardTheme {
    pub const ALL: [Self; 6] = [
        Self::Blue,
        Self::Walnut,
        Self::Moss,
        Self::Graphite,
        Self::Paper,
        Self::Plum,
    ];

    pub fn colors(self) -> (u32, u32) {
        match self {
            // Lichess' blue board: cool, clear contrast that sits well with the slate UI.
            Self::Blue => (0xDEE3E6, 0x8CA2AD),
            Self::Walnut => (0xE9D7B4, 0xA97A50),
            Self::Moss => (0xE6E9D8, 0x7F956B),
            Self::Graphite => (0xC9CBCE, 0x55595F),
            Self::Paper => (0xEAF0F5, 0x91A5BF),
            Self::Plum => (0xE8D8E8, 0x99759E),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Blue => "Blue",
            Self::Walnut => "Walnut",
            Self::Moss => "Moss",
            Self::Graphite => "Graphite",
            Self::Paper => "Paper",
            Self::Plum => "Plum",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PieceSet {
    /// Merida.
    #[default]
    Classic,
    Cburnett,
    Chessnut,
    MpChess,
    Fantasy,
    Letters,
    Flat,
}

impl PieceSet {
    /// Sets drawn from their own SVG artwork, in the order Settings offers them.
    pub const IMAGES: [Self; 5] = [
        Self::Classic,
        Self::Cburnett,
        Self::Chessnut,
        Self::MpChess,
        Self::Fantasy,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Classic => "Classic",
            Self::Cburnett => "Cburnett",
            Self::Chessnut => "Chessnut",
            Self::MpChess => "MPChess",
            Self::Fantasy => "Fantasy",
            Self::Letters => "Letters",
            Self::Flat => "Flat",
        }
    }

    /// The asset folder with this set's artwork. Letters and Flat have none of their
    /// own; Flat draws Merida's outlines in one colour.
    pub fn folder(self) -> &'static str {
        match self {
            Self::Cburnett => "cburnett",
            Self::Chessnut => "chessnut",
            Self::MpChess => "mpchess",
            Self::Fantasy => "fantasy",
            Self::Classic | Self::Letters | Self::Flat => "merida",
        }
    }

    /// The asset path of a piece's picture in this set.
    pub fn asset(self, piece: Piece) -> SharedString {
        format!("pieces/{}/{}.svg", self.folder(), piece_file(piece)).into()
    }
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
    /// Clicking an engine arrow plays its move, as in Nibbler.
    pub click_arrows: bool,
}

impl Default for BoardPrefs {
    fn default() -> Self {
        Self {
            theme: BoardTheme::Blue,
            pieces: PieceSet::Classic,
            coordinates: CoordinateStyle::OnSquares,
            animation_ms: 150,
            show_legal: true,
            highlight_last: true,
            auto_queen: false,
            click_arrows: true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Arrow {
    pub from: Square,
    pub to: Square,
    /// `0xRRGGBB`; the theme's arrow orange when `None`.
    pub color: Option<u32>,
    /// Thickness and arrowhead size, in thousandths of the normal size.
    pub scale: u16,
}

impl Arrow {
    pub fn new(from: Square, to: Square) -> Self {
        Self {
            from,
            to,
            color: None,
            scale: 1000,
        }
    }

    pub fn with_color(mut self, color: u32) -> Self {
        self.color = Some(color);
        self
    }

    pub fn with_scale(mut self, scale: f32) -> Self {
        self.scale = (scale.clamp(0.25, 1.0) * 1000.).round() as u16;
        self
    }
}

/// Whether a point lies on an arrow drawn between two square centres, in board
/// pixels. See [`arrow_distance`].
pub fn arrow_hit(point: (f32, f32), from: (f32, f32), to: (f32, f32), cell: f32) -> bool {
    arrow_distance(point, from, to, cell).is_some()
}

/// How far a point is from an arrow's centre line, as a share of how far a click may
/// be and still count, or `None` when it misses. The band is much wider than the drawn
/// arrow, half a square across along the shaft and wider at the head, and runs a little
/// past the tip, so a click need not be centred. The origin square is left out, since
/// that is where its piece stands.
pub fn arrow_distance(
    point: (f32, f32),
    from: (f32, f32),
    to: (f32, f32),
    cell: f32,
) -> Option<f32> {
    let (dx, dy) = (to.0 - from.0, to.1 - from.1);
    let length = (dx * dx + dy * dy).sqrt();
    if length < 1.0 {
        return None;
    }
    let (ux, uy) = (dx / length, dy / length);
    let (px, py) = (point.0 - from.0, point.1 - from.1);
    let along = px * ux + py * uy;
    let across = (px * -uy + py * ux).abs();
    if along < cell * 0.5 || along > length + cell * 0.3 {
        return None;
    }
    let head_len = cell * 0.56;
    let reach = if along >= length - head_len {
        cell * 0.36
    } else {
        cell * 0.25
    };
    (across <= reach).then_some(across / reach)
}

/// The largest board size up to `available` whose squares are whole device pixels,
/// with the board's one-device-pixel border on each side. Unsnapped, eight squares
/// share a fractional width and their edges blur or come out a pixel uneven.
pub fn snap_board_size(available: Pixels, scale: f32) -> Pixels {
    let scale = if scale > 0.0 { scale } else { 1.0 };
    let device = (f32::from(available) * scale).floor();
    let cell = ((device - 2.0) / 8.0).floor().max(1.0);
    px((cell * 8.0 + 2.0) / scale)
}

/// An arrow in grid coordinates: from, to and colour.
type ArrowCells = ((u32, u32), (u32, u32), u32, f32);

/// Lichess' last-move tint, rgba(155, 199, 0, 0.41), laid over a square colour.
pub fn last_move_color(base: u32) -> u32 {
    blend(base, 0x9BC700, 0.41)
}

/// Lichess' selected-square and move-destination green, rgba(20, 85, 30, 0.5).
pub const DEST_GREEN: (f32, f32, f32, f32) = (20., 85., 30., 0.5);

/// `top` at opacity `alpha` over `base`, both `0xRRGGBB`.
pub fn blend(base: u32, top: u32, alpha: f32) -> u32 {
    let channel = |shift: u32| {
        let b = ((base >> shift) & 0xFF) as f32;
        let t = ((top >> shift) & 0xFF) as f32;
        ((b + (t - b) * alpha).round() as u32).min(255) << shift
    };
    channel(16) | channel(8) | channel(0)
}

fn dest_green() -> gpui::Rgba {
    let (r, g, b, a) = DEST_GREEN;
    gpui::Rgba {
        r: r / 255.,
        g: g / 255.,
        b: b / 255.,
        a,
    }
}

pub enum BoardEvent {
    Moved(Move),
    /// A move queued for when it is the player's turn again (`None`: cancelled).
    Premove(Option<(Square, Square)>),
    /// A square was clicked while editing: `erase` for the right button.
    Square {
        square: Square,
        erase: bool,
    },
}

/// Dims the board behind the promotion choices.
const PROMOTION_SCRIM: u32 = 0x0B0D1199;
/// Outlines each promotion choice against light squares.
const PROMOTION_RING: u32 = 0x5C5851;

/// One square as a fraction of the board.
const CELL: f32 = 1.0 / 8.0;

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoveTag {
    Brilliant,
    Inaccuracy,
    Mistake,
    Blunder,
}

impl MoveTag {
    pub fn from_nag(nag: u8) -> Option<Self> {
        match nag {
            3 => Some(Self::Brilliant),
            6 => Some(Self::Inaccuracy),
            2 => Some(Self::Mistake),
            4 => Some(Self::Blunder),
            _ => None,
        }
    }

    fn appearance(self) -> (&'static str, &'static str, u32) {
        match self {
            Self::Brilliant => ("!!", "Brilliant", 0x219c9e),
            Self::Inaccuracy => ("?!", "Inaccuracy", 0xd6a431),
            Self::Mistake => ("?", "Mistake", 0xe58a35),
            Self::Blunder => ("??", "Blunder", 0xd94d59),
        }
    }
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
    move_tag: Option<MoveTag>,
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
    /// A board being edited, shown instead of the position. It need not be legal.
    edit: Option<Board>,
    /// The player may queue a move while the opponent is to move.
    premoves: bool,
    /// The queued move, highlighted until the owner plays or drops it.
    premove: Option<(Square, Square)>,
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
            move_tag: None,
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
            edit: None,
            premoves: false,
            premove: None,
        }
    }

    /// Allows queueing a move during the opponent's turn (with a player set).
    pub fn set_premoves(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.premoves = enabled;
        if !enabled {
            self.premove = None;
        }
        cx.notify();
    }

    /// Replaces the highlighted premove, e.g. clearing it once played.
    pub fn set_premove(&mut self, premove: Option<(Square, Square)>, cx: &mut Context<Self>) {
        if self.premove != premove {
            self.premove = premove;
            cx.notify();
        }
    }

    /// The opponent is to move and the player may queue a move meanwhile.
    fn premove_mode(&self) -> bool {
        self.premoves
            && self.interactive
            && self.edit.is_none()
            && self.player.is_some_and(|player| player != self.pos.turn())
    }

    /// A piece that can be picked up: to move now, or to premove.
    fn can_pick(&self, square: Square) -> bool {
        self.can_move_piece(square)
            || (self.premove_mode()
                && self.pos.board().color_at(square).is_some()
                && self.pos.board().color_at(square) == self.player)
    }

    fn queue_premove(&mut self, premove: Option<(Square, Square)>, cx: &mut Context<Self>) {
        self.premove = premove;
        self.selected = None;
        cx.emit(BoardEvent::Premove(premove));
        cx.notify();
    }

    pub fn read_only(mut self) -> Self {
        self.interactive = false;
        self
    }

    pub fn is_flipped(&self) -> bool {
        self.flipped
    }

    /// Shows a board being edited, which may be illegal, and reports clicked squares
    /// instead of moves. `None` goes back to the position.
    pub fn set_editing(&mut self, board: Option<Board>, cx: &mut Context<Self>) {
        self.selected = None;
        self.drag = None;
        self.promotion = None;
        self.user_arrows.clear();
        self.circles.clear();
        self.edit = board;
        cx.notify();
    }

    /// The position being shown.
    pub fn position(&self) -> &Chess {
        &self.pos
    }

    /// The highlighted last move, if highlighting is on.
    pub fn last_move(&self) -> Option<(Square, Square)> {
        self.last_move.filter(|_| self.prefs.highlight_last)
    }

    pub fn set_move_tag(&mut self, tag: Option<MoveTag>, cx: &mut Context<Self>) {
        if self.move_tag != tag {
            self.move_tag = tag;
            cx.notify();
        }
    }

    pub fn prefs(&self) -> BoardPrefs {
        self.prefs
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
        self.move_tag = None;
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
        if !self.interactive || self.edit.is_some() {
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
            .flat_map(|mv| {
                let rook = match mv {
                    Move::Castle { rook, .. } => Some(*rook),
                    _ => None,
                };
                std::iter::once(board_target(mv)).chain(rook)
            })
            .collect()
    }

    fn attempt_move(&mut self, from: Square, to: Square, cx: &mut Context<Self>) -> bool {
        let candidates: Vec<Move> = self
            .pos
            .legal_moves()
            .iter()
            .filter(|mv| mv.from() == Some(from) && makes_move(mv, to))
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
    fn arrow_at(&self, position: Point<Pixels>) -> Option<(Square, Square)> {
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
        match self.prefs.pieces {
            PieceSet::Classic
            | PieceSet::Cburnett
            | PieceSet::Chessnut
            | PieceSet::MpChess
            | PieceSet::Fantasy => img(self.prefs.pieces.asset(piece))
                .id(id)
                .size(relative(0.74))
                .m(relative(0.13))
                .into_any_element(),
            PieceSet::Flat => svg()
                .path(PieceSet::Flat.asset(piece))
                .size(relative(0.86))
                .m(relative(0.07))
                .text_color(rgb(if piece.color == Color::White {
                    0xFFFFFF
                } else {
                    0x14161A
                }))
                .into_any_element(),
            PieceSet::Letters => div()
                .size_full()
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

/// The pieces one side is up on, drawn small in the opponent's colour with `+N` for
/// a lead in points, as beside a Lichess player. Identical pieces overlap a little.
/// Letters and Flat have no small pictures of their own and use Classic's.
pub fn material_strip(
    roles: &[Role],
    pieces_of: Color,
    lead: Option<i32>,
    set: PieceSet,
    size: f32,
) -> gpui::Div {
    let set = if PieceSet::IMAGES.contains(&set) {
        set
    } else {
        PieceSet::Classic
    };
    let mut strip = div().flex().flex_row().items_center().h(px(size));
    let mut previous: Option<Role> = None;
    for role in roles {
        let overlap = previous == Some(*role);
        previous = Some(*role);
        strip = strip.child(
            img(set.asset(Piece {
                color: pieces_of,
                role: *role,
            }))
            .size(px(size))
            .flex_none()
            .when(overlap, |el| el.ml(px(-size * 0.55))),
        );
    }
    strip.children(lead.map(|lead| {
        div()
            .ml(px(4.))
            .font_family(MONO)
            .text_size(px(size * 0.75))
            .text_color(rgb(T.muted))
            .child(format!("+{lead}"))
    }))
}

/// Lichess' premove highlight, rgba(20, 30, 85, 0.5).
fn premove_tint() -> gpui::Rgba {
    gpui::Rgba {
        r: 20. / 255.,
        g: 30. / 255.,
        b: 85. / 255.,
        a: 0.5,
    }
}

/// Where a piece could be premoved: anywhere it could reach on an empty board, as on
/// Lichess, since the position will have changed by the time the move is played.
/// Squares of the player's own pieces count, as those may be captured first.
pub fn premove_targets(pos: &Chess, from: Square) -> Vec<Square> {
    use chess_core::shakmaty::{Bitboard, CastlingSide, attacks};
    let Some(piece) = pos.board().piece_at(from) else {
        return Vec::new();
    };
    let empty = Bitboard::EMPTY;
    let mut targets = match piece.role {
        Role::Pawn => {
            let mut squares = attacks::pawn_attacks(piece.color, from);
            let forward = if piece.color.is_white() { 1 } else { -1 };
            let start = if piece.color.is_white() {
                Rank::Second
            } else {
                Rank::Seventh
            };
            if let Some(one) = from.offset(8 * forward) {
                squares.add(one);
                if from.rank() == start
                    && let Some(two) = from.offset(16 * forward)
                {
                    squares.add(two);
                }
            }
            squares
        }
        Role::Knight => attacks::knight_attacks(from),
        Role::Bishop => attacks::bishop_attacks(from, empty),
        Role::Rook => attacks::rook_attacks(from, empty),
        Role::Queen => attacks::queen_attacks(from, empty),
        Role::King => {
            let mut squares = attacks::king_attacks(from);
            let castles = pos.castles();
            for (side, file) in [
                (CastlingSide::KingSide, File::G),
                (CastlingSide::QueenSide, File::C),
            ] {
                if castles.has(piece.color, side)
                    && let Some(rook) = castles.rook(piece.color, side)
                {
                    squares.add(Square::from_coords(file, from.rank()));
                    squares.add(rook);
                }
            }
            squares
        }
    };
    targets.discard(from);
    targets.into_iter().collect()
}

/// The legal move a queued premove stands for in `pos`, if any. Castling may be
/// queued on the king's square or the rook's; a promotion becomes a queen.
pub fn premove_move(pos: &Chess, from: Square, to: Square) -> Option<Move> {
    let candidates: Vec<Move> = pos
        .legal_moves()
        .into_iter()
        .filter(|mv| mv.from() == Some(from) && makes_move(mv, to))
        .collect();
    candidates
        .iter()
        .find(|mv| mv.promotion() == Some(Role::Queen))
        .or_else(|| candidates.first())
        .copied()
}

/// Whether dropping a piece on `to` makes this move. Castling is entered either by
/// moving the king two squares or by dropping it on its rook, as on Lichess.
fn makes_move(mv: &Move, to: Square) -> bool {
    board_target(mv) == to || matches!(mv, Move::Castle { rook, .. } if *rook == to)
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

fn arrow_path(
    from: Point<Pixels>,
    to: Point<Pixels>,
    cell: Pixels,
    scale: f32,
) -> Option<gpui::Path<Pixels>> {
    let dx = f32::from(to.x - from.x);
    let dy = f32::from(to.y - from.y);
    let length = (dx * dx + dy * dy).sqrt();
    if length < 1.0 {
        return None;
    }
    let unit = (dx / length, dy / length);
    let normal = (-unit.1, unit.0);
    let cell = f32::from(cell);
    let shaft = cell * 0.095 * scale;
    let head_half = cell * 0.27 * scale;
    let head_len = cell * 0.56 * scale;
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

/// Only the side to move can be in check in a legal position.
fn checked_king(pos: &Chess) -> Option<Square> {
    if pos.is_check() {
        pos.board().king_of(pos.turn())
    } else {
        None
    }
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
        let mut root = div()
            .id(self.id.clone())
            .relative()
            .size_full()
            // One device pixel, so the squares inside start on a pixel boundary.
            .border(px(1.0 / window.scale_factor()))
            .border_color(rgb(T.border_strong));
        let last = self
            .last_move
            .filter(|_| self.prefs.highlight_last && !editing);
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
        root = root.child(grid);

        // Pieces, absolutely positioned so moves can animate.
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
            match self.animate.filter(|(_, to)| *to == square) {
                Some((from, _)) => {
                    let (from_x, from_y) = self.grid(from);
                    let duration = Duration::from_millis(u64::from(self.prefs.animation_ms));
                    pieces_el = pieces_el.child(wrapper.with_animation(
                        ElementId::Name(format!("move-{}", self.animate_serial).into()),
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
        root = root.child(pieces_el);

        // Legal move hints over pieces.
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
            root = root.child(hint);
        }

        // Arrows and pointer capture. Listeners are window-level so drags keep
        // tracking outside the board.
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
        root = root.child(
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
                            if let Some(path) = arrow_path(center(*from), center(*to), cell, *scale)
                            {
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
            .inset_0(),
        );

        if !editing && let (Some(tag), Some((_, to))) = (self.move_tag, self.last_move) {
            let (x, y) = self.grid(to);
            let (glyph, label, color) = tag.appearance();
            root = root.child(
                div()
                    .id("move-quality-tag")
                    .absolute()
                    .left(cell * (x as f32 + 0.65))
                    .top(cell * y as f32)
                    .px(px(4.))
                    .rounded_full()
                    .bg(rgb(color))
                    .font_weight(FontWeight::BOLD)
                    .text_size(cell * 0.25)
                    .text_color(rgb(0xffffff))
                    .child(glyph)
                    .tooltip(move |window, cx| {
                        gpui_component::tooltip::Tooltip::new(label).build(window, cx)
                    }),
            );
        }

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_highlight_tracks_the_checked_king_and_clears_after_escape() {
        let mut tree = chess_core::GameTree::from_fen("4k3/8/8/8/8/8/4r3/4K3 w - - 0 1").unwrap();
        assert_eq!(
            checked_king(&tree.node(chess_core::GameTree::ROOT).pos),
            Some(Square::E1)
        );
        let escaped = tree.add_san(chess_core::GameTree::ROOT, "Kxe2").unwrap();
        assert_eq!(checked_king(&tree.node(escaped).pos), None);
        let tree = chess_core::GameTree::from_fen("4k3/4R3/8/8/8/8/8/4K3 b - - 0 1").unwrap();
        assert_eq!(
            checked_king(&tree.node(chess_core::GameTree::ROOT).pos),
            Some(Square::E8)
        );
        assert_eq!(checked_king(&Chess::default()), None);
    }

    #[test]
    fn arrows_are_hit_along_the_shaft_and_head_but_not_at_the_origin() {
        let cell = 100.0;
        // e2 to e4 on a board drawn from (0, 0): x centre 450, from y 650 to y 450.
        let (from, to) = ((450.0, 650.0), (450.0, 450.0));
        assert!(arrow_hit((450.0, 450.0), from, to, cell), "head");
        assert!(arrow_hit((470.0, 470.0), from, to, cell), "wide head");
        assert!(arrow_hit((455.0, 550.0), from, to, cell), "shaft");
        assert!(
            arrow_hit((472.0, 560.0), from, to, cell),
            "off-centre shaft"
        );
        assert!(
            arrow_hit((450.0, 425.0), from, to, cell),
            "just past the tip"
        );
        assert!(!arrow_hit((450.0, 640.0), from, to, cell), "origin square");
        assert!(
            !arrow_hit((490.0, 550.0), from, to, cell),
            "beside the shaft"
        );
        assert!(!arrow_hit((450.0, 300.0), from, to, cell), "past the head");
        assert!(!arrow_hit((0.0, 0.0), from, from, cell), "no length");
    }

    #[test]
    fn overlapping_arrows_are_told_apart_by_their_centre_lines() {
        let cell = 100.0;
        // e2-e4 and e2-f4 overlap near e3; a click right of e2-e4's line is nearer e2-f4.
        let (from, straight, diagonal) = ((450.0, 650.0), (450.0, 450.0), (550.0, 450.0));
        let point = (474.0, 560.0);
        let a = arrow_distance(point, from, straight, cell).unwrap();
        let b = arrow_distance(point, from, diagonal, cell).unwrap();
        assert!(b < a);
    }

    #[test]
    fn snapped_boards_have_whole_pixel_squares() {
        for scale in [1.0f32, 1.25, 1.5, 2.0] {
            for available in [249.0f32, 320.0, 561.0, 562.0, 577.3, 1003.0] {
                let size = f32::from(snap_board_size(px(available), scale));
                assert!(size <= available + 1e-3, "{available} at {scale}");
                let squares = (size * scale - 2.0) / 8.0;
                assert!(
                    (squares - squares.round()).abs() < 1e-3,
                    "{available} at {scale}"
                );
                assert!(available - size < (8.0 + 2.0) / scale + 1e-3);
            }
        }
        assert_eq!(snap_board_size(px(562.), 1.0), px(562.));
        assert_eq!(snap_board_size(px(249.), 1.0), px(242.));
    }

    fn fen(text: &str) -> Chess {
        text.parse::<chess_core::shakmaty::fen::Fen>()
            .unwrap()
            .into_position(chess_core::shakmaty::CastlingMode::Standard)
            .unwrap()
    }

    #[test]
    fn premoves_reach_like_on_an_empty_board() {
        // Black to move; White queues moves meanwhile.
        let pos = fen("rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1");
        let bishop = premove_targets(&pos, Square::F1);
        // Its own pawn on e2 does not block the premove: it may have moved by then.
        assert!(bishop.contains(&Square::E2) && bishop.contains(&Square::A6));
        let pawn = premove_targets(&pos, Square::D2);
        assert!(pawn.contains(&Square::D3) && pawn.contains(&Square::D4));
        assert!(pawn.contains(&Square::C3) && pawn.contains(&Square::E3));
        assert!(!pawn.contains(&Square::D5));
        let king = premove_targets(&pos, Square::E1);
        assert!(king.contains(&Square::G1) && king.contains(&Square::H1));
        assert!(king.contains(&Square::C1) && king.contains(&Square::A1));
        assert!(!king.contains(&Square::E1));
        assert!(premove_targets(&pos, Square::E5).is_empty());
    }

    #[test]
    fn a_queued_premove_plays_only_if_legal_then() {
        let pos = fen("rnbqkbnr/pppp1ppp/8/4p3/4P3/8/PPPP1PPP/RNBQKBNR w KQkq - 0 2");
        assert!(premove_move(&pos, Square::G1, Square::F3).is_some());
        // Blocked by its own pawn now: dropped.
        assert!(premove_move(&pos, Square::E4, Square::E5).is_none());
        let promote = fen("8/4P3/8/8/8/8/k7/4K3 w - - 0 1");
        assert_eq!(
            premove_move(&promote, Square::E7, Square::E8).and_then(|mv| mv.promotion()),
            Some(Role::Queen)
        );
    }

    #[test]
    fn castling_takes_the_king_square_or_the_rook() {
        let pos: Chess = "r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1"
            .parse::<chess_core::shakmaty::fen::Fen>()
            .unwrap()
            .into_position(chess_core::shakmaty::CastlingMode::Standard)
            .unwrap();
        let castles: Vec<Move> = pos
            .legal_moves()
            .into_iter()
            .filter(|mv| matches!(mv, Move::Castle { .. }))
            .collect();
        assert_eq!(castles.len(), 2);
        let short = castles
            .iter()
            .find(|mv| matches!(mv, Move::Castle { rook, .. } if *rook == Square::H1))
            .unwrap();
        assert!(makes_move(short, Square::G1));
        assert!(makes_move(short, Square::H1));
        assert!(!makes_move(short, Square::A1));
        assert!(!makes_move(short, Square::F1));
        // A normal rook move to h1 is not mistaken for castling.
        let quiet = Move::Normal {
            role: Role::King,
            from: Square::E1,
            capture: None,
            to: Square::F1,
            promotion: None,
        };
        assert!(!makes_move(&quiet, Square::H1));
        let mut view = BoardView::new("test");
        view.pos = pos;
        let targets = view.legal_targets(Square::E1);
        assert!(targets.contains(&Square::G1) && targets.contains(&Square::H1));
        assert!(targets.contains(&Square::C1) && targets.contains(&Square::A1));
    }

    #[test]
    fn every_piece_set_has_all_twelve_pictures() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets");
        for set in PieceSet::IMAGES {
            for color in [Color::White, Color::Black] {
                for role in Role::ALL {
                    let path = root.join(set.asset(Piece { color, role }).as_ref());
                    let text = std::fs::read_to_string(&path)
                        .unwrap_or_else(|err| panic!("{}: {err}", path.display()));
                    assert!(text.contains("<svg"), "{}", path.display());
                }
            }
        }
    }

    #[test]
    fn last_move_tint_blends_toward_lichess_green_yellow() {
        assert_eq!(blend(0x000000, 0xFFFFFF, 0.5), 0x808080);
        assert_eq!(blend(0x123456, 0xABCDEF, 0.0), 0x123456);
        let tinted = last_move_color(0xDEE3E6);
        // Green rises above red and blue drops, like Lichess' highlight.
        let (r, g, b) = (tinted >> 16, (tinted >> 8) & 0xFF, tinted & 0xFF);
        assert!(g > r && b < 0xE6);
    }
}
