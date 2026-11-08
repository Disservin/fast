//! SQLite game databases: one file per database, position search by Zobrist hash.

mod edit;
mod import;
mod notes;
mod queries;
mod structure;

pub use edit::GameInfo;
pub use import::{ImportControl, ImportOptions, ImportProgress, ImportSummary, import_games};
pub use notes::NoteStore;
pub use queries::{
    Filter, FilterStats, GameDetails, GameRow, MoveStat, PlayerLine, ResultFilter, ReviewPosition,
    ReviewSummary, SideFilter, Sort, SortKey, TimeClass, date_bound,
};

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
        let has_chess960 = {
            let mut stmt = conn.prepare("PRAGMA table_info(games)")?;
            stmt.query_map([], |row| row.get::<_, String>(1))?
                .collect::<rusqlite::Result<Vec<_>>>()?
                .iter()
                .any(|column| column == "chess960")
        };
        if !has_chess960 {
            conn.execute(
                "ALTER TABLE games ADD COLUMN chess960 INTEGER NOT NULL DEFAULT 0",
                [],
            )?;
        }
        let has_start_fen = {
            let mut columns = conn.prepare("PRAGMA table_info(games)")?;
            columns
                .query_map([], |row| row.get::<_, String>(1))?
                .collect::<rusqlite::Result<Vec<_>>>()?
                .iter()
                .any(|column| column == "start_fen")
        };
        if !has_start_fen {
            conn.execute(
                "ALTER TABLE games ADD COLUMN start_fen TEXT NOT NULL DEFAULT ''",
                [],
            )?;
        }
        let has_annotated = {
            let mut columns = conn.prepare("PRAGMA table_info(games)")?;
            columns
                .query_map([], |row| row.get::<_, String>(1))?
                .collect::<rusqlite::Result<Vec<_>>>()?
                .iter()
                .any(|column| column == "annotated")
        };
        if !has_annotated {
            conn.execute("ALTER TABLE games ADD COLUMN annotated TEXT", [])?;
        }
        let has_site = {
            let mut columns = conn.prepare("PRAGMA table_info(games)")?;
            columns
                .query_map([], |row| row.get::<_, String>(1))?
                .collect::<rusqlite::Result<Vec<_>>>()?
                .iter()
                .any(|column| column == "site")
        };
        if !has_site {
            conn.execute(
                "ALTER TABLE games ADD COLUMN site TEXT NOT NULL DEFAULT ''",
                [],
            )?;
        }
        // Finds a downloaded game by its URL, so downloading again skips it.
        conn.execute(
            "CREATE INDEX IF NOT EXISTS games_site ON games (site) WHERE site LIKE 'http%'",
            [],
        )?;
        let has_mate_distance = {
            let mut columns = conn.prepare("PRAGMA table_info(review_positions)")?;
            columns
                .query_map([], |row| row.get::<_, String>(1))?
                .collect::<rusqlite::Result<Vec<_>>>()?
                .iter()
                .any(|column| column == "mate_distance")
        };
        if !has_mate_distance {
            conn.execute(
                "ALTER TABLE review_positions ADD COLUMN mate_distance INTEGER",
                [],
            )?;
        }
        Ok(Self { conn, path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Display name, falling back to the file stem.
    pub fn name(&self) -> Result<String> {
        let stored: Option<String> = self
            .conn
            .query_row("SELECT value FROM meta WHERE key = 'name'", [], |row| {
                row.get(0)
            })
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

    /// The PGN file this database was last imported from, if any, for re-importing it.
    pub fn source_file(&self) -> Result<Option<PathBuf>> {
        let stored: Option<String> = self
            .conn
            .query_row(
                "SELECT value FROM meta WHERE key = 'source_file'",
                [],
                |row| row.get(0),
            )
            .optional()?;
        Ok(stored.map(PathBuf::from))
    }

    /// Remembers the PGN file this database was imported from.
    pub fn set_source_file(&self, file: &Path) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO meta (key, value) VALUES ('source_file', ?1)",
            params![file.to_string_lossy().into_owned()],
        )?;
        Ok(())
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
