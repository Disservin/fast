//! Durable engine analysis records, scoped to an exact position history and search setup.

use std::path::Path;

use anyhow::Result;
use engine::EngineId;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::live::LiveLine;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AnalysisRecord {
    pub position: String,
    pub engine: String,
    pub limit: String,
    pub lines: std::collections::BTreeMap<u32, LiveLine>,
    pub saved_at: i64,
}

pub struct AnalysisStore(Connection);

impl AnalysisStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        if let Some(parent) = path.as_ref().parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;
             CREATE TABLE IF NOT EXISTS analyses (
               cache_key TEXT PRIMARY KEY,
               engine_id INTEGER NOT NULL,
               position TEXT NOT NULL,
               engine TEXT NOT NULL,
               search_limit TEXT NOT NULL,
               record TEXT NOT NULL,
               saved_at INTEGER NOT NULL
             );
             CREATE INDEX IF NOT EXISTS analyses_position ON analyses(position, engine_id);",
        )?;
        Ok(Self(conn))
    }

    pub fn get(&self, key: &str) -> Result<Option<AnalysisRecord>> {
        let value: Option<String> = self
            .0
            .query_row(
                "SELECT record FROM analyses WHERE cache_key=?1",
                [key],
                |row| row.get(0),
            )
            .optional()?;
        value
            .map(|value| Ok(serde_json::from_str(&value)?))
            .transpose()
    }

    pub fn latest_for_position(
        &self,
        engine_id: EngineId,
        position: &str,
    ) -> Result<Option<AnalysisRecord>> {
        let value: Option<String> = self
            .0
            .query_row(
                "SELECT record FROM analyses WHERE engine_id=?1 AND position=?2 ORDER BY saved_at DESC LIMIT 1",
                params![engine_id as i64, position],
                |row| row.get(0),
            )
            .optional()?;
        value
            .map(|value| Ok(serde_json::from_str(&value)?))
            .transpose()
    }

    pub fn put(&self, key: &str, engine_id: EngineId, record: &AnalysisRecord) -> Result<()> {
        let json = serde_json::to_string(record)?;
        self.0.execute(
            "INSERT INTO analyses(cache_key,engine_id,position,engine,search_limit,record,saved_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7)
             ON CONFLICT(cache_key) DO UPDATE SET record=excluded.record,saved_at=excluded.saved_at",
            params![key, engine_id as i64, record.position, record.engine, record.limit, json, record.saved_at],
        )?;
        Ok(())
    }

    pub fn delete(&self, key: &str) -> Result<()> {
        self.0
            .execute("DELETE FROM analyses WHERE cache_key=?1", [key])?;
        Ok(())
    }
}

pub fn identity(position: &str, engine: &engine::EngineConfig, limit: &str) -> String {
    use sha2::{Digest, Sha256};
    let engine_fingerprint = engine_signature(engine);
    let bytes = serde_json::to_vec(&(position, &engine_fingerprint, limit)).unwrap_or_default();
    let digest = Sha256::digest(bytes);
    let mut key = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write;
        let _ = write!(key, "{byte:02x}");
    }
    key
}

pub fn engine_signature(engine: &engine::EngineConfig) -> String {
    let executable = std::fs::metadata(&engine.path).ok().map(|metadata| {
        (
            metadata.len(),
            metadata
                .modified()
                .ok()
                .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|time| time.as_secs()),
        )
    });
    serde_json::to_string(&(
        engine.id,
        &engine.name,
        &engine.version,
        engine.path.to_string_lossy(),
        &engine.options,
        &engine.info.name,
        executable,
    ))
    .unwrap_or_default()
}

pub fn position_key(root_fen: &str, moves: &[String]) -> String {
    format!("{root_fen}\n{}", moves.join(" "))
}

pub fn tree_position_key(tree: &chess_core::GameTree, node: chess_core::NodeId) -> String {
    let moves: Vec<String> = tree
        .path_to(node)
        .iter()
        .filter_map(|&id| tree.node(id).mv)
        .map(|mv| chess_core::shakmaty::uci::UciMove::from_standard(mv).to_string())
        .collect();
    position_key(&tree.fen(chess_core::GameTree::ROOT), &moves)
}

pub fn current_limit(limit: crate::config::AnalysisLimit) -> String {
    match limit {
        crate::config::AnalysisLimit::Infinite => "infinite".to_owned(),
        crate::config::AnalysisLimit::Depth(depth) => format!("depth:{depth}"),
        crate::config::AnalysisLimit::Seconds(seconds) => format!("seconds:{seconds}"),
    }
}

pub fn timestamp() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}
