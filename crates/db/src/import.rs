//! Streaming PGN import. Runs on a background thread with its own connection and
//! commits in batches, so readers keep working and memory stays flat.

use std::{
    io::Read,
    ops::ControlFlow,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use chess_core::{Chess, Position, position_hash};
use pgn_reader::{RawTag, Reader, SanPlus, Skip, Visitor};
use rusqlite::{Transaction, params};

use crate::{Database, Result, queries::TimeClass};

const BATCH_GAMES: u64 = 2_000;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ImportProgress {
    pub games: u64,
    pub skipped: u64,
    pub bytes: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImportSummary {
    pub games: u64,
    pub skipped: u64,
    pub cancelled: bool,
}

/// Cancellation flag shared with the importing thread.
#[derive(Clone, Default)]
pub struct ImportControl(Arc<AtomicBool>);

impl ImportControl {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

#[derive(Default)]
struct Tags {
    white: String,
    black: String,
    white_elo: Option<i64>,
    black_elo: Option<i64>,
    result: String,
    date: String,
    eco: String,
    event: String,
    round: String,
    site: String,
    time_control: String,
    fen: bool,
}

struct Game {
    tags: Tags,
    pos: Chess,
    sans: Vec<String>,
    hashes: Vec<u64>,
    failed: bool,
}

struct ImportVisitor;

impl Visitor for ImportVisitor {
    type Tags = Tags;
    type Movetext = Game;
    type Output = Option<Game>;

    fn begin_tags(&mut self) -> ControlFlow<Self::Output, Self::Tags> {
        ControlFlow::Continue(Tags::default())
    }

    fn tag(
        &mut self,
        tags: &mut Self::Tags,
        name: &[u8],
        value: RawTag<'_>,
    ) -> ControlFlow<Self::Output> {
        let text = || value.decode_utf8_lossy().into_owned();
        match name {
            b"White" => tags.white = text(),
            b"Black" => tags.black = text(),
            b"WhiteElo" => tags.white_elo = text().parse().ok(),
            b"BlackElo" => tags.black_elo = text().parse().ok(),
            b"Result" => tags.result = text(),
            b"Date" | b"UTCDate" => {
                if tags.date.is_empty() || name == b"Date" {
                    tags.date = text().replace('.', "-");
                }
            }
            b"ECO" => tags.eco = text(),
            b"Event" => tags.event = text(),
            b"Round" => tags.round = text(),
            b"Site" => tags.site = text(),
            b"TimeControl" => tags.time_control = text(),
            b"FEN" => tags.fen = true,
            _ => {}
        }
        ControlFlow::Continue(())
    }

    fn begin_movetext(&mut self, tags: Self::Tags) -> ControlFlow<Self::Output, Self::Movetext> {
        // Custom starting positions cannot use the shared position index.
        if tags.fen {
            return ControlFlow::Break(None);
        }
        ControlFlow::Continue(Game {
            tags,
            pos: Chess::default(),
            sans: Vec::with_capacity(100),
            hashes: Vec::with_capacity(100),
            failed: false,
        })
    }

    fn begin_variation(
        &mut self,
        _movetext: &mut Self::Movetext,
    ) -> ControlFlow<Self::Output, Skip> {
        ControlFlow::Continue(Skip(true))
    }

    fn san(
        &mut self,
        game: &mut Self::Movetext,
        san_plus: SanPlus,
    ) -> ControlFlow<Self::Output> {
        if game.failed {
            return ControlFlow::Continue(());
        }
        match san_plus.san.to_move(&game.pos) {
            Ok(mv) => {
                game.hashes.push(position_hash(&game.pos));
                game.sans.push(san_plus.to_string());
                game.pos.play_unchecked(mv);
            }
            Err(_) => game.failed = true,
        }
        ControlFlow::Continue(())
    }

    fn end_game(&mut self, mut game: Self::Movetext) -> Self::Output {
        if game.failed {
            return None;
        }
        game.hashes.push(position_hash(&game.pos));
        Some(game)
    }
}

fn insert_game(tx: &Transaction<'_>, game: &Game, source: &str) -> Result<()> {
    let tags = &game.tags;
    let eco = if tags.eco.is_empty() {
        chess_core::classify(game.sans.iter().map(String::as_str))
            .map(|opening| opening.eco.to_owned())
            .unwrap_or_default()
    } else {
        tags.eco.clone()
    };
    let mut insert_game = tx.prepare_cached(
        "INSERT INTO games (white, black, white_elo, black_elo, result, date, eco, event, round,
            source, time_class, plies, moves)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
    )?;
    let source = if source.is_empty() {
        guess_source(&tags.site)
    } else {
        source
    };
    insert_game.execute(params![
        tags.white,
        tags.black,
        tags.white_elo,
        tags.black_elo,
        if tags.result.is_empty() { "*" } else { tags.result.as_str() },
        tags.date,
        eco,
        tags.event,
        tags.round,
        source,
        TimeClass::from_time_control(&tags.time_control).as_str(),
        game.sans.len() as i64,
        game.sans.join(" "),
    ])?;
    let game_id = tx.last_insert_rowid();
    let mut insert_position = tx.prepare_cached(
        "INSERT OR IGNORE INTO positions (hash, game_id, ply, next_san) VALUES (?1, ?2, ?3, ?4)",
    )?;
    for (ply, hash) in game.hashes.iter().enumerate() {
        insert_position.execute(params![
            *hash as i64,
            game_id,
            ply as i64,
            game.sans.get(ply)
        ])?;
    }
    Ok(())
}

fn guess_source(site: &str) -> &'static str {
    if site.starts_with("http") || site.contains("lichess") || site.contains("chess.com") {
        "Online"
    } else {
        "OTB"
    }
}

/// Imports every readable game from `reader`. Invalid games are skipped and counted.
pub fn import_games(
    path: &Path,
    reader: impl Read,
    source: &str,
    control: &ImportControl,
    mut on_progress: impl FnMut(ImportProgress),
) -> Result<ImportSummary> {
    let mut database = Database::open(path)?;
    let counting = CountingReader {
        inner: reader,
        bytes: 0,
    };
    let mut pgn = Reader::new(counting);
    let mut visitor = ImportVisitor;
    let mut progress = ImportProgress::default();
    let mut cancelled = false;
    'outer: loop {
        let tx = database.conn.transaction()?;
        let mut in_batch = 0;
        while in_batch < BATCH_GAMES {
            if control.is_cancelled() {
                tx.commit()?;
                cancelled = true;
                break 'outer;
            }
            let Some(game) = pgn.read_game(&mut visitor)? else {
                tx.commit()?;
                break 'outer;
            };
            match game {
                Some(game) => {
                    insert_game(&tx, &game, source)?;
                    progress.games += 1;
                    in_batch += 1;
                }
                None => progress.skipped += 1,
            }
        }
        tx.commit()?;
        on_progress(progress);
    }
    on_progress(progress);
    Ok(ImportSummary {
        games: progress.games,
        skipped: progress.skipped,
        cancelled,
    })
}

struct CountingReader<R> {
    inner: R,
    bytes: u64,
}

impl<R: Read> Read for CountingReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let read = self.inner.read(buf)?;
        self.bytes += read as u64;
        Ok(read)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancel_stops_early_and_keeps_committed_games() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.sqlite");
        let control = ImportControl::default();
        control.cancel();
        let summary = import_games(&path, "1. e4 e5 *\n".as_bytes(), "", &control, |_| {}).unwrap();
        assert!(summary.cancelled);
        assert_eq!(summary.games, 0);
    }

    #[test]
    fn invalid_games_are_skipped_not_fatal() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.sqlite");
        let text = "1. e4 e5 1-0\n\n1. e5 e4 1-0\n\n1. d4 d5 0-1\n";
        let summary =
            import_games(&path, text.as_bytes(), "", &ImportControl::default(), |_| {}).unwrap();
        assert_eq!(summary.games, 2);
        assert_eq!(summary.skipped, 1);
        assert_eq!(Database::open(&path).unwrap().game_count().unwrap(), 2);
    }
}
