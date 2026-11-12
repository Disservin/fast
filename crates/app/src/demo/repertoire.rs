//! The demo repertoire, with cards at every stage of learning.

use super::*;

/// Card states needed to display each curated row as in the design.
pub(super) fn card_known(today: i64) -> Card {
    Card {
        state: CardState::Review,
        ease: 2.5,
        interval_days: 21,
        due_day: today + 14,
        lapses: 0,
    }
}

pub(super) fn card_learning(today: i64) -> Card {
    Card {
        state: CardState::Review,
        ease: 2.3,
        interval_days: 3,
        due_day: today + 2,
        lapses: 1,
    }
}

pub(super) fn card_due(today: i64, age: i64) -> Card {
    Card {
        state: CardState::Learning,
        ease: 2.3,
        interval_days: 2,
        due_day: today - age,
        lapses: 0,
    }
}

pub(super) fn repertoire(today: i64) -> Repertoire {
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

pub(super) fn apply_row(
    rep: &mut Repertoire,
    path: &[&str],
    depth: usize,
    label: &str,
    card: Option<Card>,
) {
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
