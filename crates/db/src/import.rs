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
use pgn_reader::{Nag, RawComment, RawTag, Reader, SanPlus, Skip, Visitor};
use rusqlite::{Transaction, params};

use crate::{Database, Result, queries::TimeClass};

const BATCH_GAMES: u64 = 2_000;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ImportProgress {
    pub games: u64,
    pub skipped: u64,
    pub bytes: u64,
    /// Games stored with comments, NAGs or variations.
    pub annotated: u64,
    /// Variations left out because they held an illegal move.
    pub variations_dropped: u64,
    /// Games skipped because a game with the same URL is already stored.
    pub duplicates: u64,
    /// When the newest game read started, in milliseconds since 1970 (UTC), for
    /// asking a site only for newer games next time.
    pub latest_game_ms: Option<i64>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ImportSummary {
    pub games: u64,
    pub skipped: u64,
    pub cancelled: bool,
    /// Games stored with comments, NAGs or variations.
    pub annotated: u64,
    /// Variations left out because they held an illegal move.
    pub variations_dropped: u64,
    /// The number in the file (counting from 1) of each game that lost a variation.
    pub games_with_dropped: Vec<u64>,
    /// Games skipped because a game with the same URL is already stored.
    pub duplicates: u64,
    /// When the newest game read started, in milliseconds since 1970 (UTC).
    pub latest_game_ms: Option<i64>,
}

/// How games are read and stored by [`import_games`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportOptions {
    /// Shown in the Source column; empty guesses it from the Site tag.
    pub source: String,
    /// Keep comments, NAGs and variations in the stored movetext.
    pub keep_annotations: bool,
    /// Skip games whose URL (Site or Link tag) is already stored, as when downloading
    /// an account's games again.
    pub skip_known_urls: bool,
    /// Build and maintain the optional exact pawn/material index.
    pub build_structure_index: bool,
}

impl Default for ImportOptions {
    fn default() -> Self {
        Self {
            source: String::new(),
            keep_annotations: true,
            skip_known_urls: false,
            build_structure_index: false,
        }
    }
}

/// Cancellation flag shared with the importing thread.
#[derive(Clone, Default)]
pub struct ImportControl(Arc<AtomicBool>);

impl ImportControl {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    /// Whether Cancel was pressed.
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

#[derive(Default)]
pub(crate) struct Tags {
    pub(crate) white: String,
    pub(crate) black: String,
    pub(crate) white_elo: Option<i64>,
    pub(crate) black_elo: Option<i64>,
    pub(crate) result: String,
    pub(crate) date: String,
    eco: String,
    pub(crate) event: String,
    pub(crate) round: String,
    site: String,
    /// Chess.com puts the game's URL here and "Chess.com" in Site.
    link: String,
    utc_date: String,
    utc_time: String,
    pub(crate) time_control: String,
    fen: String,
    pub(crate) variant: String,
}

impl Tags {
    /// The game's URL, when its Site or Link tag holds one.
    fn url(&self) -> &str {
        [&self.site, &self.link]
            .into_iter()
            .find(|value| value.starts_with("http"))
            .map_or("", String::as_str)
    }

    /// When the game started, from UTCDate and UTCTime, in milliseconds since 1970.
    fn start_ms(&self) -> Option<i64> {
        let mut date = self.utc_date.split('.').map(str::parse::<i64>);
        let (year, month, day) = (date.next()?.ok()?, date.next()?.ok()?, date.next()?.ok()?);
        let mut time = self.utc_time.split(':').map(str::parse::<i64>);
        let (hour, minute, second) = (
            time.next().and_then(|part| part.ok()).unwrap_or(0),
            time.next().and_then(|part| part.ok()).unwrap_or(0),
            time.next().and_then(|part| part.ok()).unwrap_or(0),
        );
        if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
            return None;
        }
        let days = days_from_civil(year, month, day);
        Some(((days * 24 + hour) * 60 + minute) * 60_000 + second * 1000)
    }
}

