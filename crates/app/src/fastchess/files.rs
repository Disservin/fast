//! fastchess on disk: the pinned release, its install, and the tournament folders it writes.

use super::*;

/// The pinned fastchess release, checked against the SHA-256 digests GitHub reports.
pub fn release() -> Entry {
    let asset = |os: &str, arch: &str, name: &str, sha256: &str| {
        let windows = os == "windows";
        Download {
            os: os.into(),
            arch: arch.into(),
            url: format!(
                "https://github.com/Disservin/fastchess/releases/download/v{VERSION}/fastchess-{name}.{}",
                if windows { "zip" } else { "tar" }
            ),
            sha256: sha256.into(),
            format: if windows { Format::Zip } else { Format::Tar },
            executable: format!(
                "fastchess-{name}/fastchess{}",
                if windows { ".exe" } else { "" }
            ),
        }
    };
    Entry {
        id: "fastchess".into(),
        name: "fastchess".into(),
        version: VERSION.into(),
        description: "Command-line tournament manager for UCI engines".into(),
        homepage: "https://github.com/Disservin/fastchess".into(),
        source_url: format!("https://github.com/Disservin/fastchess/tree/v{VERSION}"),
        license: "MIT".into(),
        license_url: format!(
            "https://raw.githubusercontent.com/Disservin/fastchess/v{VERSION}/LICENSE"
        ),
        protocol: "tool".into(),
        defaults: BTreeMap::new(),
        downloads: vec![
            asset(
                "linux",
                "x86_64",
                "linux-x86-64",
                "7ab4879eda78de88f210d87c04c65f9ea274d20e18d4f1745c6a3ddd8ef35d7e",
            ),
            asset(
                "linux",
                "aarch64",
                "linux-arm64",
                "93a3556493e50b63f8bdab1300135792eff71c0cebb45a1f086dfc57502601af",
            ),
            asset(
                "macos",
                "x86_64",
                "mac-x86-64",
                "2a9553621333133536154d07efa9d143a46c211755e760296493ee8d4f2810af",
            ),
            asset(
                "macos",
                "aarch64",
                "mac-arm64",
                "f7a812354d5c5944c0c8070c6fc492077589f98560412a977bb077cee4f441d8",
            ),
            asset(
                "windows",
                "x86_64",
                "windows-x86-64",
                "e4c66cceec4122d0638022da55e6841487a1dd9a4819685893749dbd7c645033",
            ),
            asset(
                "windows",
                "aarch64",
                "windows-arm64",
                "76d74706ac47808d3e4d2197434976e86e029f2def7265095c58ae7aa3845333",
            ),
        ],
    }
}

pub(super) fn tools_dir(data: &Path) -> PathBuf {
    data.join("tools")
}

/// The downloaded executable, when a complete installation of the pinned version exists.
pub fn installed(data: &Path) -> Option<PathBuf> {
    let entry = release();
    let asset = entry.native_download()?;
    let dir = entry.install_dir_in(&tools_dir(data), asset);
    let executable = dir.join(&asset.executable);
    (dir.join("fast-install.json").is_file() && executable.is_file()).then_some(executable)
}

pub fn install(data: &Path, cancel: Arc<AtomicBool>, progress: impl Fn(String)) -> Result<PathBuf> {
    crate::engine_catalog::install_into(&release(), &tools_dir(data), cancel, progress)
}

/// Where each tournament keeps its config, games and output.
pub fn tournaments_dir(data: &Path) -> PathBuf {
    data.join("tournaments")
}

/// Bytes used by the files under `path`, without following symlinks.
pub fn dir_size(path: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(path) else {
        return 0;
    };
    entries
        .flatten()
        .map(|entry| match entry.file_type() {
            Ok(kind) if kind.is_dir() => dir_size(&entry.path()),
            Ok(kind) if kind.is_file() => entry.metadata().map_or(0, |meta| meta.len()),
            _ => 0,
        })
        .sum()
}

/// Deletes every tournament folder except `keep`, and returns how many went and the bytes
/// freed. Stops at the first folder it cannot delete.
pub fn clear_tournaments(data: &Path, keep: Option<&Path>) -> Result<(usize, u64)> {
    let root = tournaments_dir(data);
    let Ok(entries) = std::fs::read_dir(&root) else {
        return Ok((0, 0));
    };
    let (mut count, mut freed) = (0, 0);
    for entry in entries.flatten() {
        let path = entry.path();
        if Some(path.as_path()) == keep {
            continue;
        }
        let size;
        if entry.file_type()?.is_dir() {
            size = dir_size(&path);
            std::fs::remove_dir_all(&path)?;
        } else {
            size = entry.metadata().map_or(0, |meta| meta.len());
            std::fs::remove_file(&path)?;
        }
        count += 1;
        freed += size;
    }
    Ok((count, freed))
}

/// A new, empty folder for one tournament.
pub fn new_run_dir(data: &Path) -> Result<PathBuf> {
    let root = tournaments_dir(data);
    std::fs::create_dir_all(&root)?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0);
    for suffix in 0..1000 {
        let dir = root.join(if suffix == 0 {
            format!("t{stamp}")
        } else {
            format!("t{stamp}-{suffix}")
        });
        if std::fs::create_dir(&dir).is_ok() {
            return Ok(dir);
        }
    }
    bail!("Could not create a tournament folder in {}", root.display())
}

/// A tournament found on disk that can be continued.
#[derive(Clone, Debug)]
pub struct SavedRun {
    pub dir: PathBuf,
    pub config: PathBuf,
    pub engines: Vec<String>,
    pub games: u64,
    pub modified: std::time::SystemTime,
}

/// Games recorded in a saved config's results, 0 when there is none.
pub fn saved_games(config: &Path) -> u64 {
    std::fs::read_to_string(config)
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .map_or(0, |value| games_in(&value))
}

pub(super) fn games_in(value: &serde_json::Value) -> u64 {
    value
        .get("stats")
        .and_then(|stats| stats.as_object())
        .map(|stats| {
            stats
                .values()
                .map(|pair| {
                    ["wins", "losses", "draws"]
                        .iter()
                        .filter_map(|key| pair.get(*key)?.as_u64())
                        .sum::<u64>()
                })
                .sum()
        })
        .unwrap_or(0)
}

/// Tournaments with a saved config, newest first.
pub fn saved_runs(data: &Path) -> Vec<SavedRun> {
    let Ok(entries) = std::fs::read_dir(tournaments_dir(data)) else {
        return Vec::new();
    };
    let mut runs: Vec<SavedRun> = entries
        .flatten()
        .filter_map(|entry| {
            let dir = entry.path();
            let config = dir.join("config.json");
            let modified = std::fs::metadata(&config).ok()?.modified().ok()?;
            let text = std::fs::read_to_string(&config).ok()?;
            let value: serde_json::Value = serde_json::from_str(&text).ok()?;
            let engines = value
                .get("engines")?
                .as_array()?
                .iter()
                .filter_map(|engine| engine.get("name")?.as_str().map(str::to_owned))
                .collect();
            let games = games_in(&value);
            Some(SavedRun {
                dir,
                config,
                engines,
                games,
                modified,
            })
        })
        .collect();
    runs.sort_by_key(|run| std::cmp::Reverse(run.modified));
    runs
}
