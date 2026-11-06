//! A compact opening table. Longest matching SAN prefix wins.

/// A named opening with its ECO code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Opening {
    pub eco: &'static str,
    pub name: &'static str,
}

const OPENINGS: &[(&str, &str, &str)] = &[
    ("e4", "B00", "King's Pawn Game"),
    ("e4 e5", "C20", "King's Pawn Game"),
    ("e4 e5 Nf3", "C40", "King's Knight Opening"),
    ("e4 e5 Nf3 Nc6", "C44", "King's Pawn Game"),
    ("e4 e5 Nf3 Nf6", "C42", "Petrov's Defence"),
    ("e4 e5 Nf3 Nc6 d4", "C44", "Scotch Game"),
    ("e4 e5 Nf3 Nc6 Bb5", "C60", "Ruy Lopez"),
    ("e4 e5 Nf3 Nc6 Bb5 a6", "C68", "Ruy Lopez, Morphy Defence"),
    ("e4 e5 Nf3 Nc6 Bb5 Nf6", "C65", "Ruy Lopez, Berlin Defence"),
    ("e4 e5 Nf3 Nc6 Bc4", "C50", "Italian Game"),
    ("e4 e5 Nf3 Nc6 Bc4 Bc5", "C50", "Italian Game, Giuoco Piano"),
    (
        "e4 e5 Nf3 Nc6 Bc4 Nf6",
        "C55",
        "Italian Game, Two Knights Defence",
    ),
    (
        "e4 e5 Nf3 Nc6 Bc4 Bc5 c3",
        "C53",
        "Italian Game, Giuoco Piano",
    ),
    (
        "e4 e5 Nf3 Nc6 Bc4 Bc5 c3 Nf6",
        "C54",
        "Italian Game, Giuoco Pianissimo",
    ),
    ("e4 c5", "B20", "Sicilian Defence"),
    ("e4 c5 c3", "B22", "Sicilian, Alapin Variation"),
    ("e4 c5 Nc3", "B23", "Sicilian, Closed"),
    ("e4 c5 Nf3", "B27", "Sicilian Defence"),
    ("e4 c5 Nf3 Nc6", "B30", "Sicilian Defence"),
    ("e4 c5 Nf3 Nc6 Bb5", "B30", "Sicilian, Rossolimo"),
    ("e4 c5 Nf3 e6", "B40", "Sicilian Defence"),
    ("e4 c5 Nf3 e6 d4 cxd4 Nxd4 a6", "B41", "Sicilian, Kan"),
    ("e4 c5 Nf3 d6", "B50", "Sicilian Defence"),
    (
        "e4 c5 Nf3 d6 d4 cxd4 Nxd4 Nf6 Nc3",
        "B56",
        "Sicilian Defence",
    ),
    (
        "e4 c5 Nf3 d6 d4 cxd4 Nxd4 Nf6 Nc3 Nc6",
        "B56",
        "Sicilian, Classical",
    ),
    (
        "e4 c5 Nf3 d6 d4 cxd4 Nxd4 Nf6 Nc3 g6",
        "B70",
        "Sicilian, Dragon",
    ),
    (
        "e4 c5 Nf3 d6 d4 cxd4 Nxd4 Nf6 Nc3 e6",
        "B80",
        "Sicilian, Scheveningen",
    ),
    (
        "e4 c5 Nf3 d6 d4 cxd4 Nxd4 Nf6 Nc3 a6",
        "B90",
        "Sicilian Najdorf",
    ),
    (
        "e4 c5 Nf3 d6 d4 cxd4 Nxd4 Nf6 Nc3 a6 Be3",
        "B90",
        "Sicilian Najdorf, English Attack",
    ),
    (
        "e4 c5 Nf3 d6 d4 cxd4 Nxd4 Nf6 Nc3 a6 Bg5",
        "B94",
        "Sicilian Najdorf, Bg5",
    ),
    ("e4 c6", "B10", "Caro-Kann Defence"),
    ("e4 e6", "C00", "French Defence"),
    ("e4 d6", "B07", "Pirc Defence"),
    ("e4 d5", "B01", "Scandinavian Defence"),
    ("e4 Nf6", "B02", "Alekhine's Defence"),
    ("d4", "A40", "Queen's Pawn Game"),
    ("d4 d5", "D00", "Queen's Pawn Game"),
    ("d4 d5 c4", "D06", "Queen's Gambit"),
    ("d4 d5 c4 e6", "D30", "Queen's Gambit Declined"),
    ("d4 d5 c4 c6", "D10", "Slav Defence"),
    ("d4 d5 c4 dxc4", "D20", "Queen's Gambit Accepted"),
    ("d4 Nf6", "A45", "Indian Defence"),
    ("d4 Nf6 c4", "A50", "Indian Defence"),
    ("d4 Nf6 c4 e6", "E00", "Indian Defence"),
    ("d4 Nf6 c4 g6", "E60", "King's Indian Defence"),
    ("d4 Nf6 c4 g6 Nc3 d5", "D80", "Grünfeld Defence"),
    ("d4 Nf6 c4 e6 Nc3 Bb4", "E20", "Nimzo-Indian Defence"),
    ("d4 Nf6 c4 e6 Nf3 b6", "E12", "Queen's Indian Defence"),
    ("d4 f5", "A80", "Dutch Defence"),
    ("c4", "A10", "English Opening"),
    ("Nf3", "A04", "Réti Opening"),
];

/// The most specific opening matching a sequence of SAN moves, ignoring check marks.
pub fn classify<'a>(moves: impl IntoIterator<Item = &'a str>) -> Option<Opening> {
    let line: Vec<&str> = moves
        .into_iter()
        .map(|san| san.trim_end_matches(['+', '#']))
        .collect();
    OPENINGS
        .iter()
        .filter(|(prefix, _, _)| {
            let parts = prefix.split(' ');
            let count = parts.clone().count();
            count <= line.len() && parts.zip(&line).all(|(a, b)| a == *b)
        })
        .max_by_key(|(prefix, _, _)| prefix.split(' ').count())
        .map(|&(_, eco, name)| Opening { eco, name })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn longest_prefix_wins() {
        let najdorf = "e4 c5 Nf3 d6 d4 cxd4 Nxd4 Nf6 Nc3 a6 Be3 e5 Nb3";
        let opening = classify(najdorf.split(' ')).unwrap();
        assert_eq!(opening.eco, "B90");
        assert_eq!(opening.name, "Sicilian Najdorf, English Attack");
        assert_eq!(
            classify("e4 e5 Nf3 Nc6 Bc4 Bc5 c3 Nf6".split(' '))
                .unwrap()
                .eco,
            "C54"
        );
        assert!(classify("a4".split(' ')).is_none());
    }
}
