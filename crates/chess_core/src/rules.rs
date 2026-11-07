//! How a game ends: checkmate, stalemate and the draw rules that need the game's
//! history (repetition, the fifty- and seventy-five-move rules).

use shakmaty::{Chess, Color, Position};

use crate::{GameTree, NodeId, position_hash};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GameEnd {
    Checkmate {
        winner: Color,
    },
    Stalemate,
    InsufficientMaterial,
    /// The same position for the third time (claimable).
    Threefold,
    /// The same position for the fifth time (automatic).
    Fivefold,
    /// Fifty moves by each side without a capture or pawn move (claimable).
    FiftyMoves,
    /// Seventy-five moves by each side (automatic).
    SeventyFiveMoves,
}

impl GameEnd {
    pub fn is_draw(self) -> bool {
        !matches!(self, Self::Checkmate { .. })
    }

    /// "Draw by threefold repetition", "White wins by checkmate", …
    pub fn describe(self) -> &'static str {
        match self {
            Self::Checkmate {
                winner: Color::White,
            } => "White wins by checkmate",
            Self::Checkmate {
                winner: Color::Black,
            } => "Black wins by checkmate",
            Self::Stalemate => "Stalemate: draw",
            Self::InsufficientMaterial => "Draw by insufficient material",
            Self::Threefold => "Draw by threefold repetition",
            Self::Fivefold => "Draw by fivefold repetition",
            Self::FiftyMoves => "Draw by the fifty-move rule",
            Self::SeventyFiveMoves => "Draw by the seventy-five-move rule",
        }
    }

    /// The PGN result: "1-0", "0-1" or "1/2-1/2".
    pub fn result(self) -> &'static str {
        match self {
            Self::Checkmate {
                winner: Color::White,
            } => "1-0",
            Self::Checkmate {
                winner: Color::Black,
            } => "0-1",
            _ => "1/2-1/2",
        }
    }
}

/// How the game stands at `pos`, given the hashes of every position from the start
/// up to and including `pos` (see [`position_hash`]). Repetition compares side to
/// move, castling rights and legal en passant, as the rules do.
pub fn game_end(pos: &Chess, history: &[u64]) -> Option<GameEnd> {
    if pos.is_checkmate() {
        return Some(GameEnd::Checkmate {
            winner: pos.turn().other(),
        });
    }
    if pos.is_stalemate() {
        return Some(GameEnd::Stalemate);
    }
    if pos.is_insufficient_material() {
        return Some(GameEnd::InsufficientMaterial);
    }
    let hash = position_hash(pos);
    let seen = history
        .iter()
        .filter(|known| **known == hash)
        .count()
        .max(1);
    let halfmoves = pos.halfmoves();
    if seen >= 5 {
        Some(GameEnd::Fivefold)
    } else if halfmoves >= 150 {
        Some(GameEnd::SeventyFiveMoves)
    } else if seen >= 3 {
        Some(GameEnd::Threefold)
    } else if halfmoves >= 100 {
        Some(GameEnd::FiftyMoves)
    } else {
        None
    }
}

impl GameTree {
    /// Hashes of the positions from the root to `id`, both included.
    pub fn history(&self, id: NodeId) -> Vec<u64> {
        std::iter::once(GameTree::ROOT)
            .chain(self.path_to(id))
            .map(|node| position_hash(&self.node(node).pos))
            .collect()
    }

    /// How the game stands after the moves leading to `id`.
    pub fn game_end(&self, id: NodeId) -> Option<GameEnd> {
        game_end(&self.node(id).pos, &self.history(id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn play(moves: &[&str]) -> (GameTree, NodeId) {
        let mut tree = GameTree::new();
        let mut node = GameTree::ROOT;
        for san in moves {
            node = tree.add_san(node, san).unwrap();
        }
        (tree, node)
    }

    #[test]
    fn knights_shuffling_repeat_three_then_five_times() {
        let shuffle = ["Nf3", "Nf6", "Ng1", "Ng8"];
        let (tree, node) = play(&shuffle);
        assert_eq!(tree.game_end(node), None, "start position seen twice");
        let twice: Vec<&str> = shuffle.iter().chain(&shuffle).copied().collect();
        let (tree, node) = play(&twice);
        assert_eq!(tree.game_end(node), Some(GameEnd::Threefold));
        assert!(GameEnd::Threefold.is_draw());
        let four: Vec<&str> = twice.iter().chain(&twice).copied().collect();
        let (tree, node) = play(&four);
        assert_eq!(tree.game_end(node), Some(GameEnd::Fivefold));
    }

    #[test]
    fn a_different_side_to_move_is_not_a_repetition() {
        // The same squares, but with the other side to move each time.
        let (tree, node) = play(&["Nf3", "Nf6", "Ng1", "Ng8", "Nf3"]);
        assert_eq!(tree.game_end(node), None);
    }

    #[test]
    fn mate_and_move_counters() {
        let (tree, node) = play(&["f3", "e5", "g4", "Qh4#"]);
        assert_eq!(
            tree.game_end(node),
            Some(GameEnd::Checkmate {
                winner: Color::Black
            })
        );
        assert_eq!(tree.game_end(node).unwrap().result(), "0-1");
        let fen = |halfmoves: u32| {
            GameTree::from_fen(&format!("4k3/8/8/8/8/8/8/R3K3 w - - {halfmoves} 80")).unwrap()
        };
        assert_eq!(fen(99).game_end(GameTree::ROOT), None);
        assert_eq!(fen(100).game_end(GameTree::ROOT), Some(GameEnd::FiftyMoves));
        assert_eq!(
            fen(150).game_end(GameTree::ROOT),
            Some(GameEnd::SeventyFiveMoves)
        );
        let bare = GameTree::from_fen("4k3/8/8/8/8/8/8/4K3 w - - 0 1").unwrap();
        assert_eq!(
            bare.game_end(GameTree::ROOT),
            Some(GameEnd::InsufficientMaterial)
        );
    }
}
