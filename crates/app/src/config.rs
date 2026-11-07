//! Persisted user configuration (TOML in the platform config directory).

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use anyhow::{Context as _, Result};
use chess_core::Notation;
use engine::EngineConfig;
use serde::{Deserialize, Serialize};
use ui::{BoardPrefs, BoardTheme, CoordinateStyle, PieceSet};

/// Where Fast keeps its files. Demo mode uses a separate tree.
#[derive(Clone, Debug)]
pub struct Paths {
    pub config_file: PathBuf,
    pub data_dir: PathBuf,
}

impl Paths {
    pub fn resolve(demo: bool) -> Self {
        let base_config = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
        let base_data = dirs::data_dir().unwrap_or_else(|| PathBuf::from("."));
        let suffix = if demo { "fast-demo" } else { "fast" };
        if !demo {
            // Profiles from when the app was called Outpost move over on first launch.
            migrate_dir(&base_config.join("outpost"), &base_config.join(suffix));
            migrate_dir(&base_data.join("outpost"), &base_data.join(suffix));
        }
        Self {
            config_file: base_config.join(suffix).join("config.toml"),
            data_dir: base_data.join(suffix),
        }
    }

    pub fn databases_dir(&self) -> PathBuf {
        self.data_dir.join("databases")
    }

    pub fn repertoire_file(&self) -> PathBuf {
        self.data_dir.join("repertoire.json")
    }

    pub fn notes_file(&self) -> PathBuf {
        self.data_dir.join("notes.sqlite")
    }
}

