//! The demo databases: My games with notes and reviews, a reference database and small collections.

use super::*;

pub(super) fn play_san(pos: &mut Chess, sans: &mut Vec<String>, san: &str) {
    let parsed: chess_core::shakmaty::san::San = san.parse().expect("demo SAN");
    let mv = parsed.to_move(pos).expect("legal demo SAN");
    sans.push(SanPlus::from_move_and_play_unchecked(pos, mv).to_string());
}

/// A line followed by random legal moves up to `target` plies (or close, if mated).
pub(super) fn extend(line: &str, target: usize, rng: &mut Rng) -> Vec<String> {
    let mut pos = Chess::default();
    let mut sans = Vec::with_capacity(target);
    for san in line.split_ascii_whitespace() {
        play_san(&mut pos, &mut sans, san);
    }
    while sans.len() < target {
        let moves = pos.legal_moves();
        let quiet: Vec<&Move> = moves.iter().filter(|mv| !mv.is_capture()).collect();
        let mv = if !quiet.is_empty() && rng.below(5) > 0 {
            *quiet[rng.below(quiet.len())]
        } else if !moves.is_empty() {
            moves[rng.below(moves.len())]
        } else {
            break;
        };
        sans.push(SanPlus::from_move_and_play_unchecked(&mut pos, mv).to_string());
    }
    sans
}

#[derive(Default)]
pub(super) struct PgnGame<'a> {
    white: &'a str,
    black: &'a str,
    white_elo: u32,
    black_elo: u32,
    result: &'a str,
    date: String,
    eco: &'a str,
    event: &'a str,
    round: &'a str,
    site: &'a str,
    time_control: &'a str,
    sans: Vec<String>,
}

pub(super) fn write_game(out: &mut String, game: &PgnGame<'_>) {
    write_annotated_game(out, game, &[]);
}

/// Annotations for the Najdorf English Attack, by ply: NAGs, comments and variations
/// written after that move.
pub(super) const NAJDORF_NOTES: &[(usize, &str)] = &[
    (
        9,
        "$1 { The Najdorf: a flexible move that keeps b5 and e5 in reserve. [%clk 0:14:50] }",
    ),
    (
        10,
        "{ The English Attack. } (6. Bg5 e6 7. f4 { the old main line }) (6. Be2 e5 7. Nb3)",
    ),
    (11, "$5 { A sharp position with play on both wings. }"),
];

/// The club league game's notes: the opening ones and a comment and variation at the
/// moment the preview shows.
pub(super) const CLUB_NOTES: &[(usize, &str)] = &[
    (9, "$1 { The Najdorf. [%clk 0:14:50] }"),
    (
        26,
        "$1 { Opening the f-file before Black's queenside play arrives. } (14. h4 a5 15. h5)",
    ),
];

/// Writes a game like [`write_game`], adding the raw annotation text in `notes` after
/// the move at each given ply.
pub(super) fn write_annotated_game(out: &mut String, game: &PgnGame<'_>, notes: &[(usize, &str)]) {
    use std::fmt::Write as _;
    let _ = writeln!(out, "[Event \"{}\"]", game.event);
    let _ = writeln!(out, "[Site \"{}\"]", game.site);
    let _ = writeln!(out, "[Date \"{}\"]", game.date);
    let _ = writeln!(out, "[Round \"{}\"]", game.round);
    let _ = writeln!(out, "[White \"{}\"]", game.white);
    let _ = writeln!(out, "[Black \"{}\"]", game.black);
    let _ = writeln!(out, "[Result \"{}\"]", game.result);
    let _ = writeln!(out, "[WhiteElo \"{}\"]", game.white_elo);
    let _ = writeln!(out, "[BlackElo \"{}\"]", game.black_elo);
    if !game.eco.is_empty() {
        let _ = writeln!(out, "[ECO \"{}\"]", game.eco);
    }
    let _ = writeln!(out, "[TimeControl \"{}\"]\n", game.time_control);
    for (ply, san) in game.sans.iter().enumerate() {
        if ply % 2 == 0 {
            let _ = write!(out, "{}. ", ply / 2 + 1);
        }
        let _ = write!(out, "{san} ");
        for (_, note) in notes.iter().filter(|(at, _)| *at == ply) {
            let _ = write!(out, "{note} ");
        }
    }
    let _ = writeln!(out, "{}\n", game.result);
}

