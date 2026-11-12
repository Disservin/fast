//! Optional position-pattern index, built atomically and maintained with game edits.

use crate::{Database, ImportControl, Result};
use chess_core::{GameTree, Position, PositionPattern, Variant, shakmaty::san::San};
use rusqlite::{Connection, params};

/// Stores the first occurrence of each consecutive pawn/material pattern in a game.
pub(crate) fn index_game(
    conn: &Connection,
    id: i64,
    fen: &str,
    moves: &str,
    chess960: bool,
    control: &ImportControl,
) -> Result<bool> {
    let tree = if fen.is_empty() {
        GameTree::new()
    } else if chess960 {
        GameTree::from_fen_variant(fen, Variant::Chess960).map_err(std::io::Error::other)?
    } else {
        GameTree::from_fen(fen).map_err(std::io::Error::other)?
    };
    let mut pos = tree.node(GameTree::ROOT).pos.clone();
    let mut last = None;
    let mut insert = conn.prepare_cached("INSERT OR REPLACE INTO structure_positions (game_id, ply, pawn_hash, material) VALUES (?1, ?2, ?3, ?4)")?;
    for (ply, san) in moves
        .split_ascii_whitespace()
        .map(Some)
        .chain(std::iter::once(None))
        .enumerate()
    {
        if control.is_cancelled() {
            return Ok(false);
        }
        let pattern = PositionPattern::of(&pos);
        if last != Some(pattern) {
            insert.execute(params![
                id,
                ply as i64,
                pattern.pawn_hash as i64,
                pattern.material as i64
            ])?;
            last = Some(pattern);
        }
        if let Some(san) = san {
            let mv = san
                .parse::<San>()
                .ok()
                .and_then(|san| san.to_move(&pos).ok())
                .ok_or_else(|| std::io::Error::other("Illegal move in structure index"))?;
            pos.play_unchecked(mv);
        }
    }
    Ok(true)
}

impl Database {
    /// Whether this database maintains an index for exact structure/material searches.
    pub fn has_structure_index(&self) -> Result<bool> {
        Ok(crate::meta(&self.conn, "structure_index")?.is_some_and(|value| value == "1"))
    }

    /// Indexes existing games, retaining the previous complete index if cancelled.
    /// Reports games processed and total; returns false on cancellation.
    pub fn build_structure_index(
        &mut self,
        control: &ImportControl,
        mut progress: impl FnMut(u64, u64),
    ) -> Result<bool> {
        let total = self.game_count()?;
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM structure_positions", [])?;
        let mut done = 0;
        {
            let mut games =
                tx.prepare("SELECT id, start_fen, moves, chess960 FROM games ORDER BY id")?;
            let mut rows = games.query([])?;
            while let Some(row) = rows.next()? {
                if !index_game(
                    &tx,
                    row.get(0)?,
                    &row.get::<_, String>(1)?,
                    &row.get::<_, String>(2)?,
                    row.get(3)?,
                    control,
                )? {
                    return Ok(false);
                }
                done += 1;
                if done % 100 == 0 {
                    progress(done, total);
                }
            }
        }
        if control.is_cancelled() {
            return Ok(false);
        }
        crate::set_meta(&tx, "structure_index", "1")?;
        tx.commit()?;
        progress(done, total);
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Filter, ImportOptions};

    /// Imports a tiny capture sequence with optional structure indexing.
    fn fixture(indexed: bool) -> (tempfile::TempDir, Database) {
        let options = ImportOptions {
            build_structure_index: indexed,
            ..Default::default()
        };
        crate::test_db("1. e4 d5 2. exd5 Qxd5 *", &options)
    }

    #[test]
    fn optional_index_requires_both_patterns_at_the_same_position() {
        let (_dir, mut database) = fixture(false);
        assert!(!database.has_structure_index().unwrap());
        assert_eq!(
            database
                .conn
                .query_row("SELECT COUNT(*) FROM structure_positions", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
        database
            .build_structure_index(&ImportControl::default(), |_, _| {})
            .unwrap();
        let tree = chess_core::parse_pgn("1. e4 d5 2. exd5 Qxd5 *")
            .unwrap()
            .remove(0);
        let line = tree.mainline();
        let early = PositionPattern::of(&tree.node(line[1]).pos);
        let late = PositionPattern::of(&tree.node(line[3]).pos);
        let pawns = Filter {
            pawn_structure: Some(early.pawn_hash),
            ..Default::default()
        };
        assert_eq!(database.filtered_ids(&pawns).unwrap(), vec![1]);
        let material = Filter {
            material: Some(late.material),
            ..Default::default()
        };
        assert_eq!(database.filtered_ids(&material).unwrap(), vec![1]);
        let combined = Filter {
            material: Some(late.material),
            ..pawns
        };
        assert!(database.filtered_ids(&combined).unwrap().is_empty());
    }

    #[test]
    fn indexed_imports_edits_and_deletions_keep_searches_current() {
        let (_dir, mut database) = fixture(true);
        assert!(database.has_structure_index().unwrap());
        let start = PositionPattern::of(&chess_core::Chess::default());
        let filter = Filter {
            pawn_structure: Some(start.pawn_hash),
            ..Default::default()
        };
        assert_eq!(database.filtered_ids(&filter).unwrap(), vec![1]);
        database
            .replace_game(1, "[FEN \"4k3/8/8/8/8/8/8/4K3 w - - 0 1\"]\n\n1. Kf1 *")
            .unwrap();
        assert!(database.filtered_ids(&filter).unwrap().is_empty());
        let empty = Filter {
            pawn_structure: Some(0),
            ..Default::default()
        };
        assert_eq!(database.filtered_ids(&empty).unwrap(), vec![1]);
        database.delete_games(&[1]).unwrap();
        assert!(database.filtered_ids(&empty).unwrap().is_empty());
        crate::import_text(database.path(), "1. d4 d5 *", &ImportOptions::default());
        assert_eq!(database.filtered_ids(&filter).unwrap().len(), 1);
    }

    #[test]
    fn cancelling_a_build_rolls_back_and_keeps_the_old_complete_index() {
        let (_dir, mut database) = fixture(true);
        let path = database.path().to_path_buf();
        crate::import_text(
            &path,
            &"1. d4 d5 *\n\n".repeat(101),
            &ImportOptions::default(),
        );
        let count = |database: &Database| {
            database
                .conn
                .query_row("SELECT COUNT(*) FROM structure_positions", [], |r| {
                    r.get::<_, i64>(0)
                })
                .unwrap()
        };
        let before = count(&database);
        let control = ImportControl::default();
        assert!(
            !database
                .build_structure_index(&control, |done, _| {
                    if done == 100 {
                        control.cancel();
                    }
                })
                .unwrap()
        );
        assert_eq!(count(&database), before);
        assert!(database.has_structure_index().unwrap());
    }
}
