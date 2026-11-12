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

mod arrows;
mod input;
mod moves;
mod pieces;
mod render;

pub use self::{arrows::*, moves::*, pieces::*};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BoardTheme {
    /// Configs from before the Blue board named its predecessor "stone".
    #[serde(alias = "stone")]
    Blue,
    Walnut,
    #[default]
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
            // Dark palettes use muted squares; light palettes retain brighter colors.
            Self::Blue => (0xA7B1B8, 0x687E89),
            Self::Walnut => (0xE9D7B4, 0xA97A50),
            Self::Moss => (0xB1B69F, 0x607451),
            Self::Graphite => (0x8C9096, 0x42464C),
            Self::Paper => (0xEAF0F5, 0x91A5BF),
            Self::Plum => (0xB7A5B7, 0x775B7C),
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PieceSet {
    /// Merida.
    #[default]
    Classic,
    Cburnett,
    Chessnut,
    #[serde(rename = "mpchess")]
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
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
            theme: BoardTheme::Moss,
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
            Self::Brilliant => ("!!", "Brilliant", 0x00bfa5),
            Self::Inaccuracy => ("?!", "Inaccuracy", 0xf4b400),
            Self::Mistake => ("?", "Mistake", 0xff8c32),
            Self::Blunder => ("??", "Blunder", 0xf04464),
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
    animate: Vec<(Square, Square)>,
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
            animate: Vec::new(),
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
        let transitions = move_transitions(&self.pos, &pos, last_move);
        self.pos = pos;
        self.last_move = transitions.first().copied().or(last_move);
        self.move_tag = None;
        self.selected = None;
        self.drag = None;
        self.promotion = None;
        self.user_arrows.clear();
        self.circles.clear();
        if animate && self.prefs.animation_ms > 0 {
            self.animate = transitions;
            self.animate_serial += 1;
        } else {
            self.animate.clear();
        }
        cx.notify();
    }
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
    fn chess960_castling_animates_both_pieces_to_their_final_squares() {
        for (fen, targets) in [
            (
                "4k3/8/8/8/8/8/8/R4KR1 w GA - 0 1",
                vec![(Square::F1, Square::G1), (Square::G1, Square::F1)],
            ),
            (
                "4k3/8/8/8/8/8/8/R5KR w HA - 0 1",
                vec![(Square::G1, Square::G1), (Square::H1, Square::F1)],
            ),
        ] {
            let mut tree = chess_core::GameTree::from_fen(fen).unwrap();
            let next = tree.add_san(chess_core::GameTree::ROOT, "O-O").unwrap();
            let mv = tree.node(next).mv.unwrap();
            assert_eq!(
                move_transitions(
                    &tree.node(chess_core::GameTree::ROOT).pos,
                    &tree.node(next).pos,
                    Some((mv.from().unwrap(), mv.to()))
                ),
                targets
            );
        }
    }

    #[test]
    fn chess960_king_step_and_castling_have_distinct_targets() {
        let tree = chess_core::GameTree::from_fen("4k3/8/8/8/8/8/8/R4K1R w HA - 0 1").unwrap();
        let pos = &tree.node(chess_core::GameTree::ROOT).pos;
        let step = premove_move(pos, Square::F1, Square::G1).unwrap();
        let castle = premove_move(pos, Square::F1, Square::H1).unwrap();
        assert!(!matches!(step, Move::Castle { .. }));
        assert!(matches!(castle, Move::Castle { .. }));
        assert!(!unambiguous_target(pos, &castle, Square::G1));
        assert!(unambiguous_target(pos, &castle, Square::H1));
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
