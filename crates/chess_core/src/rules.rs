//! How a game ends: checkmate, stalemate and the draw rules that need the game's
//! history (repetition, the fifty- and seventy-five-move rules).

use shakmaty::{Chess, Color, Position};

use crate::{GameTree, NodeId};

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

/// How the game stands at `pos`, given every position from the start
/// up to and including `pos`. Repetition compares piece placement, side to
/// move, castling rights and legal en passant, as the rules do.
pub fn game_end(pos: &Chess, history: &[Chess]) -> Option<GameEnd> {
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
    // Chess equality compares the actual repetition state, ignoring move counters.
    // A matching Zobrist hash alone is insufficient because hashes can collide.
    let seen = history.iter().filter(|known| *known == pos).count().max(1);
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
    /// Positions from the root to `id`, both included.
    pub fn history(&self, id: NodeId) -> Vec<Chess> {
        std::iter::once(GameTree::ROOT)
            .chain(self.path_to(id))
            .map(|node| self.node(node).pos.clone())
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
    fn repetition_compares_positions_and_ignores_move_counters() {
        let position = |fen: &str| {
            GameTree::from_fen(fen)
                .unwrap()
                .node(GameTree::ROOT)
                .pos
                .clone()
        };
        let pos = position("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 8 5");
        let earlier = position("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1");
        assert_eq!(
            game_end(&pos, &[earlier.clone(), earlier, pos.clone()]),
            Some(GameEnd::Threefold)
        );
        for fen in [
            "r3k2r/8/8/8/8/8/8/R3K2R b KQkq - 0 1",
            "r3k2r/8/8/8/8/8/8/R3K2R w Qq - 0 1",
            "r3k2r/8/8/8/8/8/R7/4K2R w Kkq - 0 1",
        ] {
            let other = position(fen);
            assert_eq!(game_end(&pos, &[other.clone(), other, pos.clone()]), None);
        }
    }

    #[test]
    fn only_legal_en_passant_distinguishes_repetitions() {
        for (pawns, expected) in [("3pP3", None), ("3p4", Some(GameEnd::Threefold))] {
            let position = |ep| {
                GameTree::from_fen(&format!("4k3/8/8/{pawns}/8/8/8/R3K3 w - {ep} 0 2"))
                    .unwrap()
                    .node(GameTree::ROOT)
                    .pos
                    .clone()
            };
            let pos = position("d6");
            let earlier = position("-");
            assert_eq!(
                game_end(&pos, &[earlier.clone(), earlier, pos.clone()]),
                expected
            );
        }
    }

    #[test]
    fn zobrist_collision_is_not_a_repetition() {
        // Collision example from Disservin/chess-library#252.
        let mut tree = GameTree::from_fen("3q4/2q5/4q3/q4k2/8/8/R5q1/2K1B1BQ w - - 0 1").unwrap();
        let mut node = GameTree::ROOT;
        for san in [
            "Rc2", "Qcb8", "Ra2", "Qbc7+", "Rc2", "Qgg8", "Bb6", "Qcg3", "Bb4", "Qec8", "Qg2",
            "Qa1",
        ] {
            node = tree.add_san(node, san).unwrap();
        }
        let pos = &tree.node(node).pos;
        let hash = crate::position_hash(pos);
        let collision = tree
            .history(node)
            .into_iter()
            .find(|earlier| crate::position_hash(earlier) == hash && earlier != pos)
            .expect("fixture must contain a real Zobrist collision");
        // Use the earlier, nonterminal position so checkmate precedence cannot
        // hide a false repetition in this regression test.
        assert_eq!(
            game_end(&collision, &[pos.clone(), pos.clone(), collision.clone()]),
            None
        );
    }

    #[test]
    fn insufficient_material_and_material_that_can_still_mate() {
        for pieces in [
            "4k3/8/8/8/8/8/8/4K3",    // Bare kings.
            "4k3/8/8/8/8/8/8/2B1K3",  // King and bishop.
            "4k3/8/8/8/8/8/8/4K1N1",  // King and knight.
            "4kb2/8/8/8/8/8/8/2B1K3", // Bishops on the same colour.
        ] {
            let tree = GameTree::from_fen(&format!("{pieces} w - - 0 1")).unwrap();
            assert_eq!(
                tree.game_end(GameTree::ROOT),
                Some(GameEnd::InsufficientMaterial),
                "{pieces}"
            );
        }
        for pieces in [
            "4k3/8/8/8/8/8/8/2B1K1N1", // Bishop and knight can mate.
            "4k3/8/8/8/8/8/8/4K1NN",   // Two knights can mate with cooperation.
            "2b1k3/8/8/8/8/8/8/2B1K3", // Opposite-coloured bishops.
            "4k3/8/8/8/8/8/P7/4K3",    // A pawn can promote.
        ] {
            let tree = GameTree::from_fen(&format!("{pieces} w - - 0 1")).unwrap();
            assert_eq!(tree.game_end(GameTree::ROOT), None, "{pieces}");
        }
    }

    #[test]
    fn stalemate_and_checkmate_take_precedence_over_move_counters() {
        let stalemate = GameTree::from_fen("7k/5K2/6Q1/8/8/8/8/8 b - - 150 80").unwrap();
        assert_eq!(stalemate.game_end(GameTree::ROOT), Some(GameEnd::Stalemate));
        let mate = GameTree::from_fen("7k/6Q1/5K2/8/8/8/8/8 b - - 150 80").unwrap();
        assert_eq!(
            mate.game_end(GameTree::ROOT),
            Some(GameEnd::Checkmate {
                winner: Color::White
            })
        );
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
