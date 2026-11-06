//! The Najdorf game used by demo mode.

use crate::{GameTree, parse_pgn};

pub const NAJDORF_PGN: &str = r#"[Event "Club league"]
[Site "?"]
[Date "2026.11.02"]
[Round "4"]
[White "White"]
[Black "Black"]
[Result "*"]
[WhiteElo "2061"]
[BlackElo "2012"]

1. e4 c5 2. Nf3 d6 3. d4 cxd4 4. Nxd4 Nf6 5. Nc3 a6 6. Be3 e5 7. Nb3 Be6 8. f3 Be7
(8... h5 $5 9. Qd2 Nbd7 10. O-O-O b5)
9. Qd2 O-O 10. O-O-O Nbd7
{ Opposite-side castling: both kings now face a pawn storm, and tempo decides. }
11. g4 b5 12. g5 b4 13. Ne2 (13. Nd5 Nxd5 14. exd5 Bf5 15. Bd3 $10) 13... Ne8 $5 *
"#;

pub fn najdorf() -> GameTree {
    parse_pgn(NAJDORF_PGN)
        .ok()
        .and_then(|mut games| games.pop())
        .unwrap_or_default()
}
