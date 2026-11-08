use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};

use crate::Result;

/// Free-text notes keyed by position hash.
pub struct NoteStore {
    conn: Connection,
}

impl NoteStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = crate::open_sqlite(path.as_ref())?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS notes (hash INTEGER PRIMARY KEY, text TEXT NOT NULL)",
        )?;
        Ok(Self { conn })
    }

    pub fn get(&self, hash: u64) -> Result<String> {
        let text: Option<String> = self
            .conn
            .query_row(
                "SELECT text FROM notes WHERE hash = ?1",
                params![hash as i64],
                |row| row.get(0),
            )
            .optional()?;
        Ok(text.unwrap_or_default())
    }

    /// Saves a note; an empty note removes the row.
    pub fn set(&self, hash: u64, text: &str) -> Result<()> {
        if text.is_empty() {
            self.conn
                .execute("DELETE FROM notes WHERE hash = ?1", params![hash as i64])?;
        } else {
            self.conn.execute(
                "INSERT OR REPLACE INTO notes (hash, text) VALUES (?1, ?2)",
                params![hash as i64, text],
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notes_round_trip_including_high_bit_hashes() {
        let dir = tempfile::tempdir().unwrap();
        let store = NoteStore::open(dir.path().join("notes.sqlite")).unwrap();
        let hash = u64::MAX - 5;
        assert_eq!(store.get(hash).unwrap(), "");
        store.set(hash, "Check f4 vs h4").unwrap();
        assert_eq!(store.get(hash).unwrap(), "Check f4 vs h4");
        store.set(hash, "").unwrap();
        assert_eq!(store.get(hash).unwrap(), "");
    }
}