/// Days from 1970-01-01 to a date in the proleptic Gregorian calendar.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let month_index = (month + 9) % 12;
    let day_of_year = (153 * month_index + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// One game read from a PGN, ready to store.
pub(crate) struct Game {
    pub(crate) tags: Tags,
    pub(crate) start_fen: String,
    pub(crate) sans: Vec<String>,
    /// The position before every move and after the last one.
    pub(crate) hashes: Vec<u64>,
    /// The full movetext with comments, NAGs and variations, when the game has any and
    /// they are kept. Without the closing result.
    pub(crate) annotated: Option<String>,
    /// Variations left out because they held an illegal move.
    pub(crate) variations_dropped: u64,
    /// The line being read: the mainline at the bottom, open variations above it.
    lines: Vec<Line>,
    /// The movetext written so far.
    text: String,
    /// Comments, NAGs and variations kept so far.
    extras: u64,
    /// The next move needs its number written, as after a comment or a variation.
    force_number: bool,
    /// The start of a comment longer than the reader's buffer.
    partial_comment: String,
}

/// The state of one line while reading: the mainline or an open variation.
struct Line {
    pos: Chess,
    /// The position before the last move of this line, where a variation starts.
    before_last: Option<Chess>,
    /// Plies played before the next move.
    ply: u32,
    /// An illegal move was found. On the mainline the game is skipped; in a variation
    /// the variation is dropped.
    failed: bool,
    /// Where this variation's text starts, to cut it when it is dropped.
    text_start: usize,
    /// `extras` before the variation, restored when it is dropped.
    extras_before: u64,
}

/// Reads each game; games with an illegal mainline move come out as `None`. With
/// `keep_annotations`, it also writes the full movetext with comments, NAGs and
/// variations; a variation with an illegal move is left out and the game kept.
#[derive(Default)]
pub(crate) struct ImportVisitor {
    pub(crate) keep_annotations: bool,
}

impl Game {
    fn line(&mut self) -> &mut Line {
        self.lines.last_mut().expect("the mainline is always there")
    }

    fn in_variation(&self) -> bool {
        self.lines.len() > 1
    }

    /// Whether the current line has stopped taking moves and annotations.
    fn line_failed(&self) -> bool {
        self.lines.last().is_some_and(|line| line.failed)
    }

    fn push_token(&mut self, token: &str) {
        if !self.text.is_empty() && !self.text.ends_with('(') {
            self.text.push(' ');
        }
        self.text.push_str(token);
    }
}

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
                if name == b"UTCDate" {
                    tags.utc_date = text();
                }
                if tags.date.is_empty() || name == b"Date" {
                    tags.date = text().replace('.', "-");
                }
            }
            b"ECO" => tags.eco = text(),
            b"Event" => tags.event = text(),
            b"Round" => tags.round = text(),
            b"Site" => tags.site = text(),
            b"Link" => tags.link = text(),
            b"UTCTime" => tags.utc_time = text(),
            b"TimeControl" => tags.time_control = text(),
            b"FEN" => tags.fen = text(),
            b"Variant" => tags.variant = text(),
            _ => {}
        }
        ControlFlow::Continue(())
    }

    fn begin_movetext(
        &mut self,
        mut tags: Self::Tags,
    ) -> ControlFlow<Self::Output, Self::Movetext> {
        let variant = chess_core::Variant::from_tag(&tags.variant);
        if variant == chess_core::Variant::Chess960 && tags.fen.is_empty() {
            tags.fen = chess_core::chess960::start_fen(518);
        }
        let (start_fen, pos) = if tags.fen.is_empty() {
            (String::new(), Chess::default())
        } else {
            let Ok(tree) = (if variant == chess_core::Variant::Chess960 {
                chess_core::GameTree::from_fen_variant(&tags.fen, variant)
            } else {
                chess_core::GameTree::from_fen(&tags.fen)
            }) else {
                return ControlFlow::Break(None);
            };
            (
                tags.fen.clone(),
                tree.node(chess_core::GameTree::ROOT).pos.clone(),
            )
        };
        let ply = (pos.fullmoves().get() - 1) * 2 + u32::from(pos.turn().is_black());
        ControlFlow::Continue(Game {
            tags,
            start_fen,
            sans: Vec::with_capacity(100),
            hashes: Vec::with_capacity(100),
            annotated: None,
            variations_dropped: 0,
            force_number: pos.turn().is_black(),
            lines: vec![Line {
                pos,
                before_last: None,
                ply,
                failed: false,
                text_start: 0,
                extras_before: 0,
            }],
            text: if self.keep_annotations {
                String::with_capacity(600)
            } else {
                String::new()
            },
            extras: 0,
            partial_comment: String::new(),
        })
    }

    fn begin_variation(&mut self, game: &mut Self::Movetext) -> ControlFlow<Self::Output, Skip> {
        if !self.keep_annotations || game.line_failed() {
            // Skipped variations still end with `end_variation`, so mark one.
            game.lines.push(Line {
                pos: Chess::default(),
                before_last: None,
                ply: 0,
                failed: true,
                text_start: game.text.len(),
                extras_before: game.extras,
            });
            return ControlFlow::Continue(Skip(true));
        }
        let line = game.line();
        let Some(start) = line.before_last.clone() else {
            // A variation before the first move has nothing to replace.
            let text_start = game.text.len();
            let extras_before = game.extras;
            game.lines.push(Line {
                pos: Chess::default(),
                before_last: None,
                ply: 0,
                failed: true,
                text_start,
                extras_before,
            });
            game.variations_dropped += 1;
            return ControlFlow::Continue(Skip(true));
        };
        let ply = line.ply - 1;
        let text_start = game.text.len();
        let extras_before = game.extras;
        game.push_token("(");
        game.lines.push(Line {
            pos: start,
            before_last: None,
            ply,
            failed: false,
            text_start,
            extras_before,
        });
        game.force_number = true;
        ControlFlow::Continue(Skip(false))
    }

    fn end_variation(&mut self, game: &mut Self::Movetext) -> ControlFlow<Self::Output> {
        if !game.in_variation() {
            return ControlFlow::Continue(());
        }
        let line = game.lines.pop().expect("checked above");
        if !self.keep_annotations {
            return ControlFlow::Continue(());
        }
        let opened = game.text.len() > line.text_start;
        if line.failed {
            // A dropped or skipped variation leaves no text behind. Skipped ones never
            // opened, and only the outermost failed variation counts.
            if opened {
                game.variations_dropped += 1;
            }
            game.text.truncate(line.text_start);
            game.extras = line.extras_before;
        } else if game.text.ends_with('(') {
            // An empty variation.
            game.text.truncate(line.text_start);
        } else {
            game.text.push(')');
            game.extras += 1;
        }
        game.force_number = true;
        ControlFlow::Continue(())
    }

    fn san(&mut self, game: &mut Self::Movetext, san_plus: SanPlus) -> ControlFlow<Self::Output> {
        if game.line_failed() {
            return ControlFlow::Continue(());
        }
        let in_variation = game.in_variation();
        let keep = self.keep_annotations;
        let line = game.line();
        let Ok(mv) = san_plus.san.to_move(&line.pos) else {
            line.failed = true;
            return ControlFlow::Continue(());
        };
        let before = line.pos.clone();
        let white = line.pos.turn().is_white();
        let number = line.ply / 2 + 1;
        line.pos.play_unchecked(mv);
        line.ply += 1;
        let hash_before = (!in_variation).then(|| position_hash(&before));
        line.before_last = Some(before);
        let san = san_plus.to_string();
        if keep {
            let token = if white {
                format!("{number}. {san}")
            } else if game.force_number {
                format!("{number}... {san}")
            } else {
                san.clone()
            };
            game.push_token(&token);
            game.force_number = false;
        }
        if let Some(hash) = hash_before {
            game.hashes.push(hash);
            game.sans.push(san);
        }
        ControlFlow::Continue(())
    }

    fn nag(&mut self, game: &mut Self::Movetext, nag: Nag) -> ControlFlow<Self::Output> {
        if self.keep_annotations && !game.line_failed() {
            game.push_token(&format!("${}", nag.0));
            game.extras += 1;
        }
        ControlFlow::Continue(())
    }

    fn partial_comment(
        &mut self,
        game: &mut Self::Movetext,
        comment: RawComment<'_>,
    ) -> ControlFlow<Self::Output> {
        if self.keep_annotations {
            game.partial_comment
                .push_str(&String::from_utf8_lossy(comment.as_bytes()));
        }
        ControlFlow::Continue(())
    }

    fn comment(
        &mut self,
        game: &mut Self::Movetext,
        comment: RawComment<'_>,
    ) -> ControlFlow<Self::Output> {
        if !self.keep_annotations {
            return ControlFlow::Continue(());
        }
        let mut text = std::mem::take(&mut game.partial_comment);
        text.push_str(&String::from_utf8_lossy(comment.as_bytes()));
        if game.line_failed() {
            return ControlFlow::Continue(());
        }
        let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
        if text.is_empty() {
            return ControlFlow::Continue(());
        }
        game.push_token(&format!("{{ {text} }}"));
        game.extras += 1;
        game.force_number = true;
        ControlFlow::Continue(())
    }

    fn end_game(&mut self, mut game: Self::Movetext) -> Self::Output {
        // Variations left open at the end of the game are closed or dropped.
        while game.in_variation() {
            let _ = self.end_variation(&mut game);
        }
        if game.line().failed {
            return None;
        }
        let pos = game.line().pos.clone();
        game.hashes.push(position_hash(&pos));
        if self.keep_annotations && game.extras > 0 {
            game.annotated = Some(std::mem::take(&mut game.text));
        }
        game.text = String::new();
        Some(game)
    }
}

