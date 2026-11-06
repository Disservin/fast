//! Shared application state: configuration, databases, repertoire, notes, toasts.

use std::{
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::Result;
use chess_core::notation::numbered;
use db::{Database, NoteStore};
use engine::{EngineConfig, EngineId, EngineManager, EngineStatus};
use gpui::{App, AppContext as _, Context, Entity, EventEmitter, PathPromptOptions, SharedString};
use trainer::{BookGap, GameLine, Leak, Repertoire, deviation, find_book_gaps, find_leaks};

use crate::config::{Config, Paths};

/// One database file known to the app.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DbEntry {
    pub path: PathBuf,
    pub name: SharedString,
    pub games: u64,
}

pub type PathCallback = Box<dyn FnOnce(PathBuf, &mut App)>;

/// A file request waiting for the user to type a path.
pub struct PendingPath {
    pub title: SharedString,
    pub on_path: PathCallback,
}

pub struct Toast {
    pub id: u64,
    pub text: SharedString,
}

/// Emitted when something other screens should re-read has changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelEvent {
    BoardPrefs,
    Databases,
    Repertoire,
    Engines,
}

pub struct AppModel {
    pub paths: Paths,
    pub demo: bool,
    pub config: Config,
    pub databases: Vec<DbEntry>,
    pub repertoire: Repertoire,
    pub leaks: Vec<Leak>,
    pub gaps: Vec<BookGap>,
    pub notes: Option<NoteStore>,
    pub toasts: Vec<Toast>,
    next_toast: u64,
    pub pending_path: Option<PendingPath>,
    pub manager: EngineManager,
    pub engine_status: std::collections::BTreeMap<EngineId, EngineStatus>,
}

impl EventEmitter<ModelEvent> for AppModel {}

/// Asks for a file through the system dialog, falling back to a typed path when no
/// dialog service exists. `save_name` selects a save dialog with that default name.
pub fn pick_path(
    model: Entity<AppModel>,
    title: &'static str,
    save_name: Option<&'static str>,
    cx: &mut App,
    on_path: impl FnOnce(PathBuf, &mut App) + 'static,
) {
    let prompt = match save_name {
        Some(name) => {
            let dir = dirs::document_dir().unwrap_or_else(|| PathBuf::from("."));
            let answer = cx.prompt_for_new_path(&dir, Some(name));
            cx.background_spawn(async move {
                answer
                    .await
                    .map(|result| result.map(|path| path.map(|path| vec![path])))
            })
        }
        None => {
            let answer = cx.prompt_for_paths(PathPromptOptions {
                files: true,
                directories: false,
                multiple: false,
                prompt: Some(title.into()),
            });
            cx.background_spawn(answer)
        }
    };
    cx.spawn(async move |cx| match prompt.await {
        Ok(Ok(Some(paths))) => {
            if let Some(path) = paths.into_iter().next() {
                cx.update(|cx| on_path(path, cx));
            }
        }
        // Cancelled by the user.
        Ok(Ok(None)) => {}
        Ok(Err(err)) => {
            log::info!("no file dialog: {err:#}");
            model.update(cx, |model, cx| {
                model.pending_path = Some(PendingPath {
                    title: title.into(),
                    on_path: Box::new(on_path),
                });
                cx.notify();
            });
        }
        Err(_) => {}
    })
    .detach();
}

/// Days since the Unix epoch; the scheduler's clock.
pub fn today() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| (elapsed.as_secs() / 86_400) as i64)
        .unwrap_or(0)
}

impl AppModel {
    pub fn load(demo: bool) -> Self {
        let paths = Paths::resolve(demo);
        let mut config = Config::load(&paths.config_file);
        if demo {
            crate::demo::prepare(&paths, &mut config);
        }
        let repertoire = std::fs::read_to_string(paths.repertoire_file())
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_else(|| Repertoire::new("My repertoire", true));
        let notes = match NoteStore::open(paths.notes_file()) {
            Ok(notes) => Some(notes),
            Err(err) => {
                log::error!("notes unavailable: {err}");
                None
            }
        };
        let engine_status = config
            .engines
            .iter()
            .map(|engine| {
                let status = if engine.enabled {
                    EngineStatus::Ready
                } else {
                    EngineStatus::Disabled
                };
                (engine.id, status)
            })
            .collect();
        let mut model = Self {
            paths,
            demo,
            config,
            databases: Vec::new(),
            repertoire,
            leaks: Vec::new(),
            gaps: Vec::new(),
            notes,
            toasts: Vec::new(),
            next_toast: 0,
            pending_path: None,
            manager: EngineManager::default(),
            engine_status,
        };
        model.refresh_databases();
        model.refresh_leaks();
        model
    }

