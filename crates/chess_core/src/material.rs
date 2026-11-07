//! The material balance shown beside each player, as on Lichess: per piece type, the
//! side with more of them shows the surplus (the opponent's pieces it is up on), and
//! the side ahead in points shows `+N`. Counting what is on the board, rather than
//! listing captures, makes trades cancel out and promotions count correctly.

use shakmaty::{Board, Color, Role};

/// Lichess' order: pawns first, queen last.
const ORDER: [Role; 5] = [
    Role::Pawn,
    Role::Knight,
    Role::Bishop,
    Role::Rook,
    Role::Queen,
];

pub fn piece_value(role: Role) -> i32 {
    match role {
        Role::Pawn => 1,
        Role::Knight | Role::Bishop => 3,
        Role::Rook => 5,
        Role::Queen => 9,
        Role::King => 0,
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MaterialBalance {
    /// Opponent pieces White is up on (shown in Black's colour), pawns first.
    pub white_up: Vec<Role>,
    /// Opponent pieces Black is up on (shown in White's colour).
    pub black_up: Vec<Role>,
    /// Points, White minus Black.
    pub score: i32,
}

impl MaterialBalance {
    /// The pieces `side` is up on.
    pub fn up(&self, side: Color) -> &[Role] {
        match side {
            Color::White => &self.white_up,
            Color::Black => &self.black_up,
        }
    }

    /// `side`'s lead in points, if it is ahead.
    pub fn lead(&self, side: Color) -> Option<i32> {
        let lead = if side.is_white() {
            self.score
        } else {
            -self.score
        };
        (lead > 0).then_some(lead)
    }
}

pub fn material_balance(board: &Board) -> MaterialBalance {
    let mut balance = MaterialBalance::default();
    for role in ORDER {
        let count = |color: Color| (board.by_role(role) & board.by_color(color)).count() as i32;
        let diff = count(Color::White) - count(Color::Black);
        balance.score += diff * piece_value(role);
        let surplus = std::iter::repeat_n(role, diff.unsigned_abs() as usize);
        if diff > 0 {
            balance.white_up.extend(surplus);
        } else {
            balance.black_up.extend(surplus);
        }
    }
    balance
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::GameTree;
    use shakmaty::Position as _;

    fn board(fen: &str) -> Board {
        GameTree::from_fen(fen)
            .unwrap()
            .node(GameTree::ROOT)
            .pos
            .board()
            .clone()
    }

    #[test]
    fn the_start_is_balanced() {
        let balance = material_balance(&Board::default());
        assert_eq!(balance, MaterialBalance::default());
        assert_eq!(balance.lead(Color::White), None);
    }

    #[test]
    fn surplus_per_piece_type_and_points() {
        // White has an extra knight; Black has two extra pawns.
        let balance = material_balance(&board("4k3/pppp4/8/8/8/8/PP6/N3K3 w - - 0 1"));
        assert_eq!(balance.white_up, vec![Role::Knight]);
        assert_eq!(balance.black_up, vec![Role::Pawn, Role::Pawn]);
        assert_eq!(balance.score, 1);
        assert_eq!(balance.lead(Color::White), Some(1));
        assert_eq!(balance.lead(Color::Black), None);
        assert_eq!(balance.up(Color::Black).len(), 2);
    }

    #[test]
    fn a_promotion_counts_as_the_new_piece() {
        // Two white queens against one: White is up a queen but down the promoted pawn.
        let balance = material_balance(&board("3qk3/p7/8/8/8/8/8/QQ2K3 w - - 0 1"));
        assert_eq!(balance.white_up, vec![Role::Queen]);
        assert_eq!(balance.black_up, vec![Role::Pawn]);
        assert_eq!(balance.score, 8);
    }
}
