//! User-installed ECO data, saved as one validated snapshot.
use anyhow::{Context, Result, ensure};
use chess_core::eco::OpeningTable;
use serde::{Deserialize, Serialize};
use std::{io::Read, path::Path, time::Duration};

const LIMIT: u64 = 4 * 1024 * 1024;
const FILE: &str = "opening-data.json";

#[derive(Serialize, Deserialize)]
struct Snapshot {
    revision: String,
    volumes: [String; 5],
}

fn parse(bytes: &[u8]) -> Result<(Snapshot, OpeningTable)> {
    ensure!(
        bytes.len() <= 6 * LIMIT as usize,
        "Opening snapshot is too large"
    );
    let snapshot: Snapshot = serde_json::from_slice(bytes)?;
    ensure!(
        snapshot.revision.len() == 40 && snapshot.revision.bytes().all(|b| b.is_ascii_hexdigit()),
        "Invalid opening revision"
    );
    let table = OpeningTable::parse(&snapshot.volumes).map_err(anyhow::Error::msg)?;
    Ok((snapshot, table))
}

fn read(path: &Path) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(6 * LIMIT + 1)
        .read_to_end(&mut bytes)?;
    Ok(bytes)
}

/// Invalid saved data never prevents startup; try the previous snapshot, then bundled data.
pub fn load(data: &Path) {
    let path = data.join(FILE);
    for candidate in [path.clone(), path.with_extension("previous")] {
        match read(&candidate).and_then(|bytes| parse(&bytes)) {
            Ok((_, table)) => {
                table.activate();
                return;
            }
            Err(err) if candidate.exists() => log::warn!("Cannot load opening data: {err:#}"),
            Err(_) => {}
        }
    }
}

pub fn update(data: &Path) -> Result<String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .https_only(true)
        .timeout_connect(Some(Duration::from_secs(20)))
        .timeout_global(Some(Duration::from_secs(120)))
        .build()
        .into();
    let fetch = |url: &str| -> Result<String> {
        let mut response = agent
            .get(url)
            .header("User-Agent", "Fast-Chess")
            .call()
            .with_context(|| format!("Downloading {url}"))?;
        let mut text = String::new();
        response
            .body_mut()
            .as_reader()
            .take(LIMIT + 1)
            .read_to_string(&mut text)?;
        ensure!(
            text.len() <= LIMIT as usize,
            "Opening download exceeds size limit"
        );
        Ok(text)
    };
    // Pin every file to the same commit, even if upstream changes during the download.
    let commit: serde_json::Value = serde_json::from_str(&fetch(
        "https://api.github.com/repos/lichess-org/chess-openings/commits/master",
    )?)?;
    let revision = commit["sha"]
        .as_str()
        .context("Missing upstream revision")?
        .to_owned();
    ensure!(
        revision.len() == 40 && revision.bytes().all(|b| b.is_ascii_hexdigit()),
        "Invalid upstream revision"
    );
    let mut volumes = Vec::new();
    for volume in 'a'..='e' {
        volumes.push(fetch(&format!(
            "https://raw.githubusercontent.com/lichess-org/chess-openings/{revision}/{volume}.tsv"
        ))?);
    }
    let snapshot = Snapshot {
        revision,
        volumes: volumes.try_into().unwrap(),
    };
    let bytes = serde_json::to_vec(&snapshot)?;
    let (_, table) = parse(&bytes)?;
    let count = table.len();
    crate::persistence::atomic_write_checked(&data.join(FILE), &bytes, |old| parse(old).is_ok())?;
    table.activate();
    Ok(format!(
        "Updated {count} openings (500 ECO codes). Revision {}. Active now.",
        &snapshot.revision[..12]
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_snapshots_are_rejected() {
        assert!(parse(b"{}").is_err());
        let snapshot = Snapshot {
            revision: "a".repeat(40),
            volumes: std::array::from_fn(|_| "eco\tname\tpgn\n".into()),
        };
        assert!(parse(&serde_json::to_vec(&snapshot).unwrap()).is_err());
    }

    #[test]
    fn saved_snapshot_round_trips_and_preserves_previous_data() {
        let snapshot = Snapshot {
            revision: "a".repeat(40),
            volumes: [
                include_str!("../../chess_core/data/chess-openings/a.tsv").into(),
                include_str!("../../chess_core/data/chess-openings/b.tsv").into(),
                include_str!("../../chess_core/data/chess-openings/c.tsv").into(),
                include_str!("../../chess_core/data/chess-openings/d.tsv").into(),
                include_str!("../../chess_core/data/chess-openings/e.tsv").into(),
            ],
        };
        let bytes = serde_json::to_vec(&snapshot).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE);
        crate::persistence::atomic_write_checked(&path, &bytes, |old| parse(old).is_ok()).unwrap();
        let (saved, table) = parse(&read(&path).unwrap()).unwrap();
        assert_eq!(saved.revision, snapshot.revision);
        assert!(table.len() > 3000);
        crate::persistence::atomic_write_checked(&path, &bytes, |old| parse(old).is_ok()).unwrap();
        assert!(parse(&read(&path.with_extension("previous")).unwrap()).is_ok());
        std::fs::write(&path, b"corrupt").unwrap();
        assert!(parse(&read(&path).unwrap()).is_err());
        assert!(parse(&read(&path.with_extension("previous")).unwrap()).is_ok());
    }

    #[test]
    #[ignore = "downloads the current public Lichess dataset"]
    fn download_and_activate() {
        let dir = tempfile::tempdir().unwrap();
        assert!(update(dir.path()).unwrap().contains("Active now"));
        assert!(parse(&read(&dir.path().join(FILE)).unwrap()).is_ok());
        assert_eq!(chess_core::classify(["e4", "c5"]).unwrap().eco, "B20");
    }
}