/// The game's ECO tag, or the code of the opening its moves reach when it has none.
pub(crate) fn game_eco(game: &Game) -> String {
    if game.tags.eco.is_empty() {
        chess_core::classify(game.sans.iter().map(String::as_str))
            .map(|opening| opening.eco.to_owned())
            .unwrap_or_default()
    } else {
        game.tags.eco.clone()
    }
}

fn insert_game(tx: &Transaction<'_>, game: &Game, source: &str) -> Result<()> {
    let tags = &game.tags;
    let eco = game_eco(game);
    let mut insert_game = tx.prepare_cached(
        "INSERT INTO games (white, black, white_elo, black_elo, result, date, eco, event, round,
            start_fen, source, time_class, plies, moves, annotated, site, chess960)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
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
        if tags.result.is_empty() {
            "*"
        } else {
            tags.result.as_str()
        },
        tags.date,
        eco,
        tags.event,
        tags.round,
        game.start_fen,
        source,
        TimeClass::from_time_control(&tags.time_control).as_str(),
        game.sans.len() as i64,
        game.sans.join(" "),
        game.annotated,
        tags.url(),
        chess_core::Variant::from_tag(&tags.variant) == chess_core::Variant::Chess960
            || chess_core::fen_variant(&game.start_fen) == Some(chess_core::Variant::Chess960),
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

/// Whether a game with this URL is already stored.
fn url_known(tx: &Transaction<'_>, url: &str) -> Result<bool> {
    if url.is_empty() {
        return Ok(false);
    }
    let mut statement = tx.prepare_cached(
        "SELECT EXISTS (SELECT 1 FROM games WHERE site = ?1 AND site LIKE 'http%')",
    )?;
    Ok(statement.query_row(params![url], |row| row.get(0))?)
}

fn guess_source(site: &str) -> &'static str {
    if site.starts_with("http") || site.contains("lichess") || site.contains("chess.com") {
        "Online"
    } else {
        "OTB"
    }
}

