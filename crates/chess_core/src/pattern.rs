//! Exact pawn structures and material counts for local game searches.

use shakmaty::{
    Chess, Color, Piece, Position, Role,
    zobrist::{Zobrist64, ZobristValue},
};

/// The pawns' coloured squares and each side's piece counts, independent of turn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PositionPattern {
    pub pawn_hash: u64,
    pub material: u64,
}

impl PositionPattern {
    /// Describes the position without castling rights, move counters or side to move.
    pub fn of(pos: &Chess) -> Self {
        let mut pawn_hash = 0;
        let mut material = 0;
        let mut shift = 0;
        for color in [Color::White, Color::Black] {
            for role in [
                Role::Pawn,
                Role::Knight,
                Role::Bishop,
                Role::Rook,
                Role::Queen,
            ] {
                let squares = pos.board().by_color(color) & pos.board().by_role(role);
                material |= (squares.count() as u64) << shift;
                shift += 6;
                if role == Role::Pawn {
                    for square in squares {
                        pawn_hash ^= Zobrist64::zobrist_for_piece(square, Piece { color, role }).0;
                    }
                }
            }
        }
        Self {
            pawn_hash,
            material,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::GameTree;

    #[test]
    fn structures_ignore_pieces_turn_and_castling_but_count_captures() {
        let mut tree = GameTree::new();
        let start = PositionPattern::of(&tree.node(GameTree::ROOT).pos);
        let knight = tree.add_san(GameTree::ROOT, "Nf3").unwrap();
        assert_eq!(PositionPattern::of(&tree.node(knight).pos), start);
        let pawn = tree.add_san(knight, "d5").unwrap();
        let after_pawn = PositionPattern::of(&tree.node(pawn).pos);
        assert_ne!(after_pawn.pawn_hash, start.pawn_hash);
        assert_eq!(after_pawn.material, start.material);
        let capture = tree.add_san(pawn, "e4").unwrap();
        let capture = tree.add_san(capture, "dxe4").unwrap();
        assert_ne!(
            PositionPattern::of(&tree.node(capture).pos).material,
            start.material
        );
    }
}