/// Rows from the design: opponent, ratings, result, source, date, moves, accuracy, line.
pub(super) type MyRow = (
    &'static str,
    u32,
    u32,
    &'static str,
    &'static str,
    &'static str,
    usize,
    f64,
    &'static str,
);

pub(super) const MY_ROWS: &[MyRow] = &[
    (
        "knightowl_42",
        2061,
        2012,
        "1-0",
        "OTB",
        "2026.11.02",
        41,
        91.2,
        "f4 a5 f5 Bc4 Kb1",
    ),
    (
        "sicilian_sam",
        2058,
        2104,
        "1/2-1/2",
        "Online",
        "2026.10.29",
        58,
        88.0,
        "h4",
    ),
    (
        "pawnstorm_ida",
        2055,
        1987,
        "1-0",
        "Online",
        "2026.10.24",
        33,
        93.5,
        "OPP",
    ),
    (
        "e5_or_bust",
        2049,
        2140,
        "0-1",
        "Online",
        "2026.10.19",
        47,
        74.1,
        "h4",
    ),
    (
        "quietmoves",
        2051,
        2033,
        "1-0",
        "OTB",
        "2026.10.11",
        29,
        89.7,
        "f4",
    ),
    (
        "bishoppair",
        2046,
        1998,
        "1/2-1/2",
        "Online",
        "2026.10.05",
        62,
        85.3,
        "G4",
    ),
    (
        "rookliftr",
        2040,
        2071,
        "1-0",
        "Online",
        "2026.09.28",
        38,
        90.4,
        "OPP",
    ),
    (
        "dragonslayer9",
        2038,
        2110,
        "0-1",
        "OTB",
        "2026.09.20",
        44,
        71.8,
        "h4",
    ),
    (
        "zwischenzug",
        2041,
        1962,
        "1-0",
        "Online",
        "2026.09.13",
        27,
        94.0,
        "OPP",
    ),
    (
        "hedgehog_h",
        2035,
        2025,
        "1/2-1/2",
        "Online",
        "2026.09.06",
        71,
        86.6,
        "f4",
    ),
    (
        "fianchetto_f",
        2032,
        2088,
        "1-0",
        "OTB",
        "2026.08.30",
        36,
        90.9,
        "OPP",
    ),
    (
        "tempo_tom",
        2029,
        1979,
        "0-1",
        "Online",
        "2026.08.22",
        52,
        78.2,
        "h4",
    ),
];

