//! Database descriptions, collection statistics and maintenance previews.

use rusqlite::params;

use crate::{Database, Result};

/// Collection-wide figures shown while managing a database.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DatabaseStats {
    pub games: u64,
    pub players: u64,
    pub events: u64,
    pub structure_indexed: bool,
}

impl Database {
    /// The user's description of this collection, empty until one is saved.
    pub fn description(&self) -> Result<String> {
        Ok(crate::meta(&self.conn, "description")?.unwrap_or_default())
    }

    /// Stores a collection's name and description together.
    pub fn set_details(&mut self, name: &str, description: &str) -> Result<()> {
        self.in_transaction(|db| {
            db.set_name(name)?;
            crate::set_meta(&db.conn, "description", description)
        })
    }

    /// Counts distinct nonempty players and events across the whole collection.
    pub fn database_stats(&self) -> Result<DatabaseStats> {
        Ok(DatabaseStats {
            games: self.game_count()?,
            players: self.conn.query_row("SELECT COUNT(*) FROM (SELECT white COLLATE NOCASE AS name FROM games WHERE trim(white) NOT IN ('', '?') UNION SELECT black COLLATE NOCASE FROM games WHERE trim(black) NOT IN ('', '?'))", [], |row| row.get::<_, i64>(0))? as u64,
            events: self.conn.query_row("SELECT COUNT(DISTINCT event COLLATE NOCASE) FROM games WHERE trim(event) NOT IN ('', '?')", [], |row| row.get::<_, i64>(0))? as u64,
            structure_indexed: self.has_structure_index()?,
        })
    }

    /// Replaces an exact player name on both sides, leaving other names untouched.
    pub fn merge_player(&mut self, from: &str, into: &str) -> Result<u64> {
        if from.trim().is_empty() || into.trim().is_empty() || from == into {
            return Ok(0);
        }
        self.in_transaction(|db| {
            let count = db.conn.query_row("SELECT COUNT(*) FROM games WHERE white = ?1 COLLATE NOCASE OR black = ?1 COLLATE NOCASE", params![from], |row| row.get::<_, i64>(0))? as u64;
            db.conn.execute("UPDATE games SET white = CASE WHEN white = ?1 COLLATE NOCASE THEN ?2 ELSE white END, black = CASE WHEN black = ?1 COLLATE NOCASE THEN ?2 ELSE black END WHERE white = ?1 COLLATE NOCASE OR black = ?1 COLLATE NOCASE", params![from, into])?;
            Ok(count)
        })
    }

    /// Previews games without moves before the user confirms their deletion.
    pub fn empty_game_ids(&self) -> Result<Vec<i64>> {
        let mut query = self
            .conn
            .prepare("SELECT id FROM games WHERE plies = 0 AND trim(moves) = '' ORDER BY id")?;
        Ok(query
            .query_map([], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?)
    }

    /// Previews exact duplicates, preserving different annotations and preferring reviewed copies.
    pub fn duplicate_game_ids(&self) -> Result<Vec<i64>> {
        let mut query = self.conn.prepare("SELECT id FROM (SELECT id, ROW_NUMBER() OVER (PARTITION BY white, black, white_elo, black_elo, result, date, event, round, start_fen, chess960, moves, COALESCE(annotated, ''), site, time_class ORDER BY EXISTS(SELECT 1 FROM review_summaries r WHERE r.game_id = games.id) DESC, id) AS rank FROM games) WHERE rank > 1 ORDER BY id")?;
        Ok(query
            .query_map([], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?)
    }
}

#[cfg(test)]
mod tests {
    use crate::ImportOptions;
    use chess_core::{Chess, position_hash};

    /// Maintenance keeps index entries, annotations and unrelated player names intact.
    #[test]
    fn manages_collection_without_losing_distinct_games() {
        let games = "[White \"Alice\"]\n[Black \"Bob\"]\n[Event \"Open\"]\n[Result \"1-0\"]\n\n1. e4 e5 1-0\n\n";
        let annotated = games.replace("e4 e5", "e4 {Keep this note} e5");
        let empty = "[White \"Alicia\"]\n[Black \"ALICE\"]\n[Result \"*\"]\n\n*\n";
        let options = ImportOptions {
            keep_annotations: true,
            ..Default::default()
        };
        let (_dir, mut database) =
            crate::test_db(&format!("{games}{games}{annotated}{empty}"), &options);
        database.set_details("Collection", "A description").unwrap();
        assert_eq!(database.name().unwrap(), "Collection");
        assert_eq!(database.description().unwrap(), "A description");
        assert_eq!(database.database_stats().unwrap().players, 3);
        assert_eq!(database.database_stats().unwrap().events, 1);
        assert_eq!(database.merge_player("alice", "Alice Smith").unwrap(), 4);
        assert_eq!(database.game(4).unwrap().unwrap().row.white, "Alicia");
        let duplicates = database.duplicate_game_ids().unwrap();
        assert_eq!(duplicates, vec![2]);
        database.delete_games(&duplicates).unwrap();
        assert_eq!(
            database
                .moves_from(position_hash(&Chess::default()))
                .unwrap()[0]
                .games,
            2
        );
        assert!(
            database
                .game(3)
                .unwrap()
                .unwrap()
                .to_pgn()
                .contains("Keep this note")
        );
        let empty = database.empty_game_ids().unwrap();
        assert_eq!(empty, vec![4]);
        database.delete_games(&empty).unwrap();
        assert_eq!(database.game_count().unwrap(), 2);
    }
}
