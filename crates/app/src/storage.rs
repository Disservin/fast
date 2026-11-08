//! What Fast keeps on disk, where, and how much space each kind of data takes.

use std::path::{Path, PathBuf};

use crate::config::Paths;

/// One kind of saved data, with the files that hold it.
#[derive(Clone, Debug)]
pub struct Store {
    /// Name shown in Settings › Data.
    pub name: &'static str,
    /// What the data is, in a player's words.
    pub about: &'static str,
    /// Files or folders, relative to the data folder (or the config folder for settings).
    pub entries: &'static [&'static str],
    /// Whether `entries` are in the config folder rather than the data folder.
    pub in_config: bool,
}

/// Every kind of data Fast saves, in the order Settings lists them.
pub const STORES: [Store; 10] = [
    Store {
        name: "Settings",
        about: "Preferences, engines, shortcuts and panel sizes",
        entries: &["config.toml", "config.previous"],
        in_config: true,
    },
    Store {
        name: "Databases",
        about: "Imported games and My games",
        entries: &["databases"],
        in_config: false,
    },
    Store {
        name: "Analysis cache",
        about: "Engine lines saved for positions you analysed",
        entries: &[
            "analysis.sqlite",
            "analysis.sqlite-wal",
            "analysis.sqlite-shm",
        ],
        in_config: false,
    },
    Store {
        name: "Repertoire",
        about: "Training lines and review schedule",
        entries: &["repertoire.json", "repertoire.previous"],
        in_config: false,
    },
    Store {
        name: "Notes",
        about: "Comments on positions",
        entries: &["notes.sqlite", "notes.sqlite-wal", "notes.sqlite-shm"],
        in_config: false,
    },
    Store {
        name: "Recovery",
        about: "Open games, restored after a crash",
        entries: &[
            "game-recovery.json",
            "game-recovery.previous",
            "play-recovery.json",
            "play-recovery.previous",
            "recovery",
        ],
        in_config: false,
    },
    Store {
        name: "Engines",
        about: "Engines downloaded from the catalog",
        entries: &["engines"],
        in_config: false,
    },
    Store {
        name: "Opening books",
        about: "Downloaded books for Play and Tournament",
        entries: &["books", "opening-data.json", "opening-data.previous"],
        in_config: false,
    },
    Store {
        name: "Tournaments",
        about: "Saved tournament runs and their games",
        entries: &["tournaments"],
        in_config: false,
    },
    Store {
        name: "Tools",
        about: "Fastchess, used to run tournaments",
        entries: &["tools"],
        in_config: false,
    },
];

/// Bytes used by each entry of `STORES`, then by anything else in the data folder.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Usage {
    pub stores: Vec<u64>,
    pub other: u64,
    /// The drive holding the data folder, when the system reports it.
    pub disk: Option<DiskSpace>,
}

/// Size and free space of a drive, in bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiskSpace {
    pub total: u64,
    pub free: u64,
}

impl Usage {
    /// All bytes Fast uses on disk.
    pub fn total(&self) -> u64 {
        self.stores.iter().sum::<u64>() + self.other
    }
}

/// Measures every store. Walks the folders, so call it off the UI thread.
pub fn measure(paths: &Paths) -> Usage {
    let config_dir = config_dir(paths);
    let stores = STORES
        .iter()
        .map(|store| {
            let base = if store.in_config {
                &config_dir
            } else {
                &paths.data_dir
            };
            store
                .entries
                .iter()
                .map(|entry| size(&base.join(entry)))
                .sum()
        })
        .collect();
    let known: Vec<&str> = STORES
        .iter()
        .filter(|store| !store.in_config)
        .flat_map(|store| store.entries.iter().copied())
        .collect();
    let other = std::fs::read_dir(&paths.data_dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| {
            entry
                .file_name()
                .to_str()
                .is_none_or(|name| !known.contains(&name))
        })
        .map(|entry| size(&entry.path()))
        .sum();
    Usage {
        stores,
        other,
        disk: disk_space(&paths.data_dir),
    }
}

/// Size and free space of the drive holding `path`. A folder not created yet is
/// looked up through its nearest existing parent.
pub fn disk_space(path: &Path) -> Option<DiskSpace> {
    let existing = path.ancestors().find(|dir| dir.exists())?;
    query_disk(existing)
}