pub(super) fn my_games(path: &Path, rng: &mut Rng) {
    let mut pgn = String::new();
    let line_after = |kind: &str| -> String {
        match kind {
            // The opponent leaves the book, so the user never deviates.
            "OPP" => format!("{ENGLISH_ATTACK} e6"),
            "G4" => "e4 c5 Nf3 d6 d4 cxd4 Nxd4 Nf6 Nc3 a6 Be3 e5 Nb3 Be6 f3 Be7 g4".to_owned(),
            moves => format!("{LINE} {moves}"),
        }
    };
    for &(opponent, my_elo, opp_elo, result, source, date, moves, _, kind) in MY_ROWS {
        // The club league game is annotated, so the preview shows comments.
        let notes = if opponent == "knightowl_42" && kind != "OPP" && kind != "G4" {
            CLUB_NOTES
        } else {
            &[]
        };
        write_annotated_game(
            &mut pgn,
            &PgnGame {
                white: "me",
                black: opponent,
                white_elo: my_elo,
                black_elo: opp_elo,
                result,
                date: date.to_owned(),
                eco: "B90",
                event: if opponent == "knightowl_42" {
                    "Club league"
                } else {
                    "Casual"
                },
                round: if opponent == "knightowl_42" { "4" } else { "?" },
                site: if source == "OTB" {
                    "Club"
                } else {
                    "https://lichess.org"
                },
                time_control: "900+10",
                sans: extend(&line_after(kind), moves * 2 - 1, rng),
            },
            notes,
        );
    }
    // Further games to reach the design's 128 filtered games at 58.2%.
    let results: Vec<&str> = std::iter::repeat_n("1-0", 3)
        .chain(std::iter::once("1/2-1/2"))
        .chain(std::iter::once("0-1"))
        .collect();
    for (index, result) in results.iter().enumerate() {
        write_game(
            &mut pgn,
            &PgnGame {
                white: "me",
                black: "club_regular",
                white_elo: 2020,
                black_elo: 1950 + index as u32 * 10,
                result,
                date: format!("2026.07.{:02}", 28 - index),
                eco: "B90",
                event: "Casual",
                round: "?",
                site: "https://lichess.org",
                time_control: "900+10",
                sans: extend(&format!("{LINE} h4"), 60 + index * 2, rng),
            },
        );
    }
    let mut filler_results: Vec<&str> = Vec::new();
    filler_results.extend(std::iter::repeat_n("1-0", 56));
    filler_results.extend(std::iter::repeat_n("1/2-1/2", 15));
    filler_results.extend(std::iter::repeat_n("0-1", 40));
    for (index, result) in filler_results.iter().enumerate() {
        let line = if index < 4 {
            format!("{ENGLISH_ATTACK} Ng4 Bc1")
        } else {
            format!("{ENGLISH_ATTACK} e6")
        };
        write_game(
            &mut pgn,
            &PgnGame {
                white: "me",
                black: "club_regular",
                white_elo: 2000,
                black_elo: 1900 + (index as u32 % 200),
                result,
                date: format!("2026.0{}.{:02}", 1 + index % 7, 1 + index % 27),
                eco: "B90",
                event: "Casual",
                round: "?",
                site: "https://lichess.org",
                time_control: "900+10",
                sans: extend(&line, 50 + rng.below(40), rng),
            },
        );
    }
    // Games reaching a position the repertoire does not answer yet.
    for index in 0..11 {
        write_game(
            &mut pgn,
            &PgnGame {
                white: "me",
                black: "kan_player",
                white_elo: 2000,
                black_elo: 1800,
                result: if index % 2 == 0 { "1-0" } else { "0-1" },
                date: format!("2025.12.{:02}", 1 + index),
                eco: "B41",
                event: "Blitz",
                round: "?",
                site: "https://lichess.org",
                time_control: "180+0",
                sans: extend("e4 c5 Nf3 e6 d4 a6", 40, rng),
            },
        );
    }
    // The rest of the collection: games as Black, which a White repertoire ignores.
    let openings = [
        "d4 Nf6 c4 g6",
        "Nf3 d5 d4 Nf6",
        "c4 e5 Nc3 Nf6",
        "e4 e5 Nf3 Nc6",
        "d4 d5 c4 e6",
    ];
    for index in 0..3075 {
        let result = ["1-0", "0-1", "1/2-1/2"][rng.below(3)];
        write_game(
            &mut pgn,
            &PgnGame {
                white: "club_regular",
                black: "me",
                white_elo: 1800 + rng.below(400) as u32,
                black_elo: 2000,
                result,
                date: format!(
                    "20{:02}.{:02}.{:02}",
                    18 + index % 8,
                    1 + index % 12,
                    1 + index % 27
                ),
                eco: "",
                event: "Blitz",
                round: "?",
                site: "https://lichess.org",
                time_control: "180+0",
                sans: extend(openings[index % openings.len()], 24 + rng.below(20), rng),
            },
        );
    }
    let imported = import_games(
        path,
        pgn.as_bytes(),
        &ImportOptions {
            build_structure_index: true,
            ..Default::default()
        },
        &ImportControl::default(),
        |_| {},
    );
    if let Err(err) = imported {
        log::error!("demo import failed: {err}");
        return;
    }
    let Ok(database) = Database::open(path) else {
        return;
    };
    let _ = database.set_name("My games");
    let Ok(ids) = database.filtered_ids(&db::Filter {
        eco_min: "B90".to_owned(),
        eco_max: "B90".to_owned(),
        ..Default::default()
    }) else {
        return;
    };
    for (id, row) in ids.iter().zip(MY_ROWS) {
        let _ = database.set_accuracy(*id, row.7);
    }
    for (offset, id) in ids.iter().enumerate().skip(MY_ROWS.len()) {
        let _ = database.set_accuracy(*id, 72.0 + (offset % 24) as f64);
    }
}

