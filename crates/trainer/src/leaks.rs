//! Finds where real games left the repertoire and what it cost.

use std::collections::HashMap;

use crate::Repertoire;

/// A game reduced to what leak analysis needs. `score` is the user's result (0, 0.5, 1).
#[derive(Clone, Debug)]
pub struct GameLine {
    pub sans: Vec<String>,
    pub user_white: bool,
    pub score: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Leak {
    /// Moves before the deviation.
    pub path: Vec<String>,
    /// What the user played.
    pub played: String,
    /// The prepared move, or `None` if the opponent left book / the node has no answer.
    pub prepared: Option<String>,
    pub games: u32,
    /// The user's average score in these games.
    pub score: f32,
    /// The user's average score in other games reaching the prepared move, if any.
    pub prepared_score: Option<f32>,
}

/// Where a game first leaves the repertoire on the user's own move.
/// Returns the ply index of the deviating move.
pub fn deviation(repertoire: &Repertoire, sans: &[String], user_white: bool) -> Option<usize> {
    if repertoire.white != user_white {
        return None;
    }
    for (ply, san) in sans.iter().enumerate() {
        let path = &sans[..ply];
        let moves = repertoire.moves_after(path)?;
        if moves.is_empty() {
            return None;
        }
        let clean = san.trim_end_matches(['+', '#']);
        if !moves.iter().any(|node| node.san == clean) {
            let user_move = (ply % 2 == 0) == user_white;
            return user_move.then_some(ply);
        }
    }
    None
}

/// Groups deviations, most frequent first (ties by earlier move).
pub fn find_leaks(repertoire: &Repertoire, games: &[GameLine]) -> Vec<Leak> {
    #[derive(Default)]
    struct Tally {
        games: u32,
        score: f32,
    }
    let mut leaks: HashMap<(Vec<String>, String), Tally> = HashMap::new();
    let mut followed: HashMap<(Vec<String>, String), Tally> = HashMap::new();
    for game in games {
        let deviated = deviation(repertoire, &game.sans, game.user_white);
        if let Some(ply) = deviated {
            let tally = leaks
                .entry((game.sans[..ply].to_vec(), game.sans[ply].clone()))
                .or_default();
            tally.games += 1;
            tally.score += game.score;
        }
        // Record the moves followed in book before any deviation.
        let end = deviated.unwrap_or(game.sans.len());
        for ply in 0..end {
            let tally = followed
                .entry((game.sans[..ply].to_vec(), game.sans[ply].clone()))
                .or_default();
            tally.games += 1;
            tally.score += game.score;
        }
    }
    let mut out: Vec<Leak> = leaks
        .into_iter()
        .map(|((path, played), tally)| {
            let prepared = repertoire
                .moves_after(&path)
                .and_then(|moves| moves.first())
                .map(|node| node.san.clone());
            let prepared_score = prepared.as_ref().and_then(|san| {
                followed
                    .get(&(path.clone(), san.clone()))
                    .map(|tally| tally.score / tally.games as f32)
            });
            Leak {
                score: tally.score / tally.games as f32,
                games: tally.games,
                path,
                played,
                prepared,
                prepared_score,
            }
        })
        .collect();
    out.sort_by(|a, b| {
        b.games
            .cmp(&a.games)
            .then(a.path.len().cmp(&b.path.len()))
            .then(a.played.cmp(&b.played))
    });
    out
}

/// A position the user's games keep reaching where the repertoire has no answer yet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BookGap {
    /// Moves up to and including the first move beyond the book.
    pub path: Vec<String>,
    pub games: u32,
}

/// Lines where the book ran out because of the opponent's move, most frequent first.
pub fn find_book_gaps(repertoire: &Repertoire, games: &[GameLine]) -> Vec<BookGap> {
    let mut gaps: HashMap<Vec<String>, u32> = HashMap::new();
    for game in games {
        if game.user_white != repertoire.white {
            continue;
        }
        for ply in 0..game.sans.len() {
            let Some(moves) = repertoire.moves_after(&game.sans[..ply]) else {
                break;
            };
            let clean = game.sans[ply].trim_end_matches(['+', '#']);
            if moves.iter().any(|node| node.san == clean) {
                continue;
            }
            let opponent_move = (ply % 2 == 0) != game.user_white;
            if moves.is_empty() && opponent_move {
                *gaps.entry(game.sans[..=ply].to_vec()).or_default() += 1;
            }
            break;
        }
    }
    let mut out: Vec<BookGap> = gaps
        .into_iter()
        .map(|(path, games)| BookGap { path, games })
        .collect();
    out.sort_by(|a, b| b.games.cmp(&a.games).then(a.path.cmp(&b.path)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(moves: &str, user_white: bool, score: f32) -> GameLine {
        GameLine {
            sans: moves.split(' ').map(str::to_owned).collect(),
            user_white,
            score,
        }
    }

    fn rep() -> Repertoire {
        let mut rep = Repertoire::new("r", true);
        rep.add_line(&["e4", "c5", "Nf3", "d6", "d4"]).unwrap();
        rep
    }

    #[test]
    fn deviation_only_counts_the_users_moves() {
        let rep = rep();
        let user_left = line("e4 c5 Nc3 d6", true, 1.0);
        assert_eq!(deviation(&rep, &user_left.sans, true), Some(2));
        // The opponent left book first: not the user's leak.
        let opponent_left = line("e4 e6 d4", true, 1.0);
        assert_eq!(deviation(&rep, &opponent_left.sans, true), None);
        // Book exhausted without deviating.
        let in_book = line("e4 c5 Nf3 d6 d4 cxd4", true, 1.0);
        assert_eq!(deviation(&rep, &in_book.sans, true), None);
        // Repertoire for the other colour does not apply.
        assert_eq!(deviation(&rep, &user_left.sans, false), None);
    }

    #[test]
    fn book_gaps_count_opponent_moves_past_the_end_of_book() {
        let mut rep = rep();
        rep.add_line(&["e4", "e6", "d4"]).unwrap();
        let games = [
            line("e4 e6 d4 a6 Nf3", true, 1.0),
            line("e4 e6 d4 a6 c4", true, 0.0),
            line("e4 e6 Nf3 d5", true, 0.0),
        ];
        let gaps = find_book_gaps(&rep, &games);
        assert_eq!(gaps.len(), 1);
        assert_eq!(gaps[0].games, 2);
        assert_eq!(gaps[0].path.join(" "), "e4 e6 d4 a6");
    }

    #[test]
    fn leaks_group_by_position_with_scores() {
        let rep = rep();
        let games = [
            line("e4 c5 Nc3 d6", true, 0.0),
            line("e4 c5 Nc3 Nc6", true, 1.0),
            line("e4 c5 c3 d5", true, 0.0),
            line("e4 c5 Nf3 d6 d4", true, 1.0),
            line("e4 c5 Nf3 e6", true, 0.5),
        ];
        let leaks = find_leaks(&rep, &games);
        assert_eq!(leaks.len(), 2);
        assert_eq!(leaks[0].played, "Nc3");
        assert_eq!(leaks[0].games, 2);
        assert!((leaks[0].score - 0.5).abs() < f32::EPSILON);
        assert_eq!(leaks[0].prepared.as_deref(), Some("Nf3"));
        assert!((leaks[0].prepared_score.unwrap() - 0.75).abs() < f32::EPSILON);
    }
}
