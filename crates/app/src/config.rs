//! Persisted user configuration (TOML in the platform config directory).

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use anyhow::{Context as _, Result};
use chess_core::Notation;
use engine::{EngineConfig, EngineId};
use serde::{Deserialize, Serialize};
use ui::{BoardPrefs, BoardTheme, CoordinateStyle, PieceSet, i18n::Language};

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

    pub fn analysis_file(&self) -> PathBuf {
        self.data_dir.join("analysis.sqlite")
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
    /// Configs from before the Blue board named its predecessor "stone".
    #[serde(alias = "stone")]
    Blue,
    Walnut,
    Moss,
    Graphite,
    Paper,
    Plum,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SavedPieces {
    Classic,
    Cburnett,
    Chessnut,
    #[serde(rename = "mpchess")]
    MpChess,
    Fantasy,
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
    pub show_move_badges: bool,
    pub auto_queen: bool,
    pub move_sounds: bool,
    /// Analyse shows Black at the bottom. Kept across games and restarts.
    pub flipped: bool,
    /// Clicking an engine arrow plays that move.
    pub click_arrows: bool,
    /// Move sound loudness, 0–100.
    pub sound_volume: u32,
    pub sound_profile: crate::sound::SoundProfile,
}

impl Default for BoardConfig {
    fn default() -> Self {
        Self {
            theme: SavedTheme::Graphite,
            pieces: SavedPieces::Classic,
            coordinates: SavedCoordinates::OnSquares,
            animation_ms: 150,
            notation: Notation::San,
            show_legal: true,
            highlight_last: true,
            show_move_badges: true,
            auto_queen: false,
            move_sounds: true,
            flipped: false,
            click_arrows: true,
            sound_volume: 80,
            sound_profile: crate::sound::SoundProfile::default(),
        }
    }
}

impl BoardConfig {
    /// Loudness for move sounds, `0.0` when they are off.
    pub fn sound_level(&self) -> f32 {
        if self.move_sounds {
            self.sound_volume.min(100) as f32 / 100.0
        } else {
            0.0
        }
    }

    /// Plays the sounds for `mv`, which led to `after`, at the saved profile and loudness.
    pub fn play_move_sound(
        &self,
        mv: &chess_core::Move,
        after: &chess_core::Chess,
        end: Option<chess_core::GameEnd>,
    ) {
        crate::sound::play_move(self.sound_profile, self.sound_level(), mv, after, end);
    }

    /// A single move sound, to preview the profile and loudness.
    pub fn play_sample(&self) {
        crate::sound::play(
            self.sound_profile,
            &[crate::sound::Sound::Move],
            self.sound_level(),
        );
    }
}

impl BoardConfig {
    pub fn prefs(&self) -> BoardPrefs {
        BoardPrefs {
            theme: match self.theme {
                SavedTheme::Blue => BoardTheme::Blue,
                SavedTheme::Walnut => BoardTheme::Walnut,
                SavedTheme::Moss => BoardTheme::Moss,
                SavedTheme::Graphite => BoardTheme::Graphite,
                SavedTheme::Paper => BoardTheme::Paper,
                SavedTheme::Plum => BoardTheme::Plum,
            },
            pieces: match self.pieces {
                SavedPieces::Classic => PieceSet::Classic,
                SavedPieces::Cburnett => PieceSet::Cburnett,
                SavedPieces::Chessnut => PieceSet::Chessnut,
                SavedPieces::MpChess => PieceSet::MpChess,
                SavedPieces::Fantasy => PieceSet::Fantasy,
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
            click_arrows: self.click_arrows,
        }
    }
}

/// Ctrl, or Cmd on macOS, plus `key`.
fn primary(key: &str) -> String {
    let modifier = if cfg!(target_os = "macos") {
        "cmd"
    } else {
        "ctrl"
    };
    format!("{modifier}-{key}")
}

/// Default keystrokes per action name. Users can rebind them in Settings; an empty
/// keystroke means the action has no shortcut.
pub fn default_shortcuts() -> BTreeMap<String, String> {
    [
        // Navigation
        ("prev_move", "left".to_owned()),
        ("next_move", "right".to_owned()),
        ("first_move", "home".to_owned()),
        ("last_move", "end".to_owned()),
        ("flip_board", "f".to_owned()),
        // Game
        ("new_game", primary("n")),
        ("open_game", primary("o")),
        ("save_game", primary("s")),
        ("save_pgn_as", primary("shift-s")),
        ("paste_game", primary("shift-v")),
        ("copy_fen", primary("shift-f")),
        ("copy_pgn", primary("shift-p")),
        // Engine
        ("toggle_engine", "space".to_owned()),
        ("pause_engine", "p".to_owned()),
        ("analyse_backward", "b".to_owned()),
        ("more_lines", "]".to_owned()),
        ("fewer_lines", "[".to_owned()),
        ("engine_output", primary("shift-l")),
        // Analysis display
        ("all_moves", "a".to_owned()),
        ("quality_arrows", "r".to_owned()),
        ("sort_lines", "d".to_owned()),
        ("next_graph", "g".to_owned()),
        ("scoresheet_style", "i".to_owned()),
        // Window
        ("new_tab", primary("t")),
        ("close_tab", primary("w")),
        ("reopen_tab", primary("shift-t")),
        ("next_tab", primary("tab")),
        ("prev_tab", primary("shift-tab")),
        ("search", primary("k")),
        ("show_analyse", primary("1")),
        ("show_database", primary("2")),
        ("show_train", primary("3")),
        ("show_play", primary("4")),
        ("show_settings", primary(",")),
        ("quit", primary("q")),
    ]
    .into_iter()
    .map(|(action, key)| (action.to_owned(), key))
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
    /// A fixed node count per search, as backwards analysis uses.
    Nodes(u64),
}

/// Nodes searched per position when a game is analysed backwards.
pub const DEFAULT_BACKWARD_NODES: u64 = 500_000;

/// How the Moves table orders engine lines.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LineSort {
    #[default]
    Eval,
    Depth,
}

/// How the scoresheet lays out the game.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScoresheetStyle {
    /// Lichess: a move table, variations as indented blocks.
    #[default]
    Table,
    /// Nibbler: running text, each variation depth in its own colour.
    Inline,
}

