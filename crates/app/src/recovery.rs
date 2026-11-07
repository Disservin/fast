//! Versioned recovery envelope. Kept until the user explicitly restores or discards it.
use serde::{Deserialize, Serialize};
use std::path::Path;
#[derive(Clone, Serialize, Deserialize)]
pub struct Recovery {
    pub saved_at: u64,
    pub pgn: String,
}
impl Recovery {
    pub fn discard(path: &Path) -> anyhow::Result<()> {
        for candidate in [path.with_extension("previous"), path.to_path_buf()] {
            match std::fs::remove_file(candidate) {
                Ok(()) => {}
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                Err(err) => return Err(err.into()),
            }
        }
        Ok(())
    }

    pub fn load(path: &Path) -> Option<Self> {
        [path.to_path_buf(), path.with_extension("previous")]
            .into_iter()
            .filter_map(|path| std::fs::read(path).ok())
            .find_map(|bytes| serde_json::from_slice(&bytes).ok())
    }
    pub fn save(path: &Path, pgn: String) -> anyhow::Result<()> {
        let saved_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs();
        crate::persistence::atomic_write(path, &serde_json::to_vec(&Self { saved_at, pgn })?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_discard_also_removes_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("recovery.json");
        Recovery::save(&path, "1. e4 *".into()).unwrap();
        Recovery::save(&path, "1. d4 *".into()).unwrap();
        Recovery::discard(&path).unwrap();
        assert!(Recovery::load(&path).is_none());
        Recovery::discard(&path).unwrap();
    }

    #[test]
    fn snapshots_preserve_variations_and_fall_back_after_corruption() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("game-recovery.json");
        let pgn = "1. e4 {note} e5 (1... c5) *";
        Recovery::save(&path, pgn.into()).unwrap();
        Recovery::save(&path, "1. d4 *".into()).unwrap();
        std::fs::write(&path, "truncated").unwrap();
        let recovery = Recovery::load(&path).unwrap();
        assert_eq!(recovery.pgn, pgn);
        assert!(recovery.saved_at > 0);
        assert!(chess_core::parse_pgn(&recovery.pgn).is_ok());
    }
}
