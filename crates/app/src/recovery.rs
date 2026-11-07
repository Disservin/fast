//! Versioned recovery envelope. Kept until the user explicitly restores or discards it.
use serde::{Deserialize, Serialize};
use std::path::Path;
#[derive(Clone, Serialize, Deserialize)]
pub struct Recovery {
    pub saved_at: u64,
    pub pgn: String,
}
impl Recovery {
    pub fn load(path: &Path) -> Option<Self> {
        serde_json::from_slice(&std::fs::read(path).ok()?).ok()
    }
    pub fn save(path: &Path, pgn: String) -> anyhow::Result<()> {
        let saved_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs();
        crate::persistence::atomic_write(path, &serde_json::to_vec(&Self { saved_at, pgn })?)
    }
}