pub(super) fn reference_db(path: &Path, rng: &mut Rng) {
    let mut pgn = String::new();
    // Move, game count, wins, draws (scores as in the design's "Moves from here").
    let moves = [
        ("f4", 412, 165, 132),
        ("h4", 268, 90, 99),
        ("Kb1", 131, 40, 54),
        ("Ng3", 47, 12, 20),
    ];
    for (san, count, wins, draws) in moves {
        for index in 0..count {
            let result = if index < wins {
                "1-0"
            } else if index < wins + draws {
                "1/2-1/2"
            } else {
                "0-1"
            };
            write_game(
                &mut pgn,
                &PgnGame {
                    white: "Master A",
                    black: "Master B",
                    white_elo: 2500 + rng.below(250) as u32,
                    black_elo: 2500 + rng.below(250) as u32,
                    result,
                    date: format!("20{:02}.{:02}.01", 10 + index % 16, 1 + index % 12),
                    eco: "B90",
                    event: "Open",
                    round: "1",
                    site: "Club",
                    time_control: "5400+30",
                    sans: extend(&format!("{LINE} {san}"), 28 + rng.below(4), rng),
                },
            );
        }
    }
    import_named(path, &pgn, "Reference: OTB masters");
}

pub(super) fn small_db(
    path: &Path,
    name: &str,
    count: usize,
    white: &str,
    line: &str,
    rng: &mut Rng,
) {
    let mut pgn = String::new();
    for index in 0..count {
        // A model game collection: the first games carry a book's comments.
        let notes = if line == ENGLISH_ATTACK && index < 12 {
            NAJDORF_NOTES
        } else {
            &[]
        };
        write_annotated_game(
            &mut pgn,
            &PgnGame {
                white,
                black: "Opponent",
                white_elo: 2200,
                black_elo: 2200,
                result: ["1-0", "1/2-1/2", "0-1"][index % 3],
                date: format!("20{:02}.01.01", 10 + index % 16),
                eco: "B90",
                event: "Open",
                round: "1",
                site: "Club",
                time_control: "5400+30",
                sans: extend(line, 30 + rng.below(30), rng),
            },
            notes,
        );
    }
    import_named(path, &pgn, name);
    // The model games read like a book, so they open in file order.
    if line == ENGLISH_ATTACK
        && let Ok(database) = Database::open(path)
    {
        let _ = database.set_source_file(Path::new("najdorf-model-games.pgn"));
    }
}

pub(super) fn import_named(path: &Path, pgn: &str, name: &str) {
    match import_games(
        path,
        pgn.as_bytes(),
        &ImportOptions {
            build_structure_index: true,
            ..Default::default()
        },
        &ImportControl::default(),
        |_| {},
    ) {
        Ok(_) => {
            if let Ok(database) = Database::open(path) {
                let _ = database.set_name(name);
            }
        }
        Err(err) => log::error!("demo import failed: {err}"),
    }
}
