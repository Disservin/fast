//! Shared application state: configuration, databases, repertoire, notes, toasts.

use std::{
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::Result;
use chess_core::notation::numbered;
use db::{Database, NoteStore};
use engine::{EngineConfig, EngineId, EngineManager, EngineStatus};
use gpui::{
    App, AppContext as _, Context, Entity, EventEmitter, PathPromptOptions, SharedString, Task,
};
use trainer::{BookGap, GameLine, Leak, Repertoire, deviation, find_book_gaps, find_leaks};

use crate::config::{Config, Paths};

/// One database file known to the app.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DbEntry {
    pub path: PathBuf,
    pub name: SharedString,
    pub games: u64,
    /// The PGN file the database was last imported from, for "Re-import from file".
    pub source_file: Option<PathBuf>,
}

pub type PathCallback = Box<dyn FnOnce(Vec<PathBuf>, &mut App)>;

/// A file request waiting for the user to type a path.
pub struct PendingPath {
    pub title: SharedString,
    pub multiple: bool,
    pub on_paths: PathCallback,
}

pub struct Toast {
    pub id: u64,
    pub text: SharedString,
    /// A button on the toast, such as "Show details".
    pub action: Option<ToastAction>,
}

/// What a toast's button does when clicked.
pub type ToastRun = std::rc::Rc<dyn Fn(&mut gpui::Window, &mut App)>;

/// A labelled button on a toast and what it does.
pub struct ToastAction {
    pub label: SharedString,
    pub run: ToastRun,
}

/// Emitted when something other screens should re-read has changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelEvent {
    Restored,
    BoardPrefs,
    Databases,
    Repertoire,
    Engines,
}

pub struct AppModel {
    pub unsaved_config: bool,
    pub unsaved_repertoire: bool,
    pub pending_backup: Option<crate::backup::Archive>,
    pub paths: Paths,
    pub demo: bool,
    pub config: Config,
    pub databases: Vec<DbEntry>,
    pub repertoire: Repertoire,
    pub leaks: Vec<Leak>,
    pub gaps: Vec<BookGap>,
    pub notes: Option<NoteStore>,
    pub analyses: Option<crate::analysis_store::AnalysisStore>,
    pub toasts: Vec<Toast>,
    next_toast: u64,
    pub pending_path: Option<PendingPath>,
    leak_task: Option<Task<()>>,
    config_save_task: Option<Task<()>>,
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
    pick_path_kind(
        model,
        title,
        save_name,
        false,
        false,
        cx,
        move |paths, cx| {
            if let Some(path) = paths.into_iter().next() {
                on_path(path, cx);
            }
        },
    );
}

/// Select several files in one native dialog.
pub fn pick_paths(
    model: Entity<AppModel>,
    title: &'static str,
    cx: &mut App,
    on_paths: impl FnOnce(Vec<PathBuf>, &mut App) + 'static,
) {
    pick_path_kind(model, title, None, false, true, cx, on_paths);
}

/// Select a directory using the platform's native folder dialog.
pub fn pick_directory(
    model: Entity<AppModel>,
    title: &'static str,
    cx: &mut App,
    on_path: impl FnOnce(PathBuf, &mut App) + 'static,
) {
    pick_path_kind(model, title, None, true, false, cx, move |paths, cx| {
        if let Some(path) = paths.into_iter().next() {
            on_path(path, cx);
        }
    });
}