    /// Re-scans the databases directory.
    pub fn refresh_databases(&mut self) {
        let dir = self.paths.databases_dir();
        let mut entries = Vec::new();
        if let Ok(read) = std::fs::read_dir(&dir) {
            for file in read.flatten() {
                let path = file.path();
                if path.extension().is_none_or(|ext| ext != "sqlite") {
                    continue;
                }
                match Database::open(&path).and_then(|db| Ok((db.name()?, db.game_count()?))) {
                    Ok((name, games)) => entries.push(DbEntry {
                        path,
                        name: name.into(),
                        games,
                    }),
                    Err(err) => log::warn!("skipping {}: {err}", path.display()),
                }
            }
        }
        // "My games" first, then reference databases, each alphabetical.
        entries.sort_by_key(|entry| {
            let name = entry.name.to_string();
            let rank = if name == "My games" {
                0
            } else if name.starts_with("Reference") {
                1
            } else {
                2
            };
            (rank, file_key(&entry.path))
        });
        self.databases = entries;
    }

    /// The database holding the user's own games, created on demand.
    pub fn my_games_path(&self) -> PathBuf {
        self.paths.databases_dir().join("my_games.sqlite")
    }

    /// The first database that is not the user's own: the "reference" column.
    pub fn reference_db(&self) -> Option<&DbEntry> {
        let mine = self.my_games_path();
        self.databases.iter().find(|entry| entry.path != mine)
    }

    pub fn my_games(&self) -> Option<&DbEntry> {
        let mine = self.my_games_path();
        self.databases.iter().find(|entry| entry.path == mine)
    }

    pub fn toast(&mut self, text: impl Into<SharedString>, cx: &mut Context<Self>) {
        let id = self.next_toast;
        self.next_toast += 1;
        self.toasts.push(Toast {
            id,
            text: text.into(),
        });
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_secs(5)).await;
            let _ = this.update(cx, |model, cx| {
                model.toasts.retain(|toast| toast.id != id);
                cx.notify();
            });
        })
        .detach();
    }

    pub fn save_config(&self, cx: &mut Context<Self>) {
        // Demo mode never overwrites its generated profile on disk.
        if self.demo {
            cx.notify();
            return;
        }
        if let Err(err) = self.config.save(&self.paths.config_file) {
            log::error!("could not save config: {err:#}");
        }
        cx.notify();
    }

    pub fn save_repertoire(&mut self, cx: &mut Context<Self>) {
        match serde_json::to_string(&self.repertoire) {
            Ok(text) => {
                let path = self.paths.repertoire_file();
                if let Some(parent) = path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                if let Err(err) = std::fs::write(&path, text) {
                    log::error!("could not save repertoire: {err}");
                }
            }
            Err(err) => log::error!("could not serialise repertoire: {err}"),
        }
        cx.emit(ModelEvent::Repertoire);
        cx.notify();
    }

    pub fn note(&self, hash: u64) -> String {
        self.notes
            .as_ref()
            .and_then(|notes| notes.get(hash).ok())
            .unwrap_or_default()
    }

    pub fn set_note(&self, hash: u64, text: &str) {
        if let Some(notes) = &self.notes
            && let Err(err) = notes.set(hash, text)
        {
            log::error!("could not save note: {err}");
        }
    }

    /// Recomputes `left_rep` for the user's games and the leak lists.
    pub fn refresh_leaks(&mut self) {
        let path = self.my_games_path();
        if !path.exists() {
            self.leaks.clear();
            self.gaps.clear();
            return;
        }
        match compute_leaks(&path, &self.config.player_name, &self.repertoire) {
            Ok((leaks, gaps)) => {
                self.leaks = leaks;
                self.gaps = gaps;
            }
            Err(err) => log::error!("leak detection failed: {err:#}"),
        }
    }

    pub fn engine(&self, id: EngineId) -> Option<&EngineConfig> {
        self.config.engines.iter().find(|engine| engine.id == id)
    }

    pub fn engine_mut(&mut self, id: EngineId) -> Option<&mut EngineConfig> {
        self.config
            .engines
            .iter_mut()
            .find(|engine| engine.id == id)
    }

    /// Engines to run in Analyse, in table order.
    pub fn analysis_engines(&self) -> Vec<EngineConfig> {
        self.config
            .engines
            .iter()
            .filter(|engine| engine.enabled && engine.use_analysis)
            .cloned()
            .collect()
    }

    /// Engines offered as Play opponents.
    pub fn play_engines(&self) -> Vec<EngineConfig> {
        self.config
            .engines
            .iter()
            .filter(|engine| engine.enabled && engine.use_play)
            .cloned()
            .collect()
    }
}

