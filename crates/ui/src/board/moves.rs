//! Move rules the board needs on its own: premove targets, piece animations and check.

use super::*;

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
        .filter(|mv| mv.from() == Some(from) && unambiguous_target(pos, mv, to))
        .collect();
    candidates
        .iter()
        .find(|mv| mv.promotion() == Some(Role::Queen))
        .or_else(|| candidates.first())
        .copied()
}

/// Animates both castling pieces to their final squares, including a stationary king.
pub(super) fn move_transitions(
    before: &Chess,
    after: &Chess,
    last: Option<(Square, Square)>,
) -> Vec<(Square, Square)> {
    if let Some((from, to)) = last {
        for mv in before.legal_moves() {
            let Move::Castle { king, rook } = mv else {
                continue;
            };
            if king != from || (rook != to && board_target(&mv) != to) {
                continue;
            }
            let mut played = before.clone();
            played.play_unchecked(mv);
            if played.board() == after.board() && played.turn() == after.turn() {
                let king_to = board_target(&mv);
                let rook_to = Square::from_coords(
                    if rook.file() > king.file() {
                        File::F
                    } else {
                        File::D
                    },
                    king.rank(),
                );
                return vec![(king, king_to), (rook, rook_to)];
            }
        }
    }
    last.into_iter().collect()
}

/// Prefers an ordinary king step when its destination could also mean castling.
pub(super) fn unambiguous_target(pos: &Chess, mv: &Move, to: Square) -> bool {
    makes_move(mv, to)
        && (!matches!(mv, Move::Castle { rook, .. } if *rook != to)
            || !pos.legal_moves().iter().any(|other| {
                other.from() == mv.from()
                    && !matches!(other, Move::Castle { .. })
                    && other.to() == to
            }))
}

/// Whether dropping a piece on `to` makes this move. Castling is entered either by
/// moving the king two squares or by dropping it on its rook, as on Lichess.
pub(super) fn makes_move(mv: &Move, to: Square) -> bool {
    board_target(mv) == to || matches!(mv, Move::Castle { rook, .. } if *rook == to)
}

pub(super) fn board_target(mv: &Move) -> Square {
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

/// Only the side to move can be in check in a legal position.
pub(super) fn checked_king(pos: &Chess) -> Option<Square> {
    if pos.is_check() {
        pos.board().king_of(pos.turn())
    } else {
        None
    }
}
