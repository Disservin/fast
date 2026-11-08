//! Registry of configured engines and their running processes.

use std::{collections::BTreeMap, path::PathBuf};

use serde::{Deserialize, Serialize};

use crate::{EngineHandle, EngineId, EngineInfo};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum EngineStatus {
    #[default]
    Ready,
    Loading,
    Searching,
    Paused,
    Stopped,
    Disabled,
    Error,
}

/// A named set of option values, such as "BT2" or "Maia 1900" for one Lc0 binary.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineProfile {
    pub name: String,
    pub options: BTreeMap<String, String>,
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
    /// A custom picture for the engine; a rook is drawn when absent.
    #[serde(default)]
    pub icon: Option<PathBuf>,
    /// Saved option sets, applied with [`EngineConfig::apply_profile`].
    #[serde(default)]
    pub profiles: Vec<EngineProfile>,
    /// The profile last applied, while the options still match it.
    #[serde(default)]
    pub active_profile: Option<String>,
    /// Sends `debug on` and keeps the raw output on screen.
    #[serde(default)]
    pub debug: bool,
    /// Custom analysis arrow colour (`0xRRGGBB`), overriding quality colours.
    #[serde(default)]
    pub arrow_color: Option<u32>,
}

impl EngineConfig {
    /// An engine is enabled while any feature uses it.
    pub fn sync_enabled(&mut self) {
        self.enabled = self.use_analysis || self.use_review || self.use_play;
    }

    /// The status of an engine that is not running: ready, or disabled.
    pub fn idle_status(&self) -> EngineStatus {
        if self.enabled {
            EngineStatus::Ready
        } else {
            EngineStatus::Disabled
        }
    }

    /// Saves the current options under `name`, replacing a profile of that name.
    pub fn save_profile(&mut self, name: &str) {
        let name = name.trim();
        if name.is_empty() {
            return;
        }
        let profile = EngineProfile {
            name: name.to_owned(),
            options: self.options.clone(),
        };
        match self.profiles.iter_mut().find(|p| p.name == name) {
            Some(existing) => *existing = profile,
            None => self.profiles.push(profile),
        }
        self.active_profile = Some(name.to_owned());
    }

    /// Replaces the options with a profile's, returning the options whose values
    /// changed (a `None` value means "back to the engine default").
    pub fn apply_profile(&mut self, name: &str) -> Vec<(String, Option<String>)> {
        let Some(profile) = self.profiles.iter().find(|p| p.name == name).cloned() else {
            return Vec::new();
        };
        let mut changed = Vec::new();
        for (option, value) in &profile.options {
            if self.options.get(option) != Some(value) {
                changed.push((option.clone(), Some(value.clone())));
            }
        }
        for option in self.options.keys() {
            if !profile.options.contains_key(option) {
                changed.push((option.clone(), self.default_value(option)));
            }
        }
        self.options = profile.options;
        self.active_profile = Some(profile.name);
        changed
    }

    pub fn delete_profile(&mut self, name: &str) {
        self.profiles.retain(|p| p.name != name);
        if self.active_profile.as_deref() == Some(name) {
            self.active_profile = None;
        }
    }

    /// The profile whose options equal the current ones, if any.
    pub fn matching_profile(&self) -> Option<&str> {
        self.active_profile
            .as_deref()
            .filter(|name| {
                self.profiles
                    .iter()
                    .any(|p| p.name == *name && p.options == self.options)
            })
            .or_else(|| {
                self.profiles
                    .iter()
                    .find(|p| p.options == self.options)
                    .map(|p| p.name.as_str())
            })
    }

    /// The value an option currently has: the saved one, else the engine's default.
    pub fn option_value(&self, name: &str) -> Option<String> {
        self.options
            .get(name)
            .cloned()
            .or_else(|| self.default_value(name))
    }

    /// The engine's own default for an option, as UCI text.
    pub fn default_value(&self, name: &str) -> Option<String> {
        let option = self.info.options.iter().find(|o| o.name == name)?;
        match &option.kind {
            crate::OptionKind::Check { default } => Some(default.to_string()),
            crate::OptionKind::Spin { default, .. } => Some(default.to_string()),
            crate::OptionKind::Combo { default, .. } | crate::OptionKind::String { default } => {
                Some(default.clone())
            }
            crate::OptionKind::Button => None,
        }
    }

    /// A second instance of the same engine, e.g. one Lc0 with another network.
    pub fn duplicate(&self, id: EngineId, existing: &[EngineConfig]) -> Self {
        let mut copy = self.clone();
        copy.id = id;
        let base = self.name.clone();
        let mut index = 2;
        loop {
            let name = format!("{base} ({index})");
            if !existing.iter().any(|engine| engine.name == name) {
                copy.name = name;
                break;
            }
            index += 1;
        }
        copy
    }

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
                let handle = EngineHandle::spawn(
                    config.path.clone(),
                    Vec::new(),
                    config
                        .options
                        .iter()
                        .map(|(name, value)| (name.clone(), value.clone()))
                        .collect(),
                );
                if config.debug {
                    handle.set_debug(true);
                }
                handle
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

    fn lc0() -> EngineConfig {
        EngineConfig {
            id: 1,
            name: "Lc0".to_owned(),
            info: EngineInfo {
                options: vec![crate::UciOption {
                    name: "Threads".to_owned(),
                    kind: crate::OptionKind::Spin {
                        default: 2,
                        min: 1,
                        max: 64,
                    },
                }],
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn profiles_save_apply_and_report_changes() {
        let mut config = lc0();
        config
            .options
            .insert("WeightsFile".to_owned(), "bt2.pb.gz".to_owned());
        config.save_profile("BT2");
        config.options.clear();
        config
            .options
            .insert("WeightsFile".to_owned(), "maia-1900.pb.gz".to_owned());
        config.options.insert("Threads".to_owned(), "1".to_owned());
        config.save_profile("Maia");
        assert_eq!(config.matching_profile(), Some("Maia"));
        let changed = config.apply_profile("BT2");
        assert!(changed.contains(&("WeightsFile".to_owned(), Some("bt2.pb.gz".to_owned()))));
        // Threads is not in BT2, so it goes back to the engine default.
        assert!(changed.contains(&("Threads".to_owned(), Some("2".to_owned()))));
        assert_eq!(config.matching_profile(), Some("BT2"));
        config.options.insert("Threads".to_owned(), "8".to_owned());
        assert_eq!(config.matching_profile(), None);
        config.delete_profile("BT2");
        assert_eq!(config.profiles.len(), 1);
        assert_eq!(config.active_profile, None);
        assert!(config.apply_profile("missing").is_empty());
    }

    #[test]
    fn duplicates_get_a_free_name_and_their_own_id() {
        let first = lc0();
        let second = first.duplicate(2, std::slice::from_ref(&first));
        assert_eq!(second.name, "Lc0 (2)");
        assert_eq!(second.id, 2);
        let third = first.duplicate(3, &[first.clone(), second]);
        assert_eq!(third.name, "Lc0 (3)");
    }
}
