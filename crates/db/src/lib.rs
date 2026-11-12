//! SQLite game databases: one file per database, position search by Zobrist hash.

mod edit;
mod import;
mod management;
mod notes;
mod queries;
mod structure;

pub use edit::GameInfo;
pub use import::{ImportControl, ImportOptions, ImportProgress, ImportSummary, import_games};
pub use management::DatabaseStats;
pub use notes::NoteStore;
pub use queries::{
    Filter, FilterStats, GameDetails, GameRow, MoveStat, PlayerLine, ResultFilter, ReviewPosition,
    ReviewSummary, SideFilter, Sort, SortKey, TimeClass, date_bound,
};

use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DbError {
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, DbError>;

/// Opens an SQLite file, creating it and its directory when missing.
pub fn open_sqlite(path: &Path) -> Result<Connection> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    Ok(Connection::open(path)?)
}

const SCHEMA: &str = r#"
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS games (
    id INTEGER PRIMARY KEY,
    white TEXT NOT NULL,
    black TEXT NOT NULL,
    white_elo INTEGER,
    black_elo INTEGER,
    result TEXT NOT NULL,
    date TEXT NOT NULL,
    eco TEXT NOT NULL,
    event TEXT NOT NULL,
    round TEXT NOT NULL,
    start_fen TEXT NOT NULL DEFAULT '',
    chess960 INTEGER NOT NULL DEFAULT 0,
    annotated TEXT,
    site TEXT NOT NULL DEFAULT '',
    source TEXT NOT NULL,
    time_class TEXT NOT NULL,
    plies INTEGER NOT NULL,
    accuracy REAL,
    left_rep TEXT,
    moves TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS games_date ON games (date DESC, id DESC);
CREATE INDEX IF NOT EXISTS games_white ON games (white COLLATE NOCASE);
CREATE INDEX IF NOT EXISTS games_black ON games (black COLLATE NOCASE);
CREATE TABLE IF NOT EXISTS positions (
    hash INTEGER NOT NULL,
    game_id INTEGER NOT NULL,
    ply INTEGER NOT NULL,
    next_san TEXT,
    PRIMARY KEY (hash, game_id, ply)
) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS structure_positions (
    game_id INTEGER NOT NULL,
    ply INTEGER NOT NULL,
    pawn_hash INTEGER NOT NULL,
    material INTEGER NOT NULL,
    PRIMARY KEY (game_id, ply)
) WITHOUT ROWID;
CREATE INDEX IF NOT EXISTS structure_pawns ON structure_positions (pawn_hash, material, game_id);
CREATE INDEX IF NOT EXISTS structure_material ON structure_positions (material, game_id);
CREATE TABLE IF NOT EXISTS review_positions (
    game_id INTEGER NOT NULL,
    job_key TEXT NOT NULL,
    ply INTEGER NOT NULL,
    fen TEXT NOT NULL,
    eval_cp INTEGER NOT NULL,
    mate_distance INTEGER,
    depth INTEGER NOT NULL,
    pv TEXT NOT NULL,
    PRIMARY KEY (game_id, job_key, ply)
) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS review_summaries (
    game_id INTEGER NOT NULL,
    job_key TEXT NOT NULL,
    engine TEXT NOT NULL,
    search_limit TEXT NOT NULL,
    white_accuracy REAL,
    black_accuracy REAL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (game_id, job_key)
) WITHOUT ROWID;
"#;

/// Columns added after the first schema, with their types, so older files gain them on open.
const ADDED_COLUMNS: [(&str, &str, &str); 5] = [
    ("games", "chess960", "INTEGER NOT NULL DEFAULT 0"),
    ("games", "start_fen", "TEXT NOT NULL DEFAULT ''"),
    ("games", "annotated", "TEXT"),
    ("games", "site", "TEXT NOT NULL DEFAULT ''"),
    ("review_positions", "mate_distance", "INTEGER"),
];

/// A value stored in the `meta` table, such as the database's name.
pub(crate) fn meta(conn: &Connection, key: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row("SELECT value FROM meta WHERE key = ?1", [key], |row| {
            row.get(0)
        })
        .optional()?)
}

