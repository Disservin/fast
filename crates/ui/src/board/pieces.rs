//! Drawing pieces: their colours, the material strip and the board theme swatch.

use super::*;

/// Whether white pieces are toned down on this board, so they do not glare on its
/// light squares.
pub(super) fn soften_white(theme: BoardTheme) -> bool {
    !matches!(theme, BoardTheme::Walnut | BoardTheme::Paper)
}

/// The share of a square a piece picture fills, here and in board images.
pub const PIECE_SCALE: f32 = 0.86;

/// The fill for one-colour pieces (Letters and Flat) of `colour` on this board.
pub fn piece_colour(theme: BoardTheme, colour: Color) -> u32 {
    match colour {
        Color::White if soften_white(theme) => 0xD0D2D5,
        Color::White => 0xFFFFFF,
        Color::Black => 0x14161A,
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

pub(super) fn piece_file(piece: Piece) -> String {
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

impl BoardView {
    pub(super) fn piece_element(
        &self,
        piece: Piece,
        cell: Pixels,
        id: impl Into<ElementId>,
    ) -> gpui::AnyElement {
        let soften_white = piece.color == Color::White && soften_white(self.prefs.theme);
        let piece_color = piece_colour(self.prefs.theme, piece.color);
        match self.prefs.pieces {
            PieceSet::Classic
            | PieceSet::Cburnett
            | PieceSet::Chessnut
            | PieceSet::MpChess
            | PieceSet::Fantasy => img(self.prefs.pieces.asset(piece))
                .id(id)
                .size(relative(PIECE_SCALE))
                .m(relative((1. - PIECE_SCALE) / 2.))
                .opacity(if soften_white { 0.82 } else { 1.0 })
                .into_any_element(),
            PieceSet::Flat => svg()
                .path(PieceSet::Flat.asset(piece))
                .size(relative(0.92))
                .m(relative(0.04))
                .text_color(rgb(piece_color))
                .into_any_element(),
            PieceSet::Letters => div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .font_family(SANS)
                .font_weight(FontWeight::BOLD)
                .text_size(cell * 0.62)
                .text_color(rgb(piece_color))
                .child(piece.role.upper_char().to_string())
                .into_any_element(),
        }
    }
}