/// Imports every readable game from `reader` in file order, so ascending ids follow
/// the file. Invalid games are skipped and counted. Only mainline moves are indexed
/// for position search and the opening tree.
pub fn import_games(
    path: &Path,
    reader: impl Read,
    options: &ImportOptions,
    control: &ImportControl,
    mut on_progress: impl FnMut(ImportProgress),
) -> Result<ImportSummary> {
    let mut database = Database::open(path)?;
    let mut indexed = database.has_structure_index()?;
    if options.build_structure_index && !indexed {
        indexed = database.build_structure_index(control, |_, _| {})?;
    }
    let counting = CountingReader {
        inner: reader,
        bytes: 0,
    };
    let mut pgn = Reader::new(counting);
    let mut visitor = ImportVisitor {
        keep_annotations: options.keep_annotations,
    };
    let mut progress = ImportProgress::default();
    let mut games_with_dropped = Vec::new();
    let mut read = 0u64;
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
            read += 1;
            if let Some(game) = &game
                && let Some(start) = game.tags.start_ms()
            {
                progress.latest_game_ms = Some(
                    progress
                        .latest_game_ms
                        .map_or(start, |latest| latest.max(start)),
                );
            }
            match game {
                Some(game) if options.skip_known_urls && url_known(&tx, game.tags.url())? => {
                    progress.duplicates += 1;
                }
                Some(game) => {
                    insert_game(&tx, &game, &options.source)?;
                    if indexed {
                        // Finish this game atomically; cancellation is checked before the next game.
                        crate::structure::index_game(
                            &tx,
                            tx.last_insert_rowid(),
                            &game.start_fen,
                            &game.sans.join(" "),
                            chess_core::Variant::from_tag(&game.tags.variant)
                                == chess_core::Variant::Chess960,
                            &ImportControl::default(),
                        )?;
                    }
                    progress.games += 1;
                    progress.annotated += u64::from(game.annotated.is_some());
                    if game.variations_dropped > 0 {
                        progress.variations_dropped += game.variations_dropped;
                        games_with_dropped.push(read);
                    }
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
        annotated: progress.annotated,
        variations_dropped: progress.variations_dropped,
        games_with_dropped,
        duplicates: progress.duplicates,
        latest_game_ms: progress.latest_game_ms,
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
        let summary = import_games(
            &path,
            "1. e4 e5 *\n".as_bytes(),
            &ImportOptions::default(),
            &control,
            |_| {},
        )
        .unwrap();
        assert!(summary.cancelled);
        assert_eq!(summary.games, 0);
    }

    #[test]
    fn invalid_games_are_skipped_not_fatal() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.sqlite");
        let text = "1. e4 e5 1-0\n\n1. e5 e4 1-0\n\n1. d4 d5 0-1\n";
        let summary = import_games(
            &path,
            text.as_bytes(),
            &ImportOptions::default(),
            &ImportControl::default(),
            |_| {},
        )
        .unwrap();
        assert_eq!(summary.games, 2);
        assert_eq!(summary.skipped, 1);
        assert_eq!(Database::open(&path).unwrap().game_count().unwrap(), 2);
    }

    #[test]
    fn chess960_import_keeps_variant_without_castling_rights() {
        for keep in [false, true] {
            let (_dir, database, summary) = import(
                "[Variant \"Chess960\"]\n[FEN \"4k3/8/8/8/8/8/8/5K2 w - - 0 1\"]\n\n1. Kg1 Kf7 *\n",
                keep,
            );
            assert_eq!(summary.games, 1);
            let stored = database.game(1).unwrap().unwrap();
            assert!(stored.chess960);
            let tree = chess_core::parse_pgn(&stored.to_pgn()).unwrap().remove(0);
            assert_eq!(tree.variant, chess_core::Variant::Chess960);
            assert_eq!(tree.mainline().len(), 2);
        }
    }

    fn import(text: &str, keep: bool) -> (tempfile::TempDir, Database, ImportSummary) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.sqlite");
        let options = ImportOptions {
            keep_annotations: keep,
            ..Default::default()
        };
        let summary = import_games(
            &path,
            text.as_bytes(),
            &options,
            &ImportControl::default(),
            |_| {},
        )
        .unwrap();
        (dir, Database::open(&path).unwrap(), summary)
    }

    /// The tree of each stored game, read back through its PGN.
    fn stored_trees(db: &Database) -> Vec<chess_core::GameTree> {
        let mut ids = db.filtered_ids(&crate::Filter::default()).unwrap();
        ids.sort_unstable();
        ids.iter()
            .map(|&id| {
                let pgn = db.game(id).unwrap().unwrap().to_pgn();
                chess_core::parse_pgn(&pgn).unwrap().remove(0)
            })
            .collect()
    }

    /// The movetext of a tree, which captures moves, comments, NAGs and variations.
    fn movetext(tree: &chess_core::GameTree) -> String {
        let pgn = chess_core::write_pgn(tree);
        pgn.split("\n\n").nth(1).unwrap().trim().to_owned()
    }

    const ANNOTATED: &str = r#"[Event "Book"]
