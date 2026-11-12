//! Changing a database after import: game headers, moves, deleting games and copying the
//! whole file.

use std::path::Path;

use chess_core::{Chess, Position, position_hash, shakmaty::san::San};
use pgn_reader::Reader;
use rusqlite::{OptionalExtension, params};

use crate::{
    Database, Result,
    import::{ImportVisitor, game_eco},
    queries::{GameRow, TimeClass},
};

/// The header fields a user can change on a stored game.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GameInfo {
    pub white: String,
    pub black: String,
    pub white_elo: Option<i64>,
    pub black_elo: Option<i64>,
    pub result: String,
    pub date: String,
    pub event: String,
    pub round: String,
    pub eco: String,
}

impl GameInfo {
    /// The editable headers of a row from the games table.
    pub fn from_row(row: &GameRow) -> Self {
        Self {
            white: row.white.clone(),
            black: row.black.clone(),
            white_elo: row.white_elo,
            black_elo: row.black_elo,
            result: row.result.clone(),
            date: row.date.clone(),
            event: row.event.clone(),
            round: row.round.clone(),
            eco: row.eco.clone(),
        }
    }
}

/// The movetext of a PGN without its tags and its closing result, as stored for a game
/// with comments or variations.
fn movetext(pgn: &str) -> String {
    let body: Vec<&str> = pgn
        .lines()
        .skip_while(|line| line.trim_start().starts_with('[') || line.trim().is_empty())
        .collect();
    let mut text = body.join("\n").trim().to_owned();
    for result in ["1-0", "0-1", "1/2-1/2", "*"] {
        if let Some(rest) = text.strip_suffix(result) {
            text = rest.trim_end().to_owned();
            break;
        }
    }
    text
}

/// The position hash before every move and after the last one, as the positions table
/// stores them.
fn line_hashes(start_fen: &str, moves: &str) -> Vec<u64> {
    let mut pos = if start_fen.is_empty() {
        Chess::default()
    } else {
        match chess_core::GameTree::from_fen(start_fen) {
            Ok(tree) => tree.node(chess_core::GameTree::ROOT).pos.clone(),
            Err(_) => return Vec::new(),
        }
    };
    let mut hashes = vec![position_hash(&pos)];
    for san in moves.split_ascii_whitespace() {
        let Some(mv) = san
            .parse::<San>()
            .ok()
            .and_then(|san| san.to_move(&pos).ok())
        else {
            break;
        };
        pos.play_unchecked(mv);
        hashes.push(position_hash(&pos));
    }
    hashes
}

impl Database {
    /// Renames players, fixes the result and the other headers of one game.
    pub fn update_game_info(&self, id: i64, info: &GameInfo) -> Result<()> {
        self.conn.execute(
            "UPDATE games SET white = ?2, black = ?3, white_elo = ?4, black_elo = ?5,
                result = ?6, date = ?7, event = ?8, round = ?9, eco = ?10
             WHERE id = ?1",
            params![
                id,
                info.white,
                info.black,
                info.white_elo,
                info.black_elo,
                if info.result.is_empty() {
                    "*"
                } else {
                    info.result.as_str()
                },
                info.date,
                info.event,
                info.round,
                info.eco,
            ],
        )?;
        Ok(())
    }