#[cfg(unix)]
fn query_disk(path: &Path) -> Option<DiskSpace> {
    use std::os::unix::ffi::OsStrExt as _;
    let path = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut stat = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    // SAFETY: `path` is a NUL-terminated string and `stat` is written by the call
    // before it is read.
    let stat = unsafe {
        if libc::statvfs(path.as_ptr(), stat.as_mut_ptr()) != 0 {
            return None;
        }
        stat.assume_init()
    };
    // The field types differ between platforms; all fit in u64.
    #[allow(clippy::unnecessary_cast)]
    let (fragment, blocks, available) = (
        stat.f_frsize as u64,
        stat.f_blocks as u64,
        stat.f_bavail as u64,
    );
    Some(DiskSpace {
        total: blocks.saturating_mul(fragment),
        free: available.saturating_mul(fragment),
    })
}

#[cfg(windows)]
fn query_disk(path: &Path) -> Option<DiskSpace> {
    use std::os::windows::ffi::OsStrExt as _;
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    let (mut available, mut total) = (0u64, 0u64);
    // SAFETY: `wide` is NUL-terminated and both outputs point to live u64s.
    let ok = unsafe {
        windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW(
            wide.as_ptr(),
            &mut available,
            &mut total,
            std::ptr::null_mut(),
        )
    };
    (ok != 0).then_some(DiskSpace {
        total,
        free: available,
    })
}

#[cfg(not(any(unix, windows)))]
fn query_disk(_path: &Path) -> Option<DiskSpace> {
    None
}

/// The folder holding `config.toml`.
pub fn config_dir(paths: &Paths) -> PathBuf {
    paths
        .config_file
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
}

/// Bytes in a file, or in every file under a folder. Missing paths count as zero, and
/// symbolic links are not followed, so nothing outside Fast's folders is counted.
fn size(path: &Path) -> u64 {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return 0;
    };
    if !meta.is_dir() {
        return meta.len();
    }
    std::fs::read_dir(path)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| size(&entry.path()))
        .sum()
}

/// A byte count as a player reads it: "820 B", "14.2 MB", "1.3 GB".
pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];
    if bytes < 1000 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64 / 1000.;
    let mut unit = 0;
    while value >= 999.95 && unit + 1 < UNITS.len() {
        value /= 1000.;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

/// Shows a folder in the system's file manager.
pub fn reveal_folder(path: &Path) {
    #[cfg(target_os = "windows")]
    let program = "explorer";
    #[cfg(target_os = "macos")]
    let program = "open";
    #[cfg(all(unix, not(target_os = "macos")))]
    let program = "xdg-open";
    if let Err(err) = std::process::Command::new(program).arg(path).spawn() {
        log::warn!("could not open {}: {err}", path.display());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, bytes: usize) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, vec![0u8; bytes]).unwrap();
    }

    #[test]
    fn measures_each_store_and_the_rest() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths {
            config_file: dir.path().join("config/config.toml"),
            data_dir: dir.path().join("data"),
        };
        write(&paths.config_file, 10);
        write(&paths.data_dir.join("databases/my_games.sqlite"), 300);
        write(&paths.data_dir.join("databases/twic.sqlite"), 200);
        write(&paths.data_dir.join("analysis.sqlite-wal"), 40);
        write(&paths.data_dir.join("recovery/analysis-2.json"), 7);
        write(&paths.data_dir.join("game-recovery.json"), 3);
        write(&paths.data_dir.join("stray.log"), 5);

        let usage = measure(&paths);
        let of = |name: &str| {
            let index = STORES.iter().position(|store| store.name == name).unwrap();
            usage.stores[index]
        };
        assert_eq!(of("Settings"), 10);
        assert_eq!(of("Databases"), 500);
        assert_eq!(of("Analysis cache"), 40);
        assert_eq!(of("Recovery"), 10);
        assert_eq!(of("Engines"), 0);
        assert_eq!(usage.other, 5);
        assert_eq!(usage.total(), 565);
    }

    #[test]
    fn missing_folders_measure_zero() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths {
            config_file: dir.path().join("none/config.toml"),
            data_dir: dir.path().join("none"),
        };
        assert_eq!(measure(&paths).total(), 0);
    }

    #[test]
    fn reports_the_drive_of_a_missing_folder() {
        let dir = tempfile::tempdir().unwrap();
        let disk = disk_space(&dir.path().join("not/yet/created")).unwrap();
        assert!(disk.total > 0);
        assert!(disk.free <= disk.total);
    }

    #[test]
    fn formats_bytes() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(999), "999 B");
        assert_eq!(format_bytes(1_000), "1.0 KB");
        assert_eq!(format_bytes(14_230_000), "14.2 MB");
        assert_eq!(format_bytes(999_960), "1.0 MB");
        assert_eq!(format_bytes(1_300_000_000), "1.3 GB");
    }
}