fn file_key(path: &Path) -> String {
    path.file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Writes each game's deviation into the database and summarises the leaks.
pub fn compute_leaks(
    path: &Path,
    player: &str,
    repertoire: &Repertoire,
) -> Result<(Vec<Leak>, Vec<BookGap>)> {
    let mut database = Database::open(path)?;
    let lines = database.player_lines(player)?;
    database.in_transaction(|db| {
        for (id, sans, white, _) in &lines {
            let left = deviation(repertoire, sans, *white)
                .map(|ply| numbered(ply as u32 + 1, sans[ply].trim_end_matches(['+', '#'])));
            db.set_left_rep(*id, left.as_deref())?;
        }
        Ok(())
    })?;
    let games: Vec<GameLine> = lines
        .into_iter()
        .map(|(_, sans, user_white, score)| GameLine {
            sans,
            user_white,
            score,
        })
        .collect();
    Ok((
        find_leaks(repertoire, &games),
        find_book_gaps(repertoire, &games),
    ))
}

/// A sensible display name and version from an engine's self-reported id.
pub fn split_engine_name(id_name: &str, fallback: &str) -> (String, String) {
    let text = id_name.trim();
    if text.is_empty() {
        return (fallback.to_owned(), String::new());
    }
    match text.split_once(' ') {
        Some((name, rest)) => {
            let version = rest.split_ascii_whitespace().next().unwrap_or("");
            if version.starts_with(|c: char| c.is_ascii_digit()) || version.starts_with("dev") {
                (name.to_owned(), version.to_owned())
            } else {
                (text.to_owned(), String::new())
            }
        }
        None => (text.to_owned(), String::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use db::{ImportControl, import_games};

    #[test]
    fn engine_names_split_on_versions() {
        assert_eq!(
            split_engine_name("Stockfish 17.1", "x"),
            ("Stockfish".to_owned(), "17.1".to_owned())
        );
        assert_eq!(
            split_engine_name("Stockfish dev-20260101-abc", "x"),
            ("Stockfish".to_owned(), "dev-20260101-abc".to_owned())
        );
        assert_eq!(
            split_engine_name("Weird Engine", "x"),
            ("Weird Engine".to_owned(), String::new())
        );
        assert_eq!(
            split_engine_name("", "fallback"),
            ("fallback".to_owned(), String::new())
        );
    }

    #[test]
    fn leak_scan_marks_deviating_games() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mine.sqlite");
        let pgn = "[White \"me\"]\n[Black \"x\"]\n[Result \"0-1\"]\n\n1. e4 c5 2. c3 0-1\n\n\
                   [White \"me\"]\n[Black \"x\"]\n[Result \"1-0\"]\n\n1. e4 c5 2. Nf3 1-0\n";
        import_games(&path, pgn.as_bytes(), "", &ImportControl::default(), |_| {}).unwrap();
        let mut rep = Repertoire::new("r", true);
        rep.add_line(&["e4", "c5", "Nf3"]).unwrap();
        let (leaks, _) = compute_leaks(&path, "me", &rep).unwrap();
        assert_eq!(leaks.len(), 1);
        assert_eq!(leaks[0].played, "c3");
        let database = Database::open(&path).unwrap();
        let ids = database.filtered_ids(&db::Filter::default()).unwrap();
        let rows = database.rows_by_ids(&ids).unwrap();
        let left: Vec<_> = rows.iter().map(|row| row.left_rep.clone()).collect();
        assert!(left.contains(&Some("2.c3".to_owned())));
        assert!(left.contains(&None));
    }
}
