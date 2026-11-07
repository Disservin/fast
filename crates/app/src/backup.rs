//! Versioned profile archives. SQLite uses its online backup API, including WAL data.
use crate::config::{Config, Paths};
use anyhow::{Context, Result, bail};
use rusqlite::{Connection, backup::Backup};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path};
#[derive(Clone, Serialize, Deserialize)]
pub struct Archive {
    pub version: u32,
    pub files: BTreeMap<String, Vec<u8>>,
}
fn sqlite_snapshot(path: &Path) -> Result<Vec<u8>> {
    let temp = tempfile::NamedTempFile::new()?;
    let source = Connection::open(path)?;
    let mut destination = Connection::open(temp.path())?;
    Backup::new(&source, &mut destination)?.run_to_completion(
        128,
        std::time::Duration::from_millis(5),
        None,
    )?;
    drop(destination);
    Ok(std::fs::read(temp.path())?)
}
impl Archive {
    pub fn capture(
        paths: &Paths,
        config: &Config,
        repertoire: &trainer::Repertoire,
    ) -> Result<Self> {
        let mut files = BTreeMap::new();
        files.insert(
            "config.toml".into(),
            toml::to_string_pretty(config)?.into_bytes(),
        );
        files.insert("repertoire.json".into(), serde_json::to_vec(repertoire)?);
        for name in ["game-recovery.json", "study.json"] {
            let path = paths.data_dir.join(name);
            if path.exists() {
                files.insert(name.into(), std::fs::read(path)?);
            }
        }
        if paths.notes_file().exists() {
            files.insert("notes.sqlite".into(), sqlite_snapshot(&paths.notes_file())?);
        }
        if let Ok(entries) = std::fs::read_dir(paths.databases_dir()) {
            for entry in entries {
                let path = entry?.path();
                if path.extension().is_some_and(|ext| ext == "sqlite") {
                    let name = path
                        .file_name()
                        .context("database filename")?
                        .to_str()
                        .context("database filename encoding")?;
                    files.insert(format!("databases/{name}"), sqlite_snapshot(&path)?);
                }
            }
        }
        Ok(Self { version: 1, files })
    }
    pub fn read(path: &Path) -> Result<Self> {
        let archive: Self = serde_json::from_slice(&std::fs::read(path)?)?;
        archive.validate()?;
        Ok(archive)
    }
    fn validate(&self) -> Result<()> {
        // Version zero used the same payload, before an explicit version was assigned.
        if self.version > 1 {
            bail!("This backup requires a newer Fast version");
        }
        for (name, bytes) in &self.files {
            let allowed = matches!(
                name.as_str(),
                "config.toml"
                    | "repertoire.json"
                    | "notes.sqlite"
                    | "game-recovery.json"
                    | "study.json"
            ) || (name.starts_with("databases/")
                && name.ends_with(".sqlite")
                && Path::new(name).components().count() == 2);
            if !allowed || name.contains("..") || name.contains('\\') {
                bail!("Invalid archive path: {name}");
            }
            match name.as_str() {
                "config.toml" => {
                    toml::from_str::<Config>(std::str::from_utf8(bytes)?)?;
                }
                "repertoire.json" => {
                    serde_json::from_slice::<trainer::Repertoire>(bytes)?;
                }
                _ if name.ends_with(".sqlite") => {
                    let temp = tempfile::NamedTempFile::new()?;
                    std::fs::write(temp.path(), bytes)?;
                    let db = Connection::open(temp.path())?;
                    let integrity: String =
                        db.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
                    if integrity != "ok" {
                        bail!("Damaged database: {name}");
                    }
                    let table = if name == "notes.sqlite" {
                        "notes"
                    } else {
                        "games"
                    };
                    db.prepare(&format!("SELECT * FROM {table} LIMIT 0"))?;
                }
                _ => {
                    serde_json::from_slice::<serde_json::Value>(bytes)?;
                }
            }
        }
        Ok(())
    }
    pub fn restore(&self, paths: &Paths, replace: bool) -> Result<()> {
        self.validate()?;
        let rollback = tempfile::tempdir()?;
        let mut plan = Vec::new();
        for (name, bytes) in &self.files {
            let target = if name == "config.toml" {
                paths.config_file.clone()
            } else {
                paths.data_dir.join(name)
            };
            if target.exists() && !replace {
                continue;
            }
            let old = if target.exists() {
                Some(if name.ends_with(".sqlite") {
                    sqlite_snapshot(&target)?
                } else {
                    std::fs::read(&target)?
                })
            } else {
                None
            };
            plan.push((target, bytes, old));
        }
        let mut applied = Vec::new();
        for (target, bytes, old) in plan {
            applied.push((target.clone(), old));
            if let Err(err) = restore_file(&target, bytes, rollback.path()) {
                for (path, old) in applied.into_iter().rev() {
                    match old {
                        Some(bytes) => restore_file(&path, &bytes, rollback.path())
                            .context("rolling back failed restore")?,
                        None => {
                            if path.exists() {
                                std::fs::remove_file(path)?;
                            }
                        }
                    }
                }
                return Err(err);
            }
        }
        Ok(())
    }
}
fn restore_file(path: &Path, bytes: &[u8], staging: &Path) -> Result<()> {
    if path.extension().is_some_and(|ext| ext == "sqlite") {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let source_path = staging.join("source.sqlite");
        std::fs::write(&source_path, bytes)?;
        let source = Connection::open(&source_path)?;
        let mut destination = Connection::open(path)?;
        Backup::new(&source, &mut destination)?.run_to_completion(
            128,
            std::time::Duration::from_millis(5),
            None,
        )?;
        Ok(())
    } else {
        crate::persistence::atomic_write(path, bytes)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_traversal_and_future_schema() {
        let mut archive = Archive {
            version: 1,
            files: [("databases/../evil.sqlite".into(), vec![])].into(),
        };
        assert!(archive.validate().is_err());
        archive.files.clear();
        archive.version = 99;
        assert!(archive.validate().is_err());
    }
    #[test]
    fn captures_wal_and_restores_with_conflict_choice() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths {
            config_file: dir.path().join("config.toml"),
            data_dir: dir.path().join("data"),
        };
        let database = db::Database::open(paths.databases_dir().join("test.sqlite")).unwrap();
        database.set_name("Original").unwrap();
        let archive = Archive::capture(
            &paths,
            &Config::default(),
            &trainer::Repertoire::new("x", true),
        )
        .unwrap();
        database.set_name("Changed").unwrap();
        archive.restore(&paths, false).unwrap();
        assert_eq!(database.name().unwrap(), "Changed");
        archive.restore(&paths, true).unwrap();
        assert_eq!(database.name().unwrap(), "Original");
    }
}
