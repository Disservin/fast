//! Registry of configured engines and their running processes.

use std::{collections::BTreeMap, path::PathBuf};

use serde::{Deserialize, Serialize};

use crate::{EngineHandle, EngineId, EngineInfo};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum EngineStatus {
    #[default]
    Ready,
    Disabled,
    Error,
}

/// What the app persists per engine. `options` are re-applied on every launch.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineConfig {
    pub id: EngineId,
    pub name: String,
    #[serde(default)]
    pub version: String,
    pub path: PathBuf,
    #[serde(default)]
    pub runs_on_gpu: bool,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub options: BTreeMap<String, String>,
    #[serde(default)]
    pub use_analysis: bool,
    #[serde(default)]
    pub use_review: bool,
    #[serde(default)]
    pub use_play: bool,
    /// Schema from the last successful handshake, so Settings renders offline.
    #[serde(default)]
    pub info: EngineInfo,
}

impl EngineConfig {
    /// "Analysis, review" text for the Used-for column.
    pub fn used_for(&self) -> String {
        let mut uses = Vec::new();
        if self.use_analysis {
            uses.push("Analysis");
        }
        if self.use_review {
            uses.push(if uses.is_empty() { "Review" } else { "review" });
        }
        if self.use_play {
            uses.push(if uses.is_empty() { "Play" } else { "play" });
        }
        if uses.is_empty() {
            "—".to_owned()
        } else {
            uses.join(", ")
        }
    }
}

/// Owns the live processes. Dropping the manager shuts every engine down.
#[derive(Default)]
pub struct EngineManager {
    running: BTreeMap<EngineId, EngineHandle>,
}

impl EngineManager {
    /// The running handle for this engine, starting it if necessary.
    pub fn ensure_running(&mut self, config: &EngineConfig) -> EngineHandle {
        self.running
            .entry(config.id)
            .or_insert_with(|| {
                EngineHandle::spawn(
                    config.path.clone(),
                    Vec::new(),
                    config
                        .options
                        .iter()
                        .map(|(name, value)| (name.clone(), value.clone()))
                        .collect(),
                )
            })
            .clone()
    }

    pub fn get(&self, id: EngineId) -> Option<&EngineHandle> {
        self.running.get(&id)
    }

    /// Stops and forgets a process, e.g. after a crash, so it can be restarted.
    pub fn shutdown(&mut self, id: EngineId) {
        if let Some(handle) = self.running.remove(&id) {
            handle.quit();
        }
    }

    pub fn shutdown_all(&mut self) {
        for (_, handle) in std::mem::take(&mut self.running) {
            handle.quit();
        }
    }
}

impl Drop for EngineManager {
    fn drop(&mut self) {
        self.shutdown_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn used_for_text() {
        let mut config = EngineConfig::default();
        assert_eq!(config.used_for(), "—");
        config.use_analysis = true;
        config.use_review = true;
        assert_eq!(config.used_for(), "Analysis, review");
        config.use_analysis = false;
        config.use_review = false;
        config.use_play = true;
        assert_eq!(config.used_for(), "Play");
    }
}
