//! Reading fastchess output: progress lines, result blocks and games in its PGN file.

/// A line of fastchess output that Fast understands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Line<'a> {
    Started {
        game: u64,
        total: u64,
    },
    Finished {
        game: u64,
        white: &'a str,
        black: &'a str,
        result: &'a str,
        reason: &'a str,
    },
    /// The dashes around each results block.
    Rule,
    Text(&'a str),
}

pub fn parse_line(line: &str) -> Line<'_> {
    let parsed = (|| {
        if line.len() >= 20 && line.bytes().all(|b| b == b'-') {
            return Some(Line::Rule);
        }
        if let Some(rest) = line.strip_prefix("Started game ") {
            let (game, rest) = rest.split_once(" of ")?;
            let (total, _) = rest.split_once(' ')?;
            return Some(Line::Started {
                game: game.parse().ok()?,
                total: total.parse().ok()?,
            });
        }
        let rest = line.strip_prefix("Finished game ")?;
        let (game, rest) = rest.split_once(" (")?;
        let (players, outcome) = rest.split_once("): ")?;
        let (white, black) = players.split_once(" vs ")?;
        let (result, reason) = match outcome.split_once(" {") {
            Some((result, reason)) => (result, reason.trim_end_matches('}')),
            None => (outcome, ""),
        };
        Some(Line::Finished {
            game: game.parse().ok()?,
            white,
            black,
            result,
            reason,
        })
    })();
    parsed.unwrap_or(Line::Text(line))
}

/// A head-to-head results block as numbers, for the stat tiles. Scores are from the first
/// engine's side, as fastchess prints them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Results {
    /// Engine A and engine B, from the block's heading.
    pub engines: Option<(String, String)>,
    pub elo: f64,
    pub elo_err: f64,
    pub nelo: f64,
    pub nelo_err: f64,
    pub los: f64,
    pub draw_ratio: f64,
    pub pairs_ratio: f64,
    pub wl_dd_ratio: f64,
    pub games: u64,
    pub wins: u64,
    pub losses: u64,
    pub draws: u64,
    pub points: f64,
    pub score_pct: f64,
    /// Pentanomial counts from 0-2 to 2-0 points, for the pairs of games.
    pub ptnml: [u64; 5],
    /// Sequential test state, present only with SPRT.
    pub llr: Option<Llr>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Llr {
    pub value: f64,
    pub lower: f64,
    pub upper: f64,
}

/// Parses the numbers of a head-to-head block. `None` when the block is not one, such as
/// the table fastchess prints for more than two engines.
pub fn results(block: &[String]) -> Option<Results> {
    let mut out = Results::default();
    let (mut elo, mut los, mut games, mut ptnml, mut llr) = (false, false, false, false, None);
    for line in block {
        let line = line.trim();
        // The first number after `key` (ignoring commas and units).
        let number = |key: &str| -> Option<f64> {
            let rest = line.split_once(key)?.1;
            rest.split(|c: char| c == ',' || c.is_whitespace())
                .find(|token| !token.is_empty())?
                .parse()
                .ok()
        };
        if let Some(rest) = line.strip_prefix("Results of ") {
            let names = rest.trim_end_matches(':');
            out.engines = names.split_once(" vs ").map(|(a, b)| {
                let b = b.split(" (").next().unwrap_or(b);
                (
                    a.trim().to_owned(),
                    b.trim().trim_end_matches(':').to_owned(),
                )
            });
        } else if line.starts_with("Elo:") {
            // Elo: -58.45 +/- 98.05, nElo: -173.72 +/- 278.00
            let nums: Vec<f64> = line
                .split(|c: char| c == ',' || c.is_whitespace())
                .filter_map(|token| token.parse().ok())
                .collect();
            if let [e, ee, n, ne] = nums[..] {
                (out.elo, out.elo_err, out.nelo, out.nelo_err) = (e, ee, n, ne);
                elo = true;
            }
        } else if line.starts_with("LOS:") {
            out.los = number("LOS:").unwrap_or_default();
            out.draw_ratio = number("DrawRatio:").unwrap_or_default();
            out.pairs_ratio = number("PairsRatio:").unwrap_or_default();
            los = true;
        } else if line.starts_with("Games:") {
            out.games = number("Games:").unwrap_or_default() as u64;
            out.wins = number("Wins:").unwrap_or_default() as u64;
            out.losses = number("Losses:").unwrap_or_default() as u64;
            out.draws = number("Draws:").unwrap_or_default() as u64;
            out.points = number("Points:").unwrap_or_default();
            out.score_pct = line
                .split_once('(')
                .and_then(|(_, rest)| rest.split('%').next())
                .and_then(|pct| pct.trim().parse().ok())
                .unwrap_or_default();
            games = true;
        } else if line.starts_with("Ptnml(0-2):") {
            let inside = line
                .split_once('[')
                .and_then(|(_, rest)| rest.split_once(']'));
            if let Some((list, _)) = inside {
                let values: Vec<u64> = list
                    .split(',')
                    .filter_map(|v| v.trim().parse().ok())
                    .collect();
                if let Ok(values) = <[u64; 5]>::try_from(values) {
                    out.ptnml = values;
                    ptnml = true;
                }
            }
            out.wl_dd_ratio = number("Ratio:").unwrap_or_default();
        } else if line.starts_with("LLR:") {
            // LLR: -0.03 (-0.9%) (-2.94, 2.94) [0.00, 5.00]
            let value = number("LLR:");
            let bounds = line.rfind('(').and_then(|open| {
                let inner = line[open + 1..].split(')').next()?;
                let (lower, upper) = inner.split_once(',')?;
                Some((lower.trim().parse().ok()?, upper.trim().parse().ok()?))
            });
            if let (Some(value), Some((lower, upper))) = (value, bounds) {
                llr = Some(Llr {
                    value,
                    lower,
                    upper,
                });
            }
        }
    }
    (elo && los && games && ptnml).then(|| {
        out.llr = llr;
        out
    })
}

/// The game fastchess tagged `[GameId "id"]`, which builds with `-events` write.
pub fn game_by_id(pgn: &str, id: u64) -> Option<String> {
    let tag = format!("[GameId \"{id}\"]");
    (0..count_games(pgn))
        .filter_map(|index| nth_game(pgn, index))
        .find(|game| game.lines().any(|line| line.trim() == tag))
}

/// The `index`-th game (from 0) of a PGN file with several games.
pub fn nth_game(pgn: &str, index: usize) -> Option<String> {
    let mut starts: Vec<usize> = pgn
        .match_indices("[Event ")
        .map(|(at, _)| at)
        .filter(|&at| at == 0 || pgn[..at].ends_with('\n'))
        .collect();
    let start = *starts.get(index)?;
    starts.push(pgn.len());
    let end = starts[index + 1];
    Some(pgn[start..end].trim().to_owned())
}

pub fn count_games(pgn: &str) -> usize {
    pgn.match_indices("[Event ")
        .filter(|&(at, _)| at == 0 || pgn[..at].ends_with('\n'))
        .count()
}