    /// Removes a game's rows from the position index, using its own moves so that only
    /// its entries are touched rather than the whole table.
    fn unindex_game(&self, id: i64) -> Result<()> {
        let stored: Option<(String, String)> = self
            .conn
            .query_row(
                "SELECT start_fen, moves FROM games WHERE id = ?1",
                params![id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((start_fen, moves)) = stored else {
            return Ok(());
        };
        let mut delete = self
            .conn
            .prepare_cached("DELETE FROM positions WHERE hash = ?1 AND game_id = ?2")?;
        for hash in line_hashes(&start_fen, &moves) {
            delete.execute(params![hash as i64, id])?;
        }
        Ok(())
    }

    /// Forgets the engine review of a game, for when its moves change.
    fn clear_review(&self, id: i64) -> Result<()> {
        self.conn.execute(
            "DELETE FROM review_positions WHERE game_id = ?1",
            params![id],
        )?;
        self.conn.execute(
            "DELETE FROM review_summaries WHERE game_id = ?1",
            params![id],
        )?;
        self.conn.execute(
            "UPDATE games SET accuracy = NULL, left_rep = NULL WHERE id = ?1",
            params![id],
        )?;
        Ok(())
    }

    /// Deletes games with their positions and reviews. Returns how many were removed.
    pub fn delete_games(&mut self, ids: &[i64]) -> Result<u64> {
        self.in_transaction(|db| {
            let mut removed = 0;
            for &id in ids {
                db.unindex_game(id)?;
                db.conn.execute(
                    "DELETE FROM structure_positions WHERE game_id = ?1",
                    params![id],
                )?;
                db.conn.execute(
                    "DELETE FROM review_positions WHERE game_id = ?1",
                    params![id],
                )?;
                db.conn.execute(
                    "DELETE FROM review_summaries WHERE game_id = ?1",
                    params![id],
                )?;
                removed += db
                    .conn
                    .execute("DELETE FROM games WHERE id = ?1", params![id])?
                    as u64;
            }
            Ok(removed)
        })
    }

    /// Replaces a game's headers and moves with the first game in `pgn`, keeping its
    /// place in the database. The engine review is kept only when the moves are the
    /// same. Returns false when `pgn` holds no game that can be read.
    pub fn replace_game(&mut self, id: i64, pgn: &str) -> Result<bool> {
        let Some(Some(game)) =
            Reader::new(pgn.as_bytes()).read_game(&mut ImportVisitor::default())?
        else {
            return Ok(false);
        };
        let moves = game.sans.join(" ");
        self.in_transaction(|db| {
            let old: Option<(String, String, bool)> = db
                .conn
                .query_row(
                    "SELECT start_fen, moves, chess960 FROM games WHERE id = ?1",
                    params![id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .optional()?;
            let Some((old_fen, old_moves, old_chess960)) = old else {
                return Ok(false);
            };
            let tags = &game.tags;
            db.conn.execute(
                "UPDATE games SET white = ?2, black = ?3, white_elo = ?4, black_elo = ?5,
                    result = ?6, date = ?7, event = ?8, round = ?9, eco = ?10
                 WHERE id = ?1",
                params![
                    id,
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
                    tags.event,
                    tags.round,
                    game_eco(&game),
                ],
            )?;
            // A PGN written by the app carries no time control; keep the stored class.
            if !tags.time_control.is_empty() {
                db.conn.execute(
                    "UPDATE games SET time_class = ?2 WHERE id = ?1",
                    params![
                        id,
                        TimeClass::from_time_control(&tags.time_control).as_str()
                    ],
                )?;
            }
            db.conn.execute(
                "UPDATE games SET annotated = ?2 WHERE id = ?1",
                params![id, movetext(pgn)],
            )?;
            let chess960 = game.is_chess960();
            if old_fen == game.start_fen && old_moves == moves && old_chess960 == chess960 {
                return Ok(true);
            }
            db.unindex_game(id)?;
            db.conn.execute("DELETE FROM structure_positions WHERE game_id = ?1", params![id])?;
            if db.has_structure_index()? {
                let control = crate::ImportControl::default();
                crate::structure::index_game(
                    &db.conn,
                    id,
                    &game.start_fen,
                    &moves,
                    chess960,
                    &control,
                )?;
            }
            db.clear_review(id)?;
            db.conn.execute(
                "UPDATE games SET start_fen = ?2, moves = ?3, plies = ?4, chess960 = ?5 WHERE id = ?1",
                params![id, game.start_fen, moves, game.sans.len() as i64, chess960],
            )?;
            let mut insert = db.conn.prepare_cached(
                "INSERT OR IGNORE INTO positions (hash, game_id, ply, next_san)
                 VALUES (?1, ?2, ?3, ?4)",
            )?;
            for (ply, hash) in game.hashes.iter().enumerate() {
                insert.execute(params![*hash as i64, id, ply as i64, game.sans.get(ply)])?;
            }
            Ok(true)
        })
    }

    /// Writes a complete copy of this database to `dest` under a new display name.
    pub fn duplicate_to(&self, dest: &Path, name: &str) -> Result<()> {
        self.conn.execute(
            "VACUUM INTO ?1",
            params![dest.to_string_lossy().into_owned()],
        )?;
        Database::open(dest)?.set_name(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Filter, ImportControl, ImportOptions, ReviewPosition, import_games};

    const GAMES: &str = "[White \"me\"]\n[Black \"alice\"]\n[Result \"1-0\"]\n\n1. e4 c5 2. Nf3 d6 1-0\n\n\
        [White \"bob\"]\n[Black \"me\"]\n[Result \"0-1\"]\n\n1. d4 d5 0-1\n";

    fn database() -> (tempfile::TempDir, Database) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("games.sqlite");
        import_games(
            &path,
            GAMES.as_bytes(),
            &ImportOptions::default(),
            &ImportControl::default(),
            |_| {},
        )
        .unwrap();
        let db = Database::open(&path).unwrap();
        (dir, db)
    }

    fn ids(db: &Database) -> Vec<i64> {
        let mut ids = db.filtered_ids(&Filter::default()).unwrap();
        ids.sort_unstable();
        ids
    }

    fn position_rows(db: &Database, id: i64) -> i64 {
        db.conn
            .query_row(
                "SELECT COUNT(*) FROM positions WHERE game_id = ?1",
                params![id],
                |row| row.get(0),
            )
            .unwrap()
    }

    #[test]
    fn headers_change_without_touching_moves() {
        let (_dir, db) = database();
        let id = ids(&db)[0];
        let mut info = GameInfo::from_row(&db.game(id).unwrap().unwrap().row);
        info.white = "Carlsen".into();
        info.white_elo = Some(2830);
        info.result = "1/2-1/2".into();
        db.update_game_info(id, &info).unwrap();
        let game = db.game(id).unwrap().unwrap();
        assert_eq!(game.row.white, "Carlsen");
        assert_eq!(game.row.white_elo, Some(2830));
        assert_eq!(game.row.result, "1/2-1/2");
        assert_eq!(game.sans, vec!["e4", "c5", "Nf3", "d6"]);
    }

    #[test]
    fn deleting_removes_positions_and_reviews() {
        let (_dir, mut db) = database();
        let [first, second] = ids(&db)[..] else {
            panic!("two games expected")
        };
        db.set_review_position(
            first,
            "job",
            &ReviewPosition {
                ply: 0,
                fen: String::new(),
                eval_cp: 20,
                mate_distance: None,
                depth: 10,
                pv: String::new(),
            },
        )
        .unwrap();
        assert_eq!(position_rows(&db, first), 5);
        assert_eq!(db.delete_games(&[first]).unwrap(), 1);
        assert_eq!(position_rows(&db, first), 0);
        assert!(db.review_positions(first, "job").unwrap().is_empty());
        assert_eq!(ids(&db), vec![second]);
        assert_eq!(position_rows(&db, second), 3);
    }

    #[test]
    fn replacing_moves_reindexes_and_keeps_the_id() {
        let (_dir, mut db) = database();
        let id = ids(&db)[0];
        let pgn = "[White \"me\"]\n[Black \"alice\"]\n[Result \"0-1\"]\n\n1. e4 e5 2. Nf3 Nc6 3. Bb5 0-1\n";
        assert!(db.replace_game(id, pgn).unwrap());
        let game = db.game(id).unwrap().unwrap();
        assert_eq!(game.sans, vec!["e4", "e5", "Nf3", "Nc6", "Bb5"]);
        assert_eq!(game.row.result, "0-1");
        assert_eq!(game.row.plies, 5);
        assert_eq!(position_rows(&db, id), 6);
        let after_e5 = db.moves_from(line_hashes("", "e4 e5")[2]).unwrap();
        assert_eq!(after_e5[0].san, "Nf3");
        assert!(
            db.moves_from(line_hashes("", "e4 c5")[2])
                .unwrap()
                .is_empty()
        );
        assert!(!db.replace_game(id, "").unwrap());
    }

    #[test]
    fn comments_and_variations_survive_a_save() {
        let (_dir, mut db) = database();
        let id = ids(&db)[0];
        let pgn = "[White \"me\"]\n[Black \"alice\"]\n[Result \"1-0\"]\n\n\
            1. e4 { Best by test } c5 (1... e5 2. Nf3) 2. Nf3 $1 d6 1-0\n";
        assert!(db.replace_game(id, pgn).unwrap());
        let game = db.game(id).unwrap().unwrap();
        assert_eq!(game.sans, vec!["e4", "c5", "Nf3", "d6"]);
        let mut info = GameInfo::from_row(&game.row);
        info.result = "0-1".into();
        db.update_game_info(id, &info).unwrap();
        let game = db.game(id).unwrap().unwrap();
        let tree = chess_core::parse_pgn(&game.to_pgn()).unwrap().remove(0);
        let first = tree.mainline()[0];
        assert_eq!(tree.node(first).comment, "Best by test");
        assert_eq!(tree.node(first).children.len(), 2);
        assert_eq!(tree.header("Result"), Some("0-1"));
        assert!(game.to_pgn().trim_end().ends_with("d6 0-1"));
    }

    #[test]
    fn duplicate_copies_games_under_a_new_name() {
        let (dir, db) = database();
        let dest = dir.path().join("copy.sqlite");
        db.duplicate_to(&dest, "Copy").unwrap();
        let copy = Database::open(&dest).unwrap();
        assert_eq!(copy.name().unwrap(), "Copy");
        assert_eq!(copy.game_count().unwrap(), 2);
        assert_eq!(db.name().unwrap(), "games");
    }
}