[Result "1-0"]

{ Before the first move } 1. e4 c5 $1 2. Nf3 (2. c3 { Alapin } d5 (2... Nf6 3. e5) 3. exd5) $2 2... d6 !?
{ Main [%clk 0:01:00] } 3. d4 (3. Bb5+ Bd7 (3... Nd7 4. d4 (4. O-O)) 4. Bxd7+) 3... cxd4 1-0
"#;

    #[test]
    fn comments_variations_and_nags_round_trip() {
        let (_dir, db, summary) = import(ANNOTATED, true);
        assert_eq!((summary.games, summary.annotated), (1, 1));
        let original = chess_core::parse_pgn(ANNOTATED).unwrap().remove(0);
        let stored = stored_trees(&db).remove(0);
        assert_eq!(movetext(&stored), movetext(&original));
        assert!(movetext(&stored).contains("[%clk 0:01:00]"));
    }

    #[test]
    fn only_the_mainline_is_indexed() {
        let (_dir, db, _) = import(ANNOTATED, true);
        let mut pos = Chess::default();
        let mv = "e4".parse::<chess_core::shakmaty::san::San>().unwrap();
        pos.play_unchecked(mv.to_move(&pos).unwrap());
        let mv = "c5".parse::<chess_core::shakmaty::san::San>().unwrap();
        pos.play_unchecked(mv.to_move(&pos).unwrap());
        let moves = db.moves_from(position_hash(&pos)).unwrap();
        assert_eq!(moves.len(), 1);
        assert_eq!(moves[0].san, "Nf3");
        let game = db.game(1).unwrap().unwrap();
        assert_eq!(game.sans, vec!["e4", "c5", "Nf3", "d6", "d4", "cxd4"]);
    }

    #[test]
    fn plain_games_and_the_option_off_store_no_movetext() {
        let (_dir, db, summary) = import("1. e4 e5 2. Nf3 *\n", true);
        assert_eq!(summary.annotated, 0);
        assert_eq!(db.game(1).unwrap().unwrap().annotated, None);
        let (_dir, db, summary) = import(ANNOTATED, false);
        assert_eq!(summary.annotated, 0);
        let game = db.game(1).unwrap().unwrap();
        assert_eq!(game.annotated, None);
        assert_eq!(game.sans.len(), 6);
    }

    #[test]
    fn illegal_variation_is_dropped_and_the_game_kept() {
        let text =
            "1. e4 e5 (1... Ke3 2. d4) (1... c5 { Sicilian }) 2. Nf3 *\n\n1. d4 (1. Ke4) d5 *\n";
        let (_dir, db, summary) = import(text, true);
        assert_eq!(summary.games, 2);
        assert_eq!(summary.variations_dropped, 2);
        assert_eq!(summary.games_with_dropped, vec![1, 2]);
        let trees = stored_trees(&db);
        assert_eq!(
            movetext(&trees[0]),
            "1. e4 e5 (1... c5 { Sicilian }) 2. Nf3 *"
        );
        assert_eq!(db.game(2).unwrap().unwrap().annotated, None);
        assert_eq!(movetext(&trees[1]), "1. d4 d5 *");
    }

    #[test]
    fn custom_start_with_black_to_move() {
        let text = "[FEN \"4k3/8/8/8/8/8/4P3/4K3 b - - 0 1\"]\n\n1... Kd7 (1... Kf7 { or this }) 2. e4 *\n";
        let (_dir, db, summary) = import(text, true);
        assert_eq!(summary.annotated, 1);
        let original = chess_core::parse_pgn(text).unwrap().remove(0);
        let stored = stored_trees(&db).remove(0);
        assert_eq!(movetext(&stored), movetext(&original));
        assert_eq!(
            stored.header("FEN"),
            Some("4k3/8/8/8/8/8/4P3/4K3 b - - 0 1")
        );
    }

    const DOWNLOADED: &str = "[Site \"https://lichess.org/aaa\"]\n[UTCDate \"2024.03.01\"]\n[UTCTime \"10:00:00\"]\n\n1. e4 *\n\n\
        [Site \"Chess.com\"]\n[Link \"https://www.chess.com/game/live/1\"]\n[UTCDate \"2024.03.02\"]\n[UTCTime \"00:00:01\"]\n\n1. d4 *\n";

    #[test]
    fn downloads_skip_games_already_stored() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.sqlite");
        let options = ImportOptions {
            skip_known_urls: true,
            ..Default::default()
        };
        let run = || {
            import_games(
                &path,
                DOWNLOADED.as_bytes(),
                &options,
                &ImportControl::default(),
                |_| {},
            )
            .unwrap()
        };
        let first = run();
        assert_eq!((first.games, first.duplicates), (2, 0));
        // 2024-03-02 00:00:01 UTC.
        assert_eq!(first.latest_game_ms, Some(1_709_337_601_000));
        let second = run();
        assert_eq!((second.games, second.duplicates), (0, 2));
        // A file import keeps games that share a URL.
        let plain = import_games(
            &path,
            DOWNLOADED.as_bytes(),
            &ImportOptions::default(),
            &ImportControl::default(),
            |_| {},
        )
        .unwrap();
        assert_eq!(plain.games, 2);
    }

    #[test]
    fn civil_dates_count_days_from_1970() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2000, 3, 1), 11_017);
        assert_eq!(days_from_civil(1969, 12, 31), -1);
    }

    #[test]
    fn ids_follow_file_order() {
        let text = "[White \"first\"]\n\n1. e4 *\n\n[White \"second\"]\n\n1. d4 *\n\n[White \"third\"]\n\n1. c4 *\n";
        let (_dir, db, _) = import(text, true);
        let mut ids = db.filtered_ids(&crate::Filter::default()).unwrap();
        ids.sort_unstable();
        let rows = db.rows_by_ids(&ids).unwrap();
        let names: Vec<_> = rows.iter().map(|row| row.white.as_str()).collect();
        assert_eq!(names, ["first", "second", "third"]);
    }
}