fn pick_path_kind(
    model: Entity<AppModel>,
    title: &'static str,
    save_name: Option<&'static str>,
    directory: bool,
    multiple: bool,
    cx: &mut App,
    on_paths: impl FnOnce(Vec<PathBuf>, &mut App) + 'static,
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
                files: !directory,
                directories: directory,
                multiple,
                prompt: Some(title.into()),
            });
            cx.background_spawn(answer)
        }
    };
    cx.spawn(async move |cx| match prompt.await {
        Ok(Ok(Some(paths))) => {
            cx.update(|cx| on_paths(paths, cx));
        }
        // Cancelled by the user.
        Ok(Ok(None)) => {}
        Ok(Err(err)) => {
            log::info!("no file dialog: {err:#}");
            model.update(cx, |model, cx| {
                model.pending_path = Some(PendingPath {
                    title: title.into(),
                    multiple,
                    on_paths: Box::new(on_paths),
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
        crate::opening_data::load(&paths.data_dir);
        let mut config = Config::load(&paths.config_file);
        if demo {
            crate::demo::prepare(&paths, &mut config);
        }
        ui::i18n::set_language(config.language);
        let mut repertoire: Repertoire = [
            paths.repertoire_file(),
            paths.repertoire_file().with_extension("previous"),
        ]
        .into_iter()
        .filter_map(|path| std::fs::read(path).ok())
        .find_map(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_else(|| Repertoire::new("My repertoire", true));
        repertoire.migrate_cards();
        let notes = match NoteStore::open(paths.notes_file()) {
            Ok(notes) => Some(notes),
            Err(err) => {
                log::error!("notes unavailable: {err}");
                None
            }
        };
        let analyses = match crate::analysis_store::AnalysisStore::open(paths.analysis_file()) {
            Ok(analyses) => Some(analyses),
            Err(err) => {
                log::error!("saved analysis unavailable: {err:#}");
                None
            }
        };
        let engine_status = config
            .engines
            .iter()
            .map(|engine| (engine.id, engine.idle_status()))
            .collect();
        let mut model = Self {
            unsaved_config: false,
            unsaved_repertoire: false,
            pending_backup: None,
            paths,
            demo,
            config,
            databases: Vec::new(),
            repertoire,
            leaks: Vec::new(),
            gaps: Vec::new(),
            notes,
            analyses,
            toasts: Vec::new(),
            next_toast: 0,
            pending_path: None,
            leak_task: None,
            config_save_task: None,
            manager: EngineManager::default(),
            engine_status,
        };
        model.refresh_databases();
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
                match Database::open(&path)
                    .and_then(|db| Ok((db.name()?, db.game_count()?, db.source_file()?)))
                {
                    Ok((name, games, source_file)) => entries.push(DbEntry {
                        path,
                        name: name.into(),
                        games,
                        source_file,
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

    pub fn set_engine_status(
        &mut self,
        id: EngineId,
        status: EngineStatus,
        cx: &mut Context<Self>,
    ) {
        self.engine_status.insert(id, status);
        cx.notify();
    }

    /// Changes which features use an engine, enabling or disabling it to match.
    pub fn set_engine_use(
        &mut self,
        id: EngineId,
        change: impl FnOnce(&mut EngineConfig),
        cx: &mut Context<Self>,
    ) {
        let Some(config) = self.engine_mut(id) else {
            return;
        };
        change(config);
        config.sync_enabled();
        let status = config.idle_status();
        self.engine_status.insert(id, status);
        self.save_config(cx);
        cx.emit(ModelEvent::Engines);
    }

    pub fn toast(&mut self, text: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.push_toast(text.into(), None, cx);
    }

    /// Shows a toast with a button, such as "Show details". It stays longer than a
    /// plain toast so there is time to use the button.
    pub fn toast_with_action(
        &mut self,
        text: impl Into<SharedString>,
        label: &str,
        run: impl Fn(&mut gpui::Window, &mut App) + 'static,
        cx: &mut Context<Self>,
    ) {
        let action = ToastAction {
            label: ui::i18n::t(label).to_owned().into(),
            run: std::rc::Rc::new(run),
        };
        self.push_toast(text.into(), Some(action), cx);
    }

    /// Removes a toast at once, as after its button was used.
    pub fn dismiss_toast(&mut self, id: u64, cx: &mut Context<Self>) {
        self.toasts.retain(|toast| toast.id != id);
        cx.notify();
    }

    fn push_toast(
        &mut self,
        text: SharedString,
        action: Option<ToastAction>,
        cx: &mut Context<Self>,
    ) {
        let id = self.next_toast;
        self.next_toast += 1;
        let seconds = if action.is_some() { 10 } else { 5 };
        self.toasts.push(Toast {
            id,
            text: ui::i18n::t(&text).to_owned().into(),
            action,
        });
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_secs(seconds))
                .await;
            let _ = this.update(cx, |model, cx| {
                model.toasts.retain(|toast| toast.id != id);
                cx.notify();
            });
        })
        .detach();
    }

    /// Coalesce live resize events into one write after the drag settles.
    pub fn schedule_config_save(&mut self, cx: &mut Context<Self>) {
        self.unsaved_config = !self.demo;
        self.config_save_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(250))
                .await;
            let _ = this.update(cx, |model, cx| model.save_config(cx));
        }));
        cx.notify();
    }

    pub fn save_config(&mut self, cx: &mut Context<Self>) {
        // Demo mode never overwrites its generated profile on disk.
        if self.demo {
            cx.notify();
            return;
        }
        self.unsaved_config = true;
        if let Err(err) = self.config.save(&self.paths.config_file) {
            self.toast(format!("Settings remain unsaved: {err:#}. Check storage permissions and retry by changing the setting."), cx);
        } else {
            self.unsaved_config = false;
        }
        cx.notify();
    }

    pub fn save_repertoire(&mut self, cx: &mut Context<Self>) {
        self.unsaved_repertoire = true;
        match serde_json::to_string(&self.repertoire) {
            Ok(text) => {
                let path = self.paths.repertoire_file();
                if let Some(parent) = path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                if let Err(err) =
                    crate::persistence::atomic_write_checked(&path, text.as_bytes(), |bytes| {
                        serde_json::from_slice::<Repertoire>(bytes).is_ok()
                    })
                {
                    self.toast(format!("Repertoire remains unsaved: {err}. Check storage space and permissions, then retry Save."), cx);
                } else {
                    self.unsaved_repertoire = false;
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

    /// Recomputes `left_rep` for the user's games and the leak lists on a background
    /// thread, then tells screens to re-read. Collections can be large.
    pub fn refresh_leaks(&mut self, cx: &mut Context<Self>) {
        let path = self.my_games_path();
        if !path.exists() {
            self.leaks.clear();
            self.gaps.clear();
            return;
        }
        let players = self.config.my_names();
        let repertoire = self.repertoire.clone();
        let task = cx.background_spawn(async move { compute_leaks(&path, &players, &repertoire) });
        self.leak_task = Some(cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |model, cx| {
                match result {
                    Ok((leaks, gaps)) => {
                        model.leaks = leaks;
                        model.gaps = gaps;
                    }
                    Err(err) => log::error!("leak detection failed: {err:#}"),
                }
                cx.emit(ModelEvent::Databases);
                cx.notify();
            });
        }));
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

/// Opens the raw-output window for a running engine, or says how to start it.
/// Shows a toast through a handle to the model, for screens that hold one.
pub trait ToastExt {
    fn toast(&self, text: impl Into<SharedString>, cx: &mut App);
}

impl ToastExt for Entity<AppModel> {
    fn toast(&self, text: impl Into<SharedString>, cx: &mut App) {
        let text = text.into();
        self.update(cx, |model, cx| model.toast(text, cx));
    }
}

pub fn open_debug_window(model: &Entity<AppModel>, id: EngineId, cx: &mut App) {
    let (log, name) = {
        let model = model.read(cx);
        let name = model
            .engine(id)
            .map_or_else(|| "Engine".to_owned(), |config| config.name.clone());
        (model.manager.get(id).map(|handle| handle.log()), name)
    };
    match log {
        Some(log) => crate::debug_window::DebugWindow::open(&name, log, cx),
        None => model.update(cx, |model, cx| {
            model.toast(
                format!("{name} is not running. Turn it on for analysis to see its output."),
                cx,
            );
        }),
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
    players: &[String],
    repertoire: &Repertoire,
) -> Result<(Vec<Leak>, Vec<BookGap>)> {
    let mut database = Database::open(path)?;
    let lines = database.player_lines(players)?;
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
    use db::{ImportControl, ImportOptions, import_games};

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
        import_games(
            &path,
            pgn.as_bytes(),
            &ImportOptions::default(),
            &ImportControl::default(),
            |_| {},
        )
        .unwrap();
        let mut rep = Repertoire::new("r", true);
        rep.add_line(&["e4", "c5", "Nf3"]).unwrap();
        let (leaks, _) = compute_leaks(&path, &["me".to_owned()], &rep).unwrap();
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