/// Stores `value` under `key` in the `meta` table.
pub(crate) fn set_meta(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO meta (key, value) VALUES (?1, ?2)",
        [key, value],
    )?;
    Ok(())
}

/// One open database file. Not `Sync`: open a separate connection per thread.
pub struct Database {
    conn: Connection,
    path: PathBuf,
}

impl Database {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let conn = open_sqlite(&path)?;
        conn.execute_batch(SCHEMA)?;
        for (table, column, kind) in ADDED_COLUMNS {
            let mut info = conn.prepare(&format!("PRAGMA table_info({table})"))?;
            let columns = info
                .query_map([], |row| row.get::<_, String>(1))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            if !columns.iter().any(|name| name == column) {
                conn.execute(
                    &format!("ALTER TABLE {table} ADD COLUMN {column} {kind}"),
                    [],
                )?;
            }
        }
        // Finds a downloaded game by its URL, so downloading again skips it.
        conn.execute(
            "CREATE INDEX IF NOT EXISTS games_site ON games (site) WHERE site LIKE 'http%'",
            [],
        )?;
        Ok(Self { conn, path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Display name, falling back to the file stem.
    pub fn name(&self) -> Result<String> {
        Ok(meta(&self.conn, "name")?.unwrap_or_else(|| {
            self.path
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
                .unwrap_or_default()
        }))
    }

    pub fn set_name(&self, name: &str) -> Result<()> {
        set_meta(&self.conn, "name", name)
    }

    /// The PGN file this database was last imported from, if any, for re-importing it.
    pub fn source_file(&self) -> Result<Option<PathBuf>> {
        Ok(meta(&self.conn, "source_file")?.map(PathBuf::from))
    }

    /// Remembers the PGN file this database was imported from.
    pub fn set_source_file(&self, file: &Path) -> Result<()> {
        set_meta(&self.conn, "source_file", &file.to_string_lossy())
    }

    /// Removes every game, its positions and reviews, before importing the file again.
    pub fn clear_games(&mut self) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute_batch(
            "DELETE FROM structure_positions; DELETE FROM positions; DELETE FROM review_positions;
             DELETE FROM review_summaries; DELETE FROM games;",
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn game_count(&self) -> Result<u64> {
        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM games", [], |row| row.get(0))?;
        Ok(count as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opening_an_older_file_adds_the_newer_columns() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("old.sqlite");
        Connection::open(&path)
            .unwrap()
            .execute_batch(
                "CREATE TABLE games (id INTEGER PRIMARY KEY, white TEXT NOT NULL,
                    black TEXT NOT NULL, white_elo INTEGER, black_elo INTEGER,
                    result TEXT NOT NULL, date TEXT NOT NULL, eco TEXT NOT NULL,
                    event TEXT NOT NULL, round TEXT NOT NULL, source TEXT NOT NULL,
                    time_class TEXT NOT NULL, plies INTEGER NOT NULL, accuracy REAL,
                    left_rep TEXT, moves TEXT NOT NULL);
                 CREATE TABLE review_positions (game_id INTEGER NOT NULL,
                    job_key TEXT NOT NULL, ply INTEGER NOT NULL, fen TEXT NOT NULL,
                    eval_cp INTEGER NOT NULL, depth INTEGER NOT NULL, pv TEXT NOT NULL,
                    PRIMARY KEY (game_id, job_key, ply)) WITHOUT ROWID;",
            )
            .unwrap();
        let db = Database::open(&path).unwrap();
        for (table, column, _) in ADDED_COLUMNS {
            let columns: Vec<String> = db
                .conn
                .prepare(&format!("PRAGMA table_info({table})"))
                .unwrap()
                .query_map([], |row| row.get(1))
                .unwrap()
                .collect::<rusqlite::Result<_>>()
                .unwrap();
            assert!(
                columns.iter().any(|name| name == column),
                "{table}.{column}"
            );
        }
        // Opening again finds every column and adds nothing twice.
        drop(db);
        Database::open(&path).unwrap();
    }
}