/// Which graph sits under the board.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphTab {
    Timeline,
    #[default]
    Eval,
    Time,
    Depth,
}

impl GraphTab {
    pub const ALL: [Self; 4] = [Self::Eval, Self::Time, Self::Depth, Self::Timeline];

    pub fn label(self) -> &'static str {
        match self {
            Self::Timeline => "Bars",
            Self::Eval => "Eval",
            Self::Time => "Time",
            Self::Depth => "Depth",
        }
    }
}

/// Analyse screen display choices.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AnalysisView {
    pub sort: LineSort,
    /// List every legal move, analysed or not, like Nibbler.
    pub all_moves: bool,
    /// One arrow per engine line, coloured by how much it loses against the best.
    pub quality_arrows: bool,
    pub scoresheet: ScoresheetStyle,
    pub graph: GraphTab,
}

impl Default for AnalysisView {
    fn default() -> Self {
        Self {
            sort: LineSort::Eval,
            all_moves: false,
            quality_arrows: true,
            scoresheet: ScoresheetStyle::Table,
            graph: GraphTab::Eval,
        }
    }
}

/// Window dimensions use logical pixels, so they survive display-scale changes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedWindow {
    pub width: u32,
    pub height: u32,
    pub maximized: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PanelSizes {
    pub scoresheet_width: u32,
    /// None keeps the board fitted to the available space.
    pub board_size: Option<u32>,
}

impl Default for PanelSizes {
    fn default() -> Self {
        Self {
            scoresheet_width: 300,
            board_size: None,
        }
    }
}

/// What a test suite run looks for in each position.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TestGoal {
    /// Play a `bm` move and avoid any `am` move.
    #[default]
    BestMove,
    /// Find the `dm` mate, or a shorter one.
    Mate,
}

/// How long an engine searches each test position.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TestLimitKind {
    Depth,
    Nodes,
    #[default]
    Time,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TestPositionSettings {
    pub goal: TestGoal,
    pub engines: Vec<EngineId>,
    pub limit: TestLimitKind,
    pub depth: u32,
    pub nodes: u64,
    /// Milliseconds per position.
    pub time_ms: u64,
    /// None keeps each engine's own Hash setting.
    pub hash_mb: Option<u32>,
    pub clear_hash: bool,
    pub parallel: bool,
    /// Processes per engine when running in parallel.
    pub processes: u32,
    pub syzygy: bool,
    pub syzygy_path: Option<PathBuf>,
}

