//! Games in progress, from fastchess's `-events` stream.
//!
//! fastchess builds with `-events` write one JSON object per line: `game_started`, `move`,
//! throttled `thinking`, and `game_finished`/`game_aborted`. Fast asks for that stream on
//! fastchess's stdout and keeps the games in progress here. Builds without `-events` (such
//! such as a custom executable built before 1.8.3) run without live boards.

use std::collections::{BTreeSet, HashMap};
use std::path::Path;
use std::process::{Command, Stdio};

use chess_core::{
    Chess, Position, Square,
    shakmaty::{CastlingMode, fen::Fen, uci::UciMove},
};

/// Options that send the event stream, with engine output while searching, to stdout.
pub fn events_args() -> Vec<String> {
    ["-events", "file=-", "info=true"].map(String::from).into()
}

/// Whether this fastchess build has the `-events` stream. Builds without it reject the
/// option before `-version` is reached.
pub fn has_events(program: &Path) -> bool {
    let mut command = Command::new(program);
    command
        .args(["-events", "file=-", "-version"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command.status().is_ok_and(|status| status.success())
}

/// Whether a command already sets up its own event stream, so Fast should not add one.
pub fn has_own_events(args: &[String]) -> bool {
    args.iter().any(|arg| arg == "-events")
}

/// Whether a line of fastchess output is an event rather than text for people.
pub fn is_event(line: &str) -> bool {
    line.starts_with("{\"event\":")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Score {
    Cp(i32),
    Mate(i32),
}

impl Score {
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
    /// From White's point of view, as fastchess reports it.
    pub score: Option<Score>,
    pub depth: Option<u32>,
    pub nodes: Option<u64>,
    pub pv: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LiveGame {
    /// Stable display position, reused by the next game once this one ends.
    pub slot: usize,
    /// fastchess's game number, as in `Finished game N` and the PGN's `GameId`.
    pub number: u64,
    pub white: Option<String>,
    pub black: Option<String>,
    pub start_fen: Option<String>,
    /// Book moves, then the engines' moves.
    pub moves: Vec<String>,
    /// The mover's score after each move, from White's point of view; `None` for book
    /// moves and moves reported without one. Parallel to `moves`.
    pub evals: Vec<Option<Score>>,
    /// Remaining time in milliseconds after the last move.
    pub wtime: Option<u64>,
    pub btime: Option<u64>,
    /// The engine searching now.
    pub thinking: Option<String>,
    pub white_search: Search,
    pub black_search: Search,
}

impl LiveGame {
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

/// All games in progress, by fastchess game number.
#[derive(Debug, Default)]
pub struct LiveGames {
    games: HashMap<u64, LiveGame>,
    /// Slots of finished games, handed to the next games so boards keep their places.
    free_slots: BTreeSet<usize>,
    next_slot: usize,
    /// Bumped on every change, so viewers redraw only when something moved.
    pub serial: u64,
}

fn json_score(value: &serde_json::Value) -> Option<Score> {
    let as_i32 = |value: &serde_json::Value| value.as_i64().and_then(|v| i32::try_from(v).ok());
    if let Some(cp) = value.get("cp").and_then(as_i32) {
        return Some(Score::Cp(cp));
    }
    value.get("mate").and_then(as_i32).map(Score::Mate)
}

fn json_moves(value: Option<&serde_json::Value>) -> Vec<String> {
    value
        .and_then(|moves| moves.as_array())
        .map(|moves| {
            moves
                .iter()
                .filter_map(|mv| mv.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

/// A `move` or `thinking` event's search output.
fn json_search(event: &serde_json::Value) -> Search {
    Search {
        score: event.get("score").and_then(json_score),
        depth: event
            .get("depth")
            .and_then(|depth| depth.as_u64())
            .and_then(|depth| u32::try_from(depth).ok()),
        nodes: event.get("nodes").and_then(|nodes| nodes.as_u64()),
        pv: json_moves(event.get("pv")),
    }
}

impl LiveGames {
    /// Takes one event line of fastchess output.
    pub fn feed(&mut self, line: &str) {
        if let Ok(event) = serde_json::from_str::<serde_json::Value>(line) {
            self.event(&event);
        }
    }

    fn take_slot(&mut self) -> usize {
        self.free_slots.pop_first().unwrap_or_else(|| {
            self.next_slot += 1;
            self.next_slot - 1
        })
    }

    fn event(&mut self, event: &serde_json::Value) {
        let text = |key: &str| event.get(key).and_then(|value| value.as_str());
        let time = |key: &str| event.get(key).and_then(|value| value.as_u64());
        let Some(number) = time("game") else {
            return;
        };
        match text("event").unwrap_or_default() {
            "game_started" => {
                let opening = json_moves(event.get("opening"));
                let slot = match self.games.get(&number) {
                    Some(game) => game.slot,
                    None => self.take_slot(),
                };
                self.games.insert(
                    number,
                    LiveGame {
                        slot,
                        number,
                        white: text("white").map(str::to_owned),
                        black: text("black").map(str::to_owned),
                        start_fen: text("start_fen").map(str::to_owned),
                        evals: vec![None; opening.len()],
                        moves: opening,
                        wtime: time("wtime"),
                        btime: time("btime"),
                        ..Default::default()
                    },
                );
            }
            "move" => {
                let Some(game) = self.games.get_mut(&number) else {
                    return;
                };
                let legal = event.get("legal").and_then(|legal| legal.as_bool()) != Some(false);
                let search = json_search(event);
                if let Some(mv) = text("move").filter(|_| legal) {
                    if time("ply").is_none_or(|ply| ply == game.moves.len() as u64 + 1) {
                        game.moves.push(mv.to_owned());
                        game.evals.resize(game.moves.len() - 1, None);
                        game.evals.push(search.score);
                    } else if let Some(fen) = text("fen") {
                        // A gap in the stream: start again from where the game stands.
                        game.start_fen = Some(fen.to_owned());
                        game.moves.clear();
                        game.evals.clear();
                    }
                }
                game.wtime = time("wtime");
                game.btime = time("btime");
                if text("side") == Some("white") {
                    game.white_search = search;
                    game.thinking = game.black.clone();
                } else {
                    game.black_search = search;
                    game.thinking = game.white.clone();
                }
            }
            "thinking" => {
                let Some(game) = self.games.get_mut(&number) else {
                    return;
                };
                let search = json_search(event);
                if text("side") == Some("white") {
                    game.white_search = search;
                } else {
                    game.black_search = search;
                }
                game.thinking = text("engine").map(str::to_owned);
            }
            "game_finished" | "game_aborted" => {
                if let Some(game) = self.games.remove(&number) {
                    self.free_slots.insert(game.slot);
                }
            }
            _ => return,
        }
        self.serial += 1;
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

    #[test]
    fn clocks_show_tenths_in_the_last_minute() {
        assert_eq!(format_clock(989), "0.9");
        assert_eq!(format_clock(61_500), "1:01");
    }

    #[test]
    fn follows_the_event_stream() {
        let mut live = LiveGames::default();
        assert!(!is_event("Started game 1 of 2 (a vs b)"));
        for line in [
            r#"{"event":"tournament_started","schema":1,"engines":["a","b"]}"#,
            r#"{"event":"game_started","game":1,"white":"a","black":"b","start_fen":"rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1","opening":["e2e4"],"wtime":1010,"btime":1010}"#,
            r#"{"event":"game_started","game":2,"white":"b","black":"a","start_fen":"rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1","opening":[],"wtime":1010,"btime":1010}"#,
            r#"{"event":"thinking","game":1,"side":"black","engine":"b","score":{"cp":-20},"depth":9,"pv":["c7c5"]}"#,
            r#"{"event":"move","game":1,"ply":2,"side":"black","engine":"b","move":"c7c5","legal":true,"wtime":1010,"btime":990,"score":{"mate":-4},"depth":12,"nodes":900,"pv":["c7c5","g1f3"]}"#,
        ] {
            assert!(is_event(line));
            live.feed(line);
        }
        let games = live.snapshot();
        assert_eq!(games.len(), 2);
        let first = &games[0];
        assert_eq!(first.number, 1);
        assert_eq!(first.moves, ["e2e4", "c7c5"]);
        // The book move has no eval; the engine move keeps the score it was played with.
        assert_eq!(first.evals, [None, Some(Score::Mate(-4))]);
        assert_eq!((first.wtime, first.btime), (Some(1010), Some(990)));
        // Event scores are already from White's side.
        assert_eq!(first.black_search.score, Some(Score::Mate(-4)));
        assert_eq!(first.black_search.nodes, Some(900));
        assert_eq!(first.thinking.as_deref(), Some("a"));
        assert_eq!(first.position().1, Some((Square::C7, Square::C5)));

        // A gap in the plies starts over from the reported position.
        live.feed(
            r#"{"event":"move","game":2,"ply":5,"side":"white","engine":"b","move":"d2d4","fen":"4k3/8/8/8/3P4/8/8/4K3 b - - 0 3","wtime":900,"btime":900}"#,
        );
        let second = &live.snapshot()[1];
        assert!(second.moves.is_empty());
        assert!(second.evals.is_empty());
        assert_eq!(second.position().0.turn(), chess_core::Color::Black);

        live.feed(r#"{"event":"game_finished","game":1,"result":"1-0"}"#);
        live.feed(r#"{"event":"game_aborted","game":2}"#);
        assert!(live.snapshot().is_empty());
        live.feed(r#"{"event":"game_started","game":3,"white":"a","black":"b","start_fen":"8/8/8/8/8/8/8/K1k5 b - - 0 1","opening":[]}"#);
        // The first free board position is reused.
        assert_eq!(live.snapshot()[0].slot, 0);
    }
}
