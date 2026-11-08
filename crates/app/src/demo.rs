//! `--demo`: a separate, generated profile showing every screen populated, including
//! scripted engine output so the UI can be explored without any engine installed.
//! Nothing here runs in normal mode, and it never touches the real profile.

use std::{collections::HashMap, path::Path};

use chess_core::{
    Chess, GameTree, Position,
    shakmaty::{Move, san::SanPlus},
};
use db::{Database, ImportControl, ImportOptions, import_games};
use engine::{AnalysisUpdate, EngineConfig, EngineInfo, OptionKind, Score, UciOption, Wdl};
use trainer::{Card, CardState, CuratedRow, RepNode, Repertoire};

use crate::{
    config::{Config, Paths},
    model::today,
};

const LINE: &str = "e4 c5 Nf3 d6 d4 cxd4 Nxd4 Nf6 Nc3 a6 Be3 e5 Nb3 Be6 f3 Be7 Qd2 O-O O-O-O Nbd7 g4 b5 g5 b4 Ne2 Ne8";
const ENGLISH_ATTACK: &str = "e4 c5 Nf3 d6 d4 cxd4 Nxd4 Nf6 Nc3 a6 Be3";

/// A tiny deterministic generator so demo data is identical on every machine.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, bound: usize) -> usize {
        (self.next() % bound.max(1) as u64) as usize
    }
}

fn play_san(pos: &mut Chess, sans: &mut Vec<String>, san: &str) {
    let parsed: chess_core::shakmaty::san::San = san.parse().expect("demo SAN");
    let mv = parsed.to_move(pos).expect("legal demo SAN");
    sans.push(SanPlus::from_move_and_play_unchecked(pos, mv).to_string());
}