impl Default for TestPositionSettings {
    fn default() -> Self {
        Self {
            goal: TestGoal::default(),
            engines: Vec::new(),
            limit: TestLimitKind::default(),
            depth: 20,
            nodes: 1_000_000,
            time_ms: 1000,
            hash_mb: Some(64),
            clear_hash: true,
            parallel: false,
            processes: 1,
            syzygy: false,
            syzygy_path: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// The user's player name in game headers, for "My games" and leak detection.
    pub player_name: String,
    pub player_rating: Option<u32>,
    pub language: Language,
    pub ui_theme: ui::UiTheme,
    pub board: BoardConfig,
    pub shortcuts: BTreeMap<String, String>,
    pub engines: Vec<EngineConfig>,
    pub next_engine_id: u64,
    pub engine_on: bool,
    /// Keep an Analyse tab's engines searching while a tab other than Analyse is shown.
    /// Off: leaving the tab stops them, and they resume on return.
    pub analyse_in_background: bool,
    pub analysis_limit: AnalysisLimit,
    /// Nodes each engine searches per position when a game is analysed backwards,
    /// whatever the search limit for ordinary analysis.
    pub backward_nodes: u64,
    pub view: AnalysisView,
    /// A fastchess executable to use instead of the pinned download.
    pub fastchess_path: Option<PathBuf>,
    pub window: Option<SavedWindow>,
    pub panels: PanelSizes,
    /// The Test positions tab's last settings.
    pub test_positions: TestPositionSettings,
    /// How many engine processes a Database batch review runs at once.
    pub review_engines: u32,
    /// The tabs open when Fast last closed, reopened on the next start.
    pub session: Option<SavedSession>,
}

/// The open tabs, in order, and which one was shown.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SavedSession {
    pub tabs: Vec<SavedTab>,
    /// Index into `tabs` of the tab that was shown.
    pub active: usize,
}

/// One open tab: its screen, the name the user gave it and, for Analyse and Play,
/// the recovery file that holds its game.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedTab {
    pub kind: SavedTabKind,
    pub id: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// File name inside the recovery directory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recovery: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SavedTabKind {
    Analyse,
    Database,
    Train,
    Play,
    Settings,
    Tournament,
    TestPositions,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            player_name: "me".to_owned(),
            player_rating: None,
            language: Language::default(),
            ui_theme: ui::UiTheme::default(),
            board: BoardConfig::default(),
            shortcuts: default_shortcuts(),
            engines: Vec::new(),
            next_engine_id: 1,
            engine_on: true,
            analyse_in_background: false,
            analysis_limit: AnalysisLimit::default(),
            backward_nodes: DEFAULT_BACKWARD_NODES,
            view: AnalysisView::default(),
            fastchess_path: None,
            window: None,
            panels: PanelSizes::default(),
            test_positions: TestPositionSettings::default(),
            review_engines: 1,
            session: None,
        }
    }
}

impl Config {
    /// Apply the interface palette together with its matching board default.
    pub fn set_ui_theme(&mut self, theme: ui::UiTheme) {
        self.ui_theme = theme;
        self.board.theme = match theme {
            ui::UiTheme::Slate => SavedTheme::Blue,
            ui::UiTheme::Obsidian => SavedTheme::Graphite,
            ui::UiTheme::Paper => SavedTheme::Paper,
            ui::UiTheme::Plum => SavedTheme::Plum,
            ui::UiTheme::Sand => SavedTheme::Walnut,
            ui::UiTheme::Forest => SavedTheme::Moss,
        };
    }