/// Moves a directory to its new name unless the new one already exists.
fn migrate_dir(old: &Path, new: &Path) {
    if old.is_dir()
        && !new.exists()
        && let Err(err) = std::fs::rename(old, new)
    {
        log::warn!(
            "could not move {} to {}: {err}",
            old.display(),
            new.display()
        );
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SavedTheme {
    Stone,
    Walnut,
    Moss,
    Graphite,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SavedPieces {
    Classic,
    Letters,
    Flat,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SavedCoordinates {
    OnSquares,
    Outside,
    Off,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct BoardConfig {
    pub theme: SavedTheme,
    pub pieces: SavedPieces,
    pub coordinates: SavedCoordinates,
    pub animation_ms: u32,
    pub notation: Notation,
    pub show_legal: bool,
    pub highlight_last: bool,
    pub auto_queen: bool,
    pub move_sounds: bool,
    /// Analyse shows Black at the bottom. Kept across games and restarts.
    pub flipped: bool,
}

impl Default for BoardConfig {
    fn default() -> Self {
        Self {
            theme: SavedTheme::Stone,
            pieces: SavedPieces::Classic,
            coordinates: SavedCoordinates::OnSquares,
            animation_ms: 150,
            notation: Notation::San,
            show_legal: true,
            highlight_last: true,
            auto_queen: false,
            move_sounds: true,
            flipped: false,
        }
    }
}

impl BoardConfig {
    pub fn prefs(&self) -> BoardPrefs {
        BoardPrefs {
            theme: match self.theme {
                SavedTheme::Stone => BoardTheme::Stone,
                SavedTheme::Walnut => BoardTheme::Walnut,
                SavedTheme::Moss => BoardTheme::Moss,
                SavedTheme::Graphite => BoardTheme::Graphite,
            },
            pieces: match self.pieces {
                SavedPieces::Classic => PieceSet::Classic,
                SavedPieces::Letters => PieceSet::Letters,
                SavedPieces::Flat => PieceSet::Flat,
            },
            coordinates: match self.coordinates {
                SavedCoordinates::OnSquares => CoordinateStyle::OnSquares,
                SavedCoordinates::Outside => CoordinateStyle::Outside,
                SavedCoordinates::Off => CoordinateStyle::Off,
            },
            animation_ms: self.animation_ms,
            show_legal: self.show_legal,
            highlight_last: self.highlight_last,
            auto_queen: self.auto_queen,
        }
    }
}

/// Default keystrokes per action name. Users can rebind them in Settings.
pub fn default_shortcuts() -> BTreeMap<String, String> {
    [
        ("prev_move", "left"),
        ("next_move", "right"),
        ("first_move", "home"),
        ("last_move", "end"),
        ("toggle_engine", "space"),
        ("flip_board", "f"),
        (
            "search",
            if cfg!(target_os = "macos") {
                "cmd-k"
            } else {
                "ctrl-k"
            },
        ),
        (
            "save_game",
            if cfg!(target_os = "macos") {
                "cmd-s"
            } else {
                "ctrl-s"
            },
        ),
    ]
    .into_iter()
    .map(|(action, key)| (action.to_owned(), key.to_owned()))
    .collect()
}

/// When live analysis stops on its own.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnalysisLimit {
    #[default]
    Infinite,
    Depth(u32),
    Seconds(u32),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// The user's player name in game headers, for "My games" and leak detection.
    pub player_name: String,
    pub player_rating: Option<u32>,
    pub language: String,
    pub board: BoardConfig,
    pub shortcuts: BTreeMap<String, String>,
    pub engines: Vec<EngineConfig>,
    pub next_engine_id: u64,
    pub engine_on: bool,
    pub analysis_limit: AnalysisLimit,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            player_name: "me".to_owned(),
            player_rating: None,
            language: "English".to_owned(),
            board: BoardConfig::default(),
            shortcuts: default_shortcuts(),
            engines: Vec::new(),
            next_engine_id: 1,
            engine_on: true,
            analysis_limit: AnalysisLimit::default(),
        }
    }
}

impl Config {
    /// Loads the config, keeping defaults if the file is missing or damaged.
    pub fn load(path: &Path) -> Self {
        let Ok(text) = std::fs::read_to_string(path) else {
            return Self::default();
        };
        match toml::from_str::<Self>(&text) {
            Ok(mut config) => {
                // New shortcuts added in later versions get their defaults.
                for (action, key) in default_shortcuts() {
                    config.shortcuts.entry(action).or_insert(key);
                }
                config
            }
            Err(err) => {
                log::warn!("ignoring unreadable config {}: {err}", path.display());
                Self::default()
            }
        }
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).context("creating config directory")?;
        }
        let text = toml::to_string_pretty(self).context("serialising config")?;
        std::fs::write(path, text).context("writing config")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine::EngineConfig;

    #[test]
    fn config_round_trips_through_toml() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/config.toml");
        let mut config = Config {
            player_name: "someone".to_owned(),
            ..Default::default()
        };
        config.board.theme = SavedTheme::Moss;
        config.board.notation = Notation::Figurine;
        config.analysis_limit = AnalysisLimit::Depth(30);
        config.engines.push(EngineConfig {
            id: 3,
            name: "Stockfish".to_owned(),
            path: "/usr/bin/stockfish".into(),
            options: [("Hash".to_owned(), "512".to_owned())]
                .into_iter()
                .collect(),
            ..Default::default()
        });
        config.save(&path).unwrap();
        assert_eq!(Config::load(&path), config);
    }

    #[test]
    fn old_profile_moves_only_when_the_new_one_is_absent() {
        let dir = tempfile::tempdir().unwrap();
        let (old, new) = (dir.path().join("outpost"), dir.path().join("fast"));
        std::fs::create_dir(&old).unwrap();
        std::fs::write(old.join("config.toml"), "x").unwrap();
        migrate_dir(&old, &new);
        assert!(new.join("config.toml").exists());
        assert!(!old.exists());
        // An existing new profile is never overwritten.
        std::fs::create_dir(&old).unwrap();
        migrate_dir(&old, &new);
        assert!(old.exists() && new.exists());
    }

    #[test]
    fn damaged_or_missing_files_fall_back_to_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        assert_eq!(Config::load(&path), Config::default());
        std::fs::write(&path, "this is = = not toml").unwrap();
        assert_eq!(Config::load(&path), Config::default());
    }

    #[test]
    fn partial_files_fill_in_missing_fields_and_shortcuts() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            "player_name = \"x\"\n[shortcuts]\nflip_board = \"g\"\n",
        )
        .unwrap();
        let config = Config::load(&path);
        assert_eq!(config.player_name, "x");
        assert_eq!(config.shortcuts["flip_board"], "g");
        assert_eq!(config.shortcuts["prev_move"], "left");
    }
}
