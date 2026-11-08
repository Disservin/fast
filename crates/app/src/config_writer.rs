//! Serializes background settings writes so an older snapshot cannot replace a newer one.

use crate::config::Config;
use anyhow::Result;
use std::{
    path::Path,
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

/// Coordinates settings snapshots without making the UI wait for disk writes.
#[derive(Default)]
pub struct ConfigWriter {
    revision: AtomicU64,
    disk: Mutex<()>,
}

impl ConfigWriter {
    /// Marks settings as changed and invalidates any snapshot still waiting to save.
    pub fn changed(&self) -> u64 {
        self.revision.fetch_add(1, Ordering::SeqCst) + 1
    }

    /// Whether a completed write still represents the settings shown in the UI.
    pub fn is_current(&self, revision: u64) -> bool {
        self.revision.load(Ordering::SeqCst) == revision
    }

    /// Keeps a profile restore from racing with an in-flight settings replacement.
    pub fn exclusive<T>(&self, operation: impl FnOnce() -> Result<T>) -> Result<T> {
        let _guard = self
            .disk
            .lock()
            .map_err(|_| anyhow::anyhow!("Settings writer lock poisoned"))?;
        operation()
    }

    /// Saves a current snapshot on a worker, or skips it if newer settings are queued.
    pub fn save(&self, revision: u64, config: &Config, path: &Path) -> Result<bool> {
        let _guard = self
            .disk
            .lock()
            .map_err(|_| anyhow::anyhow!("Settings writer lock poisoned"))?;
        if !self.is_current(revision) {
            return Ok(false);
        }
        config.save(path)?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A delayed older save cannot overwrite the current settings or their backup.
    #[test]
    fn delayed_snapshots_cannot_replace_newer_settings() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let writer = ConfigWriter::default();
        let first = Config {
            player_name: "First".into(),
            ..Default::default()
        };
        let second = Config {
            player_name: "Second".into(),
            ..Default::default()
        };
        let initial = writer.changed();
        assert!(writer.save(initial, &first, &path).unwrap());
        let delayed = writer.changed();
        let current = writer.changed();
        assert!(writer.save(current, &second, &path).unwrap());
        assert!(!writer.save(delayed, &first, &path).unwrap());
        assert_eq!(Config::load(&path), second);
        assert_eq!(Config::load(&path.with_extension("previous")), first);
        assert!(!writer.is_current(initial));
    }

    /// Restoring a profile invalidates snapshots that have not reached the disk yet.
    #[test]
    fn restoring_settings_cannot_be_undone_by_a_pending_save() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let writer = ConfigWriter::default();
        let pending = writer.changed();
        let restored = Config {
            player_name: "Restored".into(),
            ..Default::default()
        };
        writer.changed();
        writer.exclusive(|| restored.save(&path)).unwrap();
        assert!(!writer.save(pending, &Config::default(), &path).unwrap());
        assert_eq!(Config::load(&path), restored);
    }

    /// Failed writes keep the last readable settings available for retry.
    #[test]
    fn failed_snapshots_preserve_the_saved_settings() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let writer = ConfigWriter::default();
        let config = Config::default();
        let revision = writer.changed();
        assert!(writer.save(revision, &config, &path).unwrap());
        let revision = writer.changed();
        assert!(
            writer
                .save(revision, &config, &path.join("invalid"))
                .is_err()
        );
        assert!(writer.is_current(revision));
        assert_eq!(Config::load(&path), config);
    }
}
