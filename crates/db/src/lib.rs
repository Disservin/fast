//! SQLite game databases: one file per database, position search by Zobrist hash.

mod import;
mod notes;
mod queries;

pub use import::{ImportControl, ImportProgress, ImportSummary, import_games};
pub use notes::NoteStore;
pub use queries::{Filter, FilterStats, GameDetails, GameRow, MoveStat, SideFilter, TimeClass};

use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension, params};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DbError {
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, DbError>;

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
"#;

/// One open database file. Not `Sync`: open a separate connection per thread.
pub struct Database {
    conn: Connection,
    path: PathBuf,
}

impl Database {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(&path)?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn, path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Display name, falling back to the file stem.
    pub fn name(&self) -> Result<String> {
        let stored: Option<String> = self
            .conn
            .query_row("SELECT value FROM meta WHERE key = 'name'", [], |row| row.get(0))
            .optional()?;
        Ok(stored.unwrap_or_else(|| {
            self.path
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
                .unwrap_or_default()
        }))
    }

    pub fn set_name(&self, name: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO meta (key, value) VALUES ('name', ?1)",
            params![name],
        )?;
        Ok(())
    }

    pub fn game_count(&self) -> Result<u64> {
        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM games", [], |row| row.get(0))?;
        Ok(count as u64)
    }
}
