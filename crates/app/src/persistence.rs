//! Durable replacement of user files, retaining the previous version.
use anyhow::{Context, Result};
use std::{io::Write, path::Path};

pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    atomic_write_checked(path, bytes, |_| true)
}

pub fn atomic_write_checked(
    path: &Path,
    bytes: &[u8],
    valid: impl Fn(&[u8]) -> bool,
) -> Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent)?;
    let mut staged = tempfile::NamedTempFile::new_in(parent)?;
    staged.write_all(bytes)?;
    staged.as_file().sync_all()?;
    if path.exists() && valid(&std::fs::read(path)?) {
        let mut previous = tempfile::NamedTempFile::new_in(parent)?;
        previous.write_all(&std::fs::read(path)?)?;
        previous.as_file().sync_all()?;
        previous
            .persist(path.with_extension("previous"))
            .context("preserving previous version")?;
    }
    staged.persist(path).context("replacing saved file")?;
    #[cfg(unix)]
    std::fs::File::open(parent)?.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn replacement_preserves_previous_and_failure_preserves_destination() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        atomic_write(&path, b"first").unwrap();
        atomic_write(&path, b"second").unwrap();
        assert_eq!(
            std::fs::read(path.with_extension("previous")).unwrap(),
            b"first"
        );
        std::fs::remove_file(path.with_extension("previous")).unwrap();
        std::fs::create_dir(path.with_extension("previous")).unwrap();
        assert!(atomic_write(&path, b"third").is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"second");
        assert!(atomic_write(&path.join("invalid"), b"bad").is_err());
        assert_eq!(std::fs::read(path).unwrap(), b"second");
    }
}