/// A line followed by random legal moves up to `target` plies (or close, if mated).
fn extend(line: &str, target: usize, rng: &mut Rng) -> Vec<String> {
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
struct PgnGame<'a> {
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

fn write_game(out: &mut String, game: &PgnGame<'_>) {
    write_annotated_game(out, game, &[]);
}

/// Annotations for the Najdorf English Attack, by ply: NAGs, comments and variations
/// written after that move.
const NAJDORF_NOTES: &[(usize, &str)] = &[
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
const CLUB_NOTES: &[(usize, &str)] = &[
    (9, "$1 { The Najdorf. [%clk 0:14:50] }"),
    (
        26,
        "$1 { Opening the f-file before Black's queenside play arrives. } (14. h4 a5 15. h5)",
    ),
];

/// Writes a game like [`write_game`], adding the raw annotation text in `notes` after
/// the move at each given ply.
fn write_annotated_game(out: &mut String, game: &PgnGame<'_>, notes: &[(usize, &str)]) {
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
type MyRow = (
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

const MY_ROWS: &[MyRow] = &[
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

fn my_games(path: &Path, rng: &mut Rng) {
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
        &ImportOptions::default(),
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

fn reference_db(path: &Path, rng: &mut Rng) {
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

fn small_db(path: &Path, name: &str, count: usize, white: &str, line: &str, rng: &mut Rng) {
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

fn import_named(path: &Path, pgn: &str, name: &str) {
    match import_games(
        path,
        pgn.as_bytes(),
        &ImportOptions::default(),
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

/// Card states needed to display each curated row as in the design.
fn card_known(today: i64) -> Card {
    Card {
        state: CardState::Review,
        ease: 2.5,
        interval_days: 21,
        due_day: today + 14,
        lapses: 0,
    }
}

fn card_learning(today: i64) -> Card {
    Card {
        state: CardState::Review,
        ease: 2.3,
        interval_days: 3,
        due_day: today + 2,
        lapses: 1,
    }
}

fn card_due(today: i64, age: i64) -> Card {
    Card {
        state: CardState::Learning,
        ease: 2.3,
        interval_days: 2,
        due_day: today - age,
        lapses: 0,
    }
}

fn repertoire(today: i64) -> Repertoire {
    let mut rep = Repertoire::new("White vs Sicilian", true);
    let main = LINE.split(' ').collect::<Vec<_>>();
    let lines: Vec<Vec<&str>> = vec![
        [main.as_slice(), &["f4"]].concat(),
        [main.as_slice(), &["Ng3"]].concat(),
        [&main[..23], &["Nh5", "Nd5"]].concat(),
        "e4 c5 Nf3 d6 d4 cxd4 Nxd4 Nf6 Nc3 a6 Be3 e5 Nb3 Be7 f3"
            .split(' ')
            .collect(),
        "e4 c5 Nf3 d6 d4 cxd4 Nxd4 Nf6 Nc3 a6 Be3 Ng4 Bg5"
            .split(' ')
            .collect(),
        "e4 c5 Nf3 d6 d4 cxd4 Nxd4 Nf6 Nc3 Nc6 Bg5"
            .split(' ')
            .collect(),
        "e4 c5 Nf3 d6 d4 cxd4 Nxd4 Nf6 Nc3 g6 Be3"
            .split(' ')
            .collect(),
        "e4 c5 Nf3 Nc6 Bb5".split(' ').collect(),
        "e4 c5 Nf3 e6 d4".split(' ').collect(),
    ];
    for line in &lines {
        rep.add_line(line).expect("legal demo repertoire line");
    }
    // Grow to the design's 412 positions with side lines off the main tree. They are
    // reachable in drills and coverage, but curated rows keep the tree readable.
    let mut rng = Rng(0x5EED_1234_ABCD_0001);
    let mut base: Vec<Vec<String>> = lines
        .iter()
        .map(|line| line.iter().map(|san| (*san).to_owned()).collect())
        .collect();
    let forbidden = [
        format!("{LINE} h4"),
        format!("{LINE} Kb1"),
        format!("{ENGLISH_ATTACK} e6"),
        format!("{ENGLISH_ATTACK} Ng4 Bc1"),
        format!("{ENGLISH_ATTACK} e5 Nb3 Be6 f3 Be7 g4"),
        // The book deliberately ends here: games reach this as an unanswered gap.
        "e4 c5 Nf3 e6 d4 ".to_owned(),
    ];
    let mut guard = 0;
    while rep.positions() < 412 && guard < 5_000 {
        guard += 1;
        let line = base[rng.below(base.len())].clone();
        let cut = 6 + rng.below(line.len().saturating_sub(6).max(1));
        let mut pos = Chess::default();
        let mut path: Vec<String> = Vec::new();
        for san in line.iter().take(cut) {
            play_san(&mut pos, &mut path, san);
        }
        // A move and its reply, or a single move when that would overshoot the target.
        let plies = if rep.positions() >= 411 { 1 } else { 2 };
        for _ in 0..plies {
            let moves = pos.legal_moves();
            if moves.is_empty() {
                break;
            }
            let mv = moves[rng.below(moves.len())];
            path.push(SanPlus::from_move_and_play_unchecked(&mut pos, mv).to_string());
        }
        // Keep filler clear of the lines the demo games use to leave the book.
        let joined = path.join(" ");
        if forbidden
            .iter()
            .any(|prefix| joined.starts_with(prefix.as_str()))
        {
            continue;
        }
        let refs: Vec<&str> = path.iter().map(String::as_str).collect();
        if rep.add_line(&refs).is_ok() && path.len().is_multiple_of(7) {
            base.push(path);
        }
    }
    // Assign schedule states so coverage reads 71% known, 18% learning, 11% new
    // once the curated rows below have their own states.
    let mut counter = 0u32;
    let mut due_left = 19;
    fn walk(nodes: &mut [RepNode], today: i64, counter: &mut u32, due_left: &mut i32) {
        for node in nodes {
            if node.card.is_some() {
                *counter += 1;
                let slot = *counter % 100;
                node.card = Some(if slot < 72 {
                    card_known(today)
                } else if *due_left > 0 {
                    *due_left -= 1;
                    card_due(today, 1)
                } else if slot < 90 {
                    card_learning(today)
                } else {
                    Card::default()
                });
            }
            walk(&mut node.children, today, counter, due_left);
        }
    }
    walk(&mut rep.roots, today, &mut counter, &mut due_left);

    // The rows shown in the tree, with their display status set on the row's own card.
    let rows: &[(&str, usize, &str, Option<Card>)] = &[
        ("e4", 0, "1.e4 c5 2.Nf3 d6 3.d4", Some(card_known(today))),
        (
            "e4 c5 Nf3 d6 d4 cxd4 Nxd4 Nf6 Nc3 a6",
            1,
            "5…a6 Najdorf",
            Some(card_known(today)),
        ),
        (
            "e4 c5 Nf3 d6 d4 cxd4 Nxd4 Nf6 Nc3 a6 Be3",
            2,
            "6.Be3 e5 7.Nb3",
            Some(card_known(today)),
        ),
        (
            "e4 c5 Nf3 d6 d4 cxd4 Nxd4 Nf6 Nc3 a6 Be3 e5 Nb3 Be6",
            3,
            "7…Be6 8.f3",
            Some(card_learning(today)),
        ),
        (
            "e4 c5 Nf3 d6 d4 cxd4 Nxd4 Nf6 Nc3 a6 Be3 e5 Nb3 Be6 f3 Be7 Qd2 O-O O-O-O Nbd7 g4 b5 g5 b4",
            4,
            "12…b4 13.Ne2",
            Some(card_due(today, 30)),
        ),
        (
            "e4 c5 Nf3 d6 d4 cxd4 Nxd4 Nf6 Nc3 a6 Be3 e5 Nb3 Be6 f3 Be7 Qd2 O-O O-O-O Nbd7 g4 b5 g5 Nh5",
            4,
            "12…Nh5 13.Nd5",
            Some(Card::default()),
        ),
        (
            "e4 c5 Nf3 d6 d4 cxd4 Nxd4 Nf6 Nc3 a6 Be3 e5 Nb3 Be7",
            3,
            "7…Be7 8.f3",
            Some(card_known(today)),
        ),
        (
            "e4 c5 Nf3 d6 d4 cxd4 Nxd4 Nf6 Nc3 a6 Be3 Ng4",
            2,
            "6…Ng4 7.Bg5",
            Some(card_known(today)),
        ),
        (
            "e4 c5 Nf3 d6 d4 cxd4 Nxd4 Nf6 Nc3 Nc6",
            1,
            "5…Nc6 Classical",
            Some(card_known(today)),
        ),
        (
            "e4 c5 Nf3 d6 d4 cxd4 Nxd4 Nf6 Nc3 g6",
            1,
            "5…g6 Dragon",
            Some(card_learning(today)),
        ),
        ("e4 c5 Nf3 Nc6", 0, "2…Nc6 3.Bb5", Some(card_known(today))),
        ("e4 c5 Nf3 e6", 0, "2…e6 3.d4", Some(Card::default())),
    ];
    for (path, depth, label, card) in rows {
        let path: Vec<&str> = path.split(' ').collect();
        apply_row(&mut rep, &path, *depth, label, *card);
    }
    rep
}

fn apply_row(rep: &mut Repertoire, path: &[&str], depth: usize, label: &str, card: Option<Card>) {
    let mut level = &mut rep.roots;
    for (index, san) in path.iter().enumerate() {
        let Some(found) = level.iter().position(|candidate| candidate.san == *san) else {
            return;
        };
        if index + 1 == path.len() {
            let target = &mut level[found];
            target.row = Some(CuratedRow {
                depth,
                label: label.to_owned(),
            });
            // Opponent moves show the status of the user's reply.
            if target.card.is_some() {
                target.card = card;
            } else if let Some(reply) = target.children.first_mut() {
                reply.card = card;
            }
            return;
        }
        level = &mut level[found].children;
    }
}

fn demo_engine(
    id: u64,
    (name, version, path): (&str, &str, &str),
    enabled: bool,
    uses: (bool, bool, bool),
    options: &[(&str, &str)],
) -> EngineConfig {
    let schema = vec![
        spin("Threads", 16, 1, 1024),
        spin("Hash", 4096, 1, 1_048_576),
        spin("MultiPV", 3, 1, 256),
        UciOption {
            name: "SyzygyPath".to_owned(),
            kind: OptionKind::String {
                default: String::new(),
            },
        },
        UciOption {
            name: "UCI_ShowWDL".to_owned(),
            kind: OptionKind::Check { default: true },
        },
    ];
    EngineConfig {
        id,
        name: name.to_owned(),
        version: version.to_owned(),
        path: path.into(),
        enabled,
        options: options
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect(),
        use_analysis: uses.0,
        use_review: uses.1,
        use_play: uses.2,
        info: EngineInfo {
            name: format!("{name} {version}"),
            author: String::new(),
            options: schema,
        },
        ..Default::default()
    }
}

fn spin(name: &str, default: i64, min: i64, max: i64) -> UciOption {
    UciOption {
        name: name.to_owned(),
        kind: OptionKind::Spin { default, min, max },
    }
}

/// Builds the demo profile if needed and fills in the demo configuration.
pub fn prepare(paths: &Paths, config: &mut Config) {
    let today = today();
    config.player_name = "me".to_owned();
    config.saved_filters = vec![
        crate::config::SavedFilter {
            name: "Najdorf losses as White".to_owned(),
            player: "me".to_owned(),
            colour: 1,
            result: 3,
            eco: "B90–B99".to_owned(),
            ..Default::default()
        },
        crate::config::SavedFilter {
            name: "Annotated model games".to_owned(),
            only_annotated: true,
            ..Default::default()
        },
    ];
    // A remembered Lichess account, so Settings › Databases and Import show one.
    config.player_aliases = vec!["me_on_lichess".to_owned()];
    config.accounts = vec![crate::config::Account {
        site: crate::game_download::Site::Lichess,
        name: "me_on_lichess".to_owned(),
        database: paths.databases_dir().join("my_games.sqlite"),
        // 2026-11-02 18:00 UTC and the day after.
        last_game_ms: Some(1_793_642_400_000),
        updated_at: Some(1_793_700_000),
    }];
    config.player_rating = Some(2061);
    let opts = [
        ("Threads", "16"),
        ("Hash", "4096"),
        ("MultiPV", "3"),
        ("SyzygyPath", "D:\\Tablebases\\syzygy"),
        ("UCI_ShowWDL", "true"),
    ];
    config.engines = vec![
        demo_engine(
            1,
            (
                "Stockfish",
                "17.1",
                "C:\\Engines\\stockfish\\stockfish-avx2.exe",
            ),
            true,
            (true, true, true),
            &opts,
        ),
        demo_engine(
            2,
            ("Lc0", "0.31", "C:\\Engines\\lc0\\lc0.exe"),
            true,
            (true, false, false),
            &opts,
        ),
        demo_engine(
            3,
            ("Maia", "1900", "C:\\Engines\\maia\\lc0.exe"),
            true,
            (false, false, true),
            &opts,
        ),
        demo_engine(
            4,
            ("Berserk", "13", "C:\\Engines\\berserk\\berserk.exe"),
            false,
            (false, false, false),
            &opts,
        ),
    ];
    config.next_engine_id = 5;

    let databases = paths.databases_dir();
    let stamp = databases.join(".generated-v3");
    if stamp.exists() {
        return;
    }
    let _ = std::fs::remove_dir_all(&paths.data_dir);
    if let Err(err) = std::fs::create_dir_all(&databases) {
        log::error!("could not create demo directory: {err}");
        return;
    }
    let mut rng = Rng(0x0DD5_EED5_0000_0042);
    my_games(&databases.join("my_games.sqlite"), &mut rng);
    reference_db(&databases.join("a_reference.sqlite"), &mut rng);
    small_db(
        &databases.join("b_najdorf.sqlite"),
        "Najdorf model games",
        186,
        "Model",
        ENGLISH_ATTACK,
        &mut rng,
    );
    small_db(
        &databases.join("c_opponent.sqlite"),
        "Opponent: knightowl_42",
        74,
        "knightowl_42",
        "d4 Nf6 c4 e6",
        &mut rng,
    );
    if let Ok(text) = serde_json::to_string(&repertoire(today)) {
        let _ = std::fs::write(paths.repertoire_file(), text);
    }
    if let Ok(notes) = db::NoteStore::open(paths.notes_file()) {
        let tree = chess_core::sample::najdorf();
        let end = *tree.mainline().last().unwrap_or(&GameTree::ROOT);
        let _ = notes.set(
            chess_core::position_hash(&tree.node(end).pos),
            "Check f4 vs h4 move order — I keep playing h4 here.",
        );
    }
    let _ = std::fs::write(stamp, "");
}

/// Timeline evals for the sample game, as in the design.
pub const TIMELINE: [i32; 26] = [
    14, 20, 17, 20, 14, 25, 20, 20, 17, 25, 20, 25, 20, 14, 17, 20, 25, 14, 42, 20, 75, 6, 25, -55,
    20, 42,
];

/// Scripted engine output for the sample position. Returns per-engine-name lines.
pub fn scripted_lines(hash: u64) -> HashMap<&'static str, Vec<AnalysisUpdate>> {
    let tree = chess_core::sample::najdorf();
    let end = *tree.mainline().last().unwrap_or(&GameTree::ROOT);
    let mut out = HashMap::new();
    if chess_core::position_hash(&tree.node(end).pos) != hash {
        return out;
    }
    let update = |multipv, cp, wdl: (u32, u32, u32), pv: &str| AnalysisUpdate {
        depth: 34,
        seldepth: Some(52),
        multipv,
        score: Some(Score::Cp(cp)),
        wdl: Some(Wdl {
            win: wdl.0,
            draw: wdl.1,
            loss: wdl.2,
        }),
        nodes: Some(1_200_000_000),
        nps: Some(58_000_000),
        tbhits: None,
        hashfull: Some(214),
        time_ms: Some(21_000),
        bound: false,
        pv: pv.split(' ').map(str::to_owned).collect(),
    };
    let f4 = "f3f4 a6a5 f4f5 e6c4 c1b1 a5a4 b3c1 c4e2 f1e2 a4a3 b2b3";
    let h4 = "h2h4 a6a5 c1b1 a5a4 b3c1 d6d5";
    let kb1 = "c1b1 a6a5 c2c3 b4c3 e2c3 a5a4";
    let ng3 = "e2g3 a6a5 c1b1 a5a4 b3c1 g7g6";
    let lines = vec![
        update(1, 42, (330, 520, 150), f4),
        update(2, 31, (310, 520, 170), h4),
        update(3, 18, (280, 560, 160), kb1),
        update(4, 5, (240, 550, 210), ng3),
    ];
    out.insert("Lc0", lines.clone());
    out.insert("Stockfish", lines);
    out
}

/// A scripted line written the way an engine would, for the Log panel.
pub fn info_line(update: &AnalysisUpdate) -> String {
    let score = match update.score {
        Some(Score::Cp(cp)) => format!("cp {cp}"),
        Some(Score::Mate(mate)) => format!("mate {mate}"),
        None => "cp 0".to_owned(),
    };
    format!(
        "info depth {} seldepth {} multipv {} score {score} nodes {} nps {} pv {}",
        update.depth,
        update.seldepth.unwrap_or(0),
        update.multipv,
        update.nodes.unwrap_or(0),
        update.nps.unwrap_or(0),
        update.pv.join(" "),
    )
}

/// One-line explanation shown in Line preview (the hook is empty outside demo mode).
pub fn explanation(san: &str) -> Option<&'static str> {
    (san == "f4").then_some(
        "Gains a tempo on the kingside and forces Black to commit the light-squared bishop before the queenside storm lands.",
    )
}

/// A mid-game Italian for the Play screen.
pub const PLAY_LINE: &str = "e4 e5 Nf3 Nc6 Bc4 Bc5 c3 Nf6";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn design_lines_are_legal() {
        let mut rng = Rng(7);
        for kind in [LINE, ENGLISH_ATTACK, PLAY_LINE] {
            assert_eq!(extend(kind, 0, &mut rng).len(), kind.split(' ').count());
        }
        assert_eq!(extend(ENGLISH_ATTACK, 40, &mut rng).len(), 40);
        let tree = chess_core::sample::najdorf();
        let end = *tree.mainline().last().unwrap();
        let hash = chess_core::position_hash(&tree.node(end).pos);
        let lines = &scripted_lines(hash)["Stockfish"];
        for line in lines {
            let sans = chess_core::pv_to_san(&tree.node(end).pos, &line.pv);
            assert_eq!(sans.len(), line.pv.len(), "illegal PV: {:?}", line.pv);
        }
    }

    #[test]
    fn repertoire_matches_the_design() {
        let today = 20_000;
        let rep = repertoire(today);
        assert_eq!(rep.positions(), 412);
        let rows = rep.tree_rows(today);
        assert_eq!(rows.len(), 12);
        assert_eq!(rows[4].label, "12…b4 13.Ne2");
        assert_eq!(rows[4].status, trainer::RepStatus::Due);
        let drills = rep.drills(today);
        let due = drills
            .iter()
            .filter(|drill| {
                let path: Vec<&str> = drill
                    .path
                    .iter()
                    .map(String::as_str)
                    .chain([drill.answer.as_str()])
                    .collect();
                rep.node(&path)
                    .and_then(|node| node.card)
                    .is_some_and(|card| card.is_due(today))
            })
            .count();
        // With the 6 reviews already done, the session reads "7 of 24".
        assert_eq!(due, 18);
        assert_eq!(drills[0].answer, "Ne2");
        assert_eq!(drills[0].path.len(), 24);
    }
}
