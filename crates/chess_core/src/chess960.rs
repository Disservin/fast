//! Chess960 (Fischer random) start positions by their standard number, 0 to 959, and
//! telling Chess960 positions apart from standard ones.

use shakmaty::{CastlingMode, Chess, Position, fen::Fen};

/// Whether a game is standard chess or Chess960.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Variant {
    #[default]
    Standard,
    Chess960,
}

impl Variant {
    /// The castling rules positions of this variant follow.
    pub fn castling_mode(self) -> CastlingMode {
        match self {
            Self::Standard => CastlingMode::Standard,
            Self::Chess960 => CastlingMode::Chess960,
        }
    }

    /// The variant a PGN `Variant` tag names; unknown names are standard.
    pub fn from_tag(tag: &str) -> Self {
        let tag = tag.to_ascii_lowercase().replace([' ', '-', '_'], "");
        if matches!(
            tag.as_str(),
            "chess960" | "fischerandom" | "fischerrandom" | "960" | "frc"
        ) {
            Self::Chess960
        } else {
            Self::Standard
        }
    }
}

/// The back rank of Chess960 position `number` (0–959) in the standard numbering,
/// such as "RNBQKBNR" for 518. Numbers above 959 wrap.
pub fn back_rank(number: u16) -> String {
    let mut n = usize::from(number % 960);
    let mut rank = [None::<char>; 8];
    // Light-squared bishop on b, d, f or h; dark-squared on a, c, e or g.
    rank[2 * (n % 4) + 1] = Some('B');
    n /= 4;
    rank[2 * (n % 4)] = Some('B');
    n /= 4;
    let place = |rank: &mut [Option<char>; 8], mut nth: usize, piece: char| {
        for square in rank.iter_mut() {
            if square.is_none() {
                if nth == 0 {
                    *square = Some(piece);
                    return;
                }
                nth -= 1;
            }
        }
    };
    place(&mut rank, n % 6, 'Q');
    n /= 6;
    const KNIGHTS: [(usize, usize); 10] = [
        (0, 1),
        (0, 2),
        (0, 3),
        (0, 4),
        (1, 2),
        (1, 3),
        (1, 4),
        (2, 3),
        (2, 4),
        (3, 4),
    ];
    let (first, second) = KNIGHTS[n];
    // The second knight goes among the squares left after the first.
    place(&mut rank, first, 'N');
    place(&mut rank, second - 1, 'N');
    for piece in ['R', 'K', 'R'] {
        place(&mut rank, 0, piece);
    }
    rank.iter().map(|square| square.unwrap_or('?')).collect()
}

/// The FEN of Chess960 position `number`, with Shredder castling rights.
pub fn start_fen(number: u16) -> String {
    let white = back_rank(number);
    let rooks: Vec<char> = white
        .char_indices()
        .filter(|(_, piece)| *piece == 'R')
        .map(|(file, _)| (b'a' + file as u8) as char)
        .collect();
    let castling: String = rooks
        .iter()
        .rev()
        .map(|file| file.to_ascii_uppercase())
        .chain(rooks.iter().rev().copied())
        .collect();
    format!(
        "{}/pppppppp/8/8/8/8/PPPPPPPP/{white} w {castling} - 0 1",
        white.to_ascii_lowercase()
    )
}

/// Chess960 position `number` (0–959); 518 is the standard start.
pub fn chess960_position(number: u16) -> Chess {
    start_fen(number)
        .parse::<Fen>()
        .ok()
        .and_then(|fen| fen.into_position(CastlingMode::Chess960).ok())
        .expect("every Chess960 start position is legal")
}

/// The number of a Chess960 start position, when `pos` is one with full castling
/// rights and nothing moved.
pub fn chess960_number(pos: &Chess) -> Option<u16> {
    let board = pos.board();
    let rank: String = (0..8)
        .map(|file| {
            let square =
                shakmaty::Square::from_coords(shakmaty::File::new(file), shakmaty::Rank::First);
            match board.piece_at(square) {
                Some(piece) if piece.color.is_white() => piece.role.upper_char(),
                _ => '?',
            }
        })
        .collect();
    let number = (0..960).find(|&number| back_rank(number) == rank)?;
    let start = chess960_position(number);
    (start.board() == board
        && pos.turn() == start.turn()
        && pos.castles().castling_rights() == start.castles().castling_rights()
        && pos.halfmoves() == 0
        && pos.fullmoves().get() == 1)
        .then_some(number)
}

/// The variant a FEN needs: Chess960 when its castling rights only make sense there,
/// as with Shredder letters (`HAha`) or a king or rook off its standard square.
pub fn fen_variant(fen: &str) -> Option<Variant> {
    let parsed: Fen = fen.trim().parse().ok()?;
    let castling = fen.split_whitespace().nth(2).unwrap_or("-");
    let file_letters = castling
        .chars()
        .any(|c| c != '-' && !matches!(c, 'K' | 'Q' | 'k' | 'q'));
    if !file_letters
        && parsed
            .clone()
            .into_position::<Chess>(CastlingMode::Standard)
            .is_ok()
    {
        return Some(Variant::Standard);
    }
    parsed
        .into_position::<Chess>(CastlingMode::Chess960)
        .ok()
        .map(|_| Variant::Chess960)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn number_518_is_the_standard_start() {
        assert_eq!(back_rank(518), "RNBQKBNR");
        assert_eq!(chess960_position(518).board(), Chess::default().board());
        assert_eq!(chess960_number(&Chess::default()), Some(518));
        assert_eq!(back_rank(0), "BBQNNRKR");
        assert_eq!(back_rank(959), "RKRNNQBB");
    }

    #[test]
    fn all_960_numbers_give_different_legal_positions() {
        let mut seen = std::collections::HashSet::new();
        for number in 0..960 {
            let rank = back_rank(number);
            assert!(seen.insert(rank.clone()), "{number} repeats {rank}");
            let king = rank.find('K').unwrap();
            let rooks: Vec<usize> = rank.match_indices('R').map(|(i, _)| i).collect();
            assert!(rooks[0] < king && king < rooks[1], "{number}: {rank}");
            let pos = chess960_position(number);
            assert!(pos.legal_moves().len() >= 16);
            assert_eq!(chess960_number(&pos), Some(number));
        }
    }

    #[test]
    fn fens_tell_chess960_from_standard() {
        let standard = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";
        assert_eq!(fen_variant(standard), Some(Variant::Standard));
        // X-FEN: KQkq with the king and rooks off their standard squares.
        let xfen = "bqnbrkrn/pppppppp/8/8/8/8/PPPPPPPP/BQNBRKRN w KQkq - 0 1";
        assert_eq!(fen_variant(xfen), Some(Variant::Chess960));
        let shredder = "bqnbrkrn/pppppppp/8/8/8/8/PPPPPPPP/BQNBRKRN w GEge - 0 1";
        assert_eq!(fen_variant(shredder), Some(Variant::Chess960));
        assert_eq!(
            fen_variant("7k/8/8/8/8/8/8/K7 w - - 0 1"),
            Some(Variant::Standard)
        );
        assert_eq!(fen_variant("nonsense"), None);
        assert_eq!(Variant::from_tag("Fischerandom"), Variant::Chess960);
        assert_eq!(Variant::from_tag("Chess 960"), Variant::Chess960);
        assert_eq!(Variant::from_tag("Standard"), Variant::Standard);
    }
}
