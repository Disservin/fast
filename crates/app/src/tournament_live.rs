//! Games in progress, rebuilt from fastchess's engine-communication log.
//!
//! fastchess has no move-by-move output, but it can log every UCI message it exchanges with
//! engines. Fast points that log at fastchess's own stdout and follows it: each game runs on
//! one fastchess thread, `ucinewgame` starts the next game on that thread, `position … moves`
//! carries the moves so far, `go` the clocks and `info` the engines' evaluations.
//! `docs/fastchess-integration.md` describes a move event that would replace this.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use chess_core::{
    Chess, Position, Square,
    shakmaty::{CastlingMode, fen::Fen, uci::UciMove},
};

/// Whether the log can be sent to fastchess's stdout. Windows has no `/dev/stdout`.
pub const SUPPORTED: bool = cfg!(unix);

/// Options that make fastchess log engine traffic to its stdout as it happens.
pub fn log_args() -> Vec<String> {
    [
        "-log",
        "file=/dev/stdout",
        "level=warn",
        "engine=true",
        "realtime=true",
    ]
    .map(String::from)
    .into()
}

/// Whether a command already configures logging itself, so Fast should not.
pub fn has_own_log(args: &[String]) -> bool {
    args.iter().any(|arg| arg == "-log")
}

/// fastchess saves the log options in `config.json`; put back its defaults so continuing
/// the tournament from a terminal does not print every engine message.
pub fn reset_saved_log(config: &Path) -> anyhow::Result<()> {
    let Ok(text) = std::fs::read_to_string(config) else {
        return Ok(());
    };
    let mut value: serde_json::Value = serde_json::from_str(&text)?;
    let Some(log) = value.get_mut("log").and_then(|log| log.as_object_mut()) else {
        return Ok(());
    };
    if log.get("file").and_then(|file| file.as_str()) != Some("/dev/stdout") {
        return Ok(());
    }
    log.insert("file".into(), "".into());
    log.insert("engine_coms".into(), false.into());
    let mut out = serde_json::to_string_pretty(&value)?;
    out.push('\n');
    crate::persistence::atomic_write(config, out.as_bytes())?;
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Score {
    Cp(i32),
    Mate(i32),
}

impl Score {
    fn negate(self) -> Self {
        match self {
            Self::Cp(cp) => Self::Cp(-cp),
            Self::Mate(moves) => Self::Mate(-moves),
        }
    }

    pub fn format(self) -> String {
        match self {
            Self::Cp(cp) => format!("{:+.2}", cp as f32 / 100.),
            Self::Mate(moves) => format!("#{moves}"),
        }
    }
}

/// An engine's latest search output in a game.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Search {
    /// From White's point of view.
    pub score: Option<Score>,
    pub depth: Option<u32>,
    pub nodes: Option<u64>,
    pub pv: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LiveGame {
    /// Stable display order: the fastchess thread's first appearance.
    pub slot: usize,
    /// Changes when the thread starts another game.
    pub game: u64,
    pub white: Option<String>,
    pub black: Option<String>,
    /// `None` for the standard initial position.
    pub start_fen: Option<String>,
    pub moves: Vec<String>,
    /// Remaining time in milliseconds at the last `go`.
    pub wtime: Option<u64>,
    pub btime: Option<u64>,
    /// The engine searching now.
    pub thinking: Option<String>,
    pub white_search: Search,
    pub black_search: Search,
}

impl LiveGame {
    fn white_to_move_at_start(&self) -> bool {
        self.start_fen
            .as_deref()
            .and_then(|fen| fen.split_whitespace().nth(1))
            .is_none_or(|side| side != "b")
    }

    /// The position after the moves that apply, and the last move's squares.
    pub fn position(&self) -> (Chess, Option<(Square, Square)>) {
        let mut pos: Chess = self
            .start_fen
            .as_deref()
            .and_then(|fen| fen.parse::<Fen>().ok())
            .and_then(|fen| fen.into_position(CastlingMode::Standard).ok())
            .unwrap_or_default();
        let mut last = None;
        for uci in &self.moves {
            let Some(mv) = uci
                .parse::<UciMove>()
                .ok()
                .and_then(|uci| uci.to_move(&pos).ok())
            else {
                break;
            };
            last = mv.from().map(|from| (from, mv.to()));
            pos.play_unchecked(mv);
        }
        (pos, last)
    }
}

/// All games in progress, keyed by fastchess thread.
#[derive(Debug, Default)]
pub struct LiveGames {
    games: HashMap<String, LiveGame>,
    /// Threads that sent `ucinewgame`; their next `position` starts a new game.
    starting: HashSet<String>,
    next_slot: usize,
    next_game: u64,
    /// Bumped on every change, so viewers redraw only when something moved.
    pub serial: u64,
}

/// One engine-log line: thread, engine, direction and UCI message.
fn split_line(line: &str) -> Option<(&str, &str, bool, &str)> {
    let rest = line.strip_prefix("[Engine]")?;
    let rest = &rest[rest.find("] <")? + 3..];
    let (thread, rest) = rest.split_once('>')?;
    let rest = rest.trim_start();
    if rest.starts_with("<stderr> ") {
        return None;
    }
    if let Some((engine, message)) = rest.split_once(" <--- ") {
        return Some((thread.trim(), engine, true, message));
    }
    let (engine, message) = rest.split_once(" ---> ")?;
    Some((thread.trim(), engine, false, message))
}

fn parse_search(message: &str, white: bool) -> Option<Search> {
    let mut tokens = message.split_whitespace();
    let mut search = Search::default();
    tokens.next().filter(|token| *token == "info")?;
    while let Some(token) = tokens.next() {
        match token {
            "depth" => search.depth = tokens.next().and_then(|value| value.parse().ok()),
            "nodes" => search.nodes = tokens.next().and_then(|value| value.parse().ok()),
            "score" => {
                let kind = tokens.next();
                let value = tokens.next().and_then(|value| value.parse().ok());
                search.score = match (kind, value) {
                    (Some("cp"), Some(cp)) => Some(Score::Cp(cp)),
                    (Some("mate"), Some(moves)) => Some(Score::Mate(moves)),
                    _ => None,
                };
            }
            "pv" => {
                search.pv = tokens.by_ref().map(str::to_owned).collect();
            }
            // Free text runs to the end of the line.
            "string" => return None,
            _ => {}
        }
    }
    let score = search.score?;
    search.score = Some(if white { score } else { score.negate() });
    Some(search)
}

impl LiveGames {
    /// Takes a line of fastchess output; returns whether it was engine traffic.
    pub fn feed(&mut self, line: &str) -> bool {
        if !line.starts_with("[Engine]") {
            return false;
        }
        if let Some((thread, engine, to_engine, message)) = split_line(line) {
            if to_engine {
                self.sent(thread, engine, message);
            } else {
                self.received(thread, engine, message);
            }
        }
        true
    }

    fn sent(&mut self, thread: &str, engine: &str, message: &str) {
        let command = message.split_whitespace().next().unwrap_or_default();
        match command {
            "ucinewgame" => {
                self.starting.insert(thread.to_owned());
            }
            "position" => self.position(thread, engine, message),
            "go" => {
                let Some(game) = self.games.get_mut(thread) else {
                    return;
                };
                let mut tokens = message.split_whitespace();
                while let Some(token) = tokens.next() {
                    match token {
                        "wtime" => game.wtime = tokens.next().and_then(|value| value.parse().ok()),
                        "btime" => game.btime = tokens.next().and_then(|value| value.parse().ok()),
                        _ => {}
                    }
                }
                game.thinking = Some(engine.to_owned());
                self.serial += 1;
            }
            _ => {}
        }
    }

    fn position(&mut self, thread: &str, engine: &str, message: &str) {
        let rest = message.strip_prefix("position").unwrap_or_default().trim();
        let (start, moves) = match rest.split_once(" moves") {
            Some((start, moves)) => (
                start.trim(),
                moves.split_whitespace().map(str::to_owned).collect(),
            ),
            None => (rest, Vec::new()),
        };
        let start_fen = start.strip_prefix("fen").map(|fen| fen.trim().to_owned());
        let starting = self.starting.remove(thread);
        let fresh = match self.games.get(thread) {
            None => true,
            Some(game) => starting || game.start_fen != start_fen,
        };
        if fresh {
            let slot = match self.games.get(thread) {
                Some(game) => game.slot,
                None => {
                    self.next_slot += 1;
                    self.next_slot - 1
                }
            };
            self.next_game += 1;
            self.games.insert(
                thread.to_owned(),
                LiveGame {
                    slot,
                    game: self.next_game,
                    start_fen,
                    ..Default::default()
                },
            );
        }
        let Some(game) = self.games.get_mut(thread) else {
            return;
        };
        game.moves = moves;
        // The engine asked to move plays the side to move.
        let white = game.white_to_move_at_start() == (game.moves.len() % 2 == 0);
        let side = if white {
            &mut game.white
        } else {
            &mut game.black
        };
        if side.is_none() {
            *side = Some(engine.to_owned());
        }
        self.serial += 1;
    }

    fn received(&mut self, thread: &str, engine: &str, message: &str) {
        let Some(game) = self.games.get_mut(thread) else {
            return;
        };
        let white = game.white.as_deref() == Some(engine);
        if !white && game.black.as_deref() != Some(engine) {
            return;
        }
        if message.starts_with("info ") {
            if let Some(search) = parse_search(message, white) {
                if white {
                    game.white_search = search;
                } else {
                    game.black_search = search;
                }
                self.serial += 1;
            }
        } else if let Some(rest) = message.strip_prefix("bestmove ") {
            // Show the move now; the next `position` confirms it.
            if game.thinking.as_deref() == Some(engine)
                && let Some(mv) = rest.split_whitespace().next()
            {
                game.moves.push(mv.to_owned());
                game.thinking = None;
                self.serial += 1;
            }
        }
    }

    /// The games, in display order.
    pub fn snapshot(&self) -> Vec<LiveGame> {
        let mut games: Vec<_> = self.games.values().cloned().collect();
        games.sort_by_key(|game| game.slot);
        games
    }
}

/// Clock text: m:ss, with tenths under a minute.
pub fn format_clock(ms: u64) -> String {
    if ms < 60_000 {
        format!("{}.{}", ms / 1000, ms % 1000 / 100)
    } else {
        format!("{}:{:02}", ms / 60_000, ms / 1000 % 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn engine(thread: &str, name: &str, to: bool, message: &str) -> String {
        format!(
            "[Engine] [19:02:20.553742] <{thread:>20}>  {name} {} {message}",
            if to { "<---" } else { "--->" }
        )
    }

    #[test]
    fn follows_games_per_thread() {
        let mut live = LiveGames::default();
        assert!(!live.feed("Started game 1 of 2 (a vs b c)"));
        for line in [
            engine("1", "a", true, "ucinewgame"),
            engine("1", "b c", true, "ucinewgame"),
            engine("1", "a", true, "position startpos"),
            engine("1", "a", true, "go wtime 1010 btime 1010 winc 10 binc 10"),
            engine(
                "1",
                "a",
                false,
                "info depth 12 seldepth 15 score cp 31 nodes 5000 pv d2d4 d7d5",
            ),
            engine("1", "a", false, "bestmove d2d4 ponder d7d5"),
            engine(
                "2",
                "b c",
                true,
                "position fen 4k3/8/8/8/8/8/8/4K2R b K - 0 1",
            ),
            engine("1", "b c", true, "position startpos moves d2d4"),
            engine("1", "b c", true, "go wtime 989 btime 1010 winc 10 binc 10"),
            engine("1", "b c", false, "info depth 9 score mate -3 pv e7e5"),
            "[Engine] [19:05:48.069344] <8769744>  <stderr> a ---> exited".to_owned(),
        ] {
            assert!(live.feed(&line), "{line}");
        }
        let games = live.snapshot();
        assert_eq!(games.len(), 2);
        let first = &games[0];
        assert_eq!(first.white.as_deref(), Some("a"));
        assert_eq!(first.black.as_deref(), Some("b c"));
        assert_eq!(first.moves, ["d2d4"]);
        assert_eq!((first.wtime, first.btime), (Some(989), Some(1010)));
        assert_eq!(first.white_search.score, Some(Score::Cp(31)));
        assert_eq!(first.white_search.pv, ["d2d4", "d7d5"]);
        // Black's mate against itself is a White mate from White's side.
        assert_eq!(first.black_search.score, Some(Score::Mate(3)));
        let (pos, last) = first.position();
        assert_eq!(pos.turn(), chess_core::Color::Black);
        assert_eq!(last, Some((Square::D2, Square::D4)));
        let second = &games[1];
        assert_eq!(second.black.as_deref(), Some("b c"));
        assert_eq!(second.position().0.turn(), chess_core::Color::Black);

        let before = first.game;
        live.feed(&engine("1", "a", true, "ucinewgame"));
        live.feed(&engine("1", "b c", true, "position startpos"));
        let games = live.snapshot();
        assert_eq!(games[0].slot, 0);
        assert_ne!(games[0].game, before);
        assert_eq!(games[0].white.as_deref(), Some("b c"));
        assert!(games[0].moves.is_empty());
    }

    #[test]
    fn saved_log_options_are_reset() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(
            &path,
            r#"{"log": {"file": "/dev/stdout", "level": 3, "engine_coms": true}, "rounds": 2}"#,
        )
        .unwrap();
        reset_saved_log(&path).unwrap();
        let value: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(value["log"]["file"], "");
        assert_eq!(value["log"]["engine_coms"], false);
        assert_eq!(value["rounds"], 2);
    }

    #[test]
    fn clocks_show_tenths_in_the_last_minute() {
        assert_eq!(format_clock(989), "0.9");
        assert_eq!(format_clock(61_500), "1:01");
    }
}