    /// Loads the config, keeping defaults if the file is missing or damaged.
    pub fn load(path: &Path) -> Self {
        let text = [path.to_path_buf(), path.with_extension("previous")]
            .into_iter()
            .filter_map(|candidate| std::fs::read_to_string(candidate).ok())
            .find(|text| toml::from_str::<Self>(text).is_ok());
        let Some(text) = text else {
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
        crate::persistence::atomic_write_checked(path, text.as_bytes(), |bytes| {
            std::str::from_utf8(bytes)
                .ok()
                .is_some_and(|text| toml::from_str::<Self>(text).is_ok())
        })
        .context("writing config")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine::EngineConfig;

    #[test]
    fn language_uses_only_locale_codes() {
        for (value, expected) in [("en", Language::English), ("de", Language::German)] {
            let config: Config = toml::from_str(&format!("language = {value:?}")).unwrap();
            assert_eq!(config.language, expected);
            let saved = toml::to_string(&config).unwrap();
            assert!(saved.contains(&format!("language = {:?}", expected.code())));
        }
        for value in ["English", "German", "Deutsch", "de-DE", "unknown"] {
            assert!(toml::from_str::<Config>(&format!("language = {value:?}")).is_err());
        }
        assert_eq!(Config::default().language, Language::English);
    }

    #[test]
    fn config_round_trips_through_toml() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/config.toml");
        let mut config = Config {
            player_name: "someone".to_owned(),
            ..Default::default()
        };
        config.language = Language::German;
        config.ui_theme = ui::UiTheme::Sand;
        config.board.theme = SavedTheme::Moss;
        config.board.sound_profile = crate::sound::SoundProfile::Bell;
        config.board.notation = Notation::Figurine;
        config.analysis_limit = AnalysisLimit::Depth(30);
        config.view.sort = LineSort::Depth;
        config.view.scoresheet = ScoresheetStyle::Inline;
        config.view.graph = GraphTab::Time;
        config.window = Some(SavedWindow {
            width: 1600,
            height: 1000,
            maximized: true,
        });
        config.panels = PanelSizes {
            scoresheet_width: 360,
            board_size: Some(480),
        };
        config.engines.push(EngineConfig {
            id: 3,
            name: "Stockfish".to_owned(),
            path: "/usr/bin/stockfish".into(),
            options: [("Hash".to_owned(), "512".to_owned())]
                .into_iter()
                .collect(),
            profiles: vec![engine::EngineProfile {
                name: "Big hash".to_owned(),
                options: [("Hash".to_owned(), "4096".to_owned())]
                    .into_iter()
                    .collect(),
            }],
            icon: Some("/tmp/sf.png".into()),
            debug: true,
            arrow_color: Some(0x4e9de0),
            ..Default::default()
        });
        config.save(&path).unwrap();
        assert_eq!(Config::load(&path), config);
    }

    #[test]
    fn damaged_current_keeps_previous_valid_version_recoverable() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let original = Config::default();
        original.save(&path).unwrap();
        let changed = Config {
            player_name: "changed".into(),
            ..Default::default()
        };
        changed.save(&path).unwrap();
        std::fs::write(&path, "damaged").unwrap();
        assert_eq!(Config::load(&path), original);
        changed.save(&path).unwrap();
        assert_eq!(
            toml::from_str::<Config>(
                &std::fs::read_to_string(path.with_extension("previous")).unwrap()
            )
            .unwrap(),
            original
        );
    }

    #[test]
    fn backwards_analysis_defaults_to_a_fixed_node_count() {
        let config = toml::from_str::<Config>("").unwrap();
        assert_eq!(config.backward_nodes, DEFAULT_BACKWARD_NODES);
        let config = Config {
            backward_nodes: 2_000_000,
            ..Default::default()
        };
        let text = toml::to_string(&config).unwrap();
        assert_eq!(
            toml::from_str::<Config>(&text).unwrap().backward_nodes,
            2_000_000
        );
        let limit = AnalysisLimit::Nodes(42);
        let text = toml::to_string(&Config {
            analysis_limit: limit,
            ..Default::default()
        })
        .unwrap();
        assert_eq!(
            toml::from_str::<Config>(&text).unwrap().analysis_limit,
            limit
        );
    }

    #[test]
    fn open_tabs_and_their_names_round_trip() {
        assert_eq!(toml::from_str::<Config>("").unwrap().session, None);
        let session = SavedSession {
            tabs: vec![
                SavedTab {
                    kind: SavedTabKind::Analyse,
                    id: 1,
                    name: Some("Najdorf prep".into()),
                    recovery: Some("game-recovery.json".into()),
                },
                SavedTab {
                    kind: SavedTabKind::Database,
                    id: 4,
                    name: None,
                    recovery: None,
                },
                SavedTab {
                    kind: SavedTabKind::TestPositions,
                    id: 7,
                    name: None,
                    recovery: None,
                },
            ],
            active: 1,
        };
        let text = toml::to_string(&Config {
            session: Some(session.clone()),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(
            toml::from_str::<Config>(&text).unwrap().session,
            Some(session)
        );
    }

    #[test]
    fn every_ui_theme_round_trips_and_missing_theme_defaults_to_obsidian() {
        assert_eq!(
            toml::from_str::<Config>("").unwrap().ui_theme,
            ui::UiTheme::Obsidian
        );
        for ui_theme in ui::UiTheme::ALL {
            let config = Config {
                ui_theme,
                ..Default::default()
            };
            let saved = toml::to_string(&config).unwrap();
            assert_eq!(toml::from_str::<Config>(&saved).unwrap(), config);
        }
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
    fn the_retired_stone_board_loads_as_blue() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "[board]\ntheme = \"stone\"\n").unwrap();
        assert_eq!(Config::load(&path).board.theme, SavedTheme::Blue);
    }

    #[test]
    fn default_shortcuts_are_unique_and_all_bindable() {
        let defaults = default_shortcuts();
        let mut keys: Vec<&String> = defaults.values().collect();
        let count = keys.len();
        keys.sort();
        keys.dedup();
        assert_eq!(keys.len(), count, "two actions share a default key");
        for action in defaults.keys() {
            assert!(
                crate::shell::action_for(action).is_some(),
                "{action} has no action"
            );
        }
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
