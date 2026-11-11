//! Adds local and downloaded PGN collections without exposing unfinished databases.

use anyhow::{Result, bail, ensure};
use db::{Database, ImportControl, ImportOptions, import_games};
use std::{
    fs::File,
    io::{Read, Seek, Write},
    path::{Path, PathBuf},
    time::Duration,
};

/// Where a new collection's games come from.
pub enum Source {
    Local(PathBuf),
    Web(String),
    Twic,
}

/// Finds the latest PGN archive advertised by the TWIC publisher.
fn latest_twic(html: &str) -> Result<String> {
    let issue = html
        .split("twic")
        .filter_map(|tail| {
            let digits: String = tail.chars().take_while(char::is_ascii_digit).collect();
            (!digits.is_empty() && tail[digits.len()..].starts_with("g.zip"))
                .then(|| digits.parse::<u32>().ok())
                .flatten()
        })
        .max()
        .ok_or_else(|| anyhow::anyhow!("No TWIC PGN download found"))?;
    Ok(format!("https://theweekinchess.com/zips/twic{issue}g.zip"))
}

/// Downloads into a temporary file with bounded memory, progress and cancellation.
fn download(
    url: &str,
    control: &ImportControl,
    progress: &impl Fn(String),
) -> Result<tempfile::NamedTempFile> {
    ensure!(url.starts_with("https://"), "Use an HTTPS download URL");
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .https_only(true)
        .timeout_global(Some(Duration::from_secs(24 * 60 * 60)))
        .timeout_recv_body(Some(Duration::from_secs(30)))
        .build()
        .into();
    let mut response = agent.get(url).call()?;
    let mut output = tempfile::NamedTempFile::new()?;
    let mut input = response.body_mut().as_reader();
    let mut buffer = [0; 64 * 1024];
    let mut bytes = 0_u64;
    loop {
        ensure!(!control.is_cancelled(), "Cancelled");
        let read = input.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        output.write_all(&buffer[..read])?;
        bytes += read as u64;
        progress(ui::i18n::tf(
            "Downloaded {} MB",
            &[&format!("{:.1}", bytes as f64 / 1_048_576.)],
        ));
    }
    output.flush()?;
    Ok(output)
}

/// Imports plain, gzipped or zipped PGNs, leaving all work in a temporary database.
fn import_path(
    path: &Path,
    name: &str,
    target: &Path,
    options: &ImportOptions,
    control: &ImportControl,
    progress: &impl Fn(String),
) -> Result<()> {
    let report = |value: db::ImportProgress| {
        progress(ui::i18n::tf(
            "Imported {} games",
            &[&value.games.to_string()],
        ))
    };
    let mut file = File::open(path)?;
    let mut magic = [0; 4];
    let read = file.read(&mut magic)?;
    file.rewind()?;
    let name = name.split('?').next().unwrap_or(name).to_ascii_lowercase();
    if name.ends_with(".zip") || (read == 4 && magic == *b"PK\x03\x04") {
        let mut zip = zip::ZipArchive::new(file)?;
        let mut found = false;
        for index in 0..zip.len() {
            ensure!(!control.is_cancelled(), "Cancelled");
            let entry = zip.by_index(index)?;
            if entry.is_file() && entry.name()?.to_ascii_lowercase().ends_with(".pgn") {
                found = true;
                import_games(target, entry, options, control, report)?;
            }
        }
        ensure!(found, "The archive contains no PGN files");
    } else if name.ends_with(".gz") || (read >= 2 && magic[..2] == [0x1f, 0x8b]) {
        import_games(
            target,
            flate2::read::MultiGzDecoder::new(file),
            options,
            control,
            report,
        )?;
    } else if name.ends_with(".pgn") || name.starts_with("https://") {
        import_games(target, file, options, control, report)?;
    } else {
        bail!("Choose a PGN, PGN.gz, ZIP or Fast SQLite database");
    }
    ensure!(!control.is_cancelled(), "Cancelled");
    Ok(())
}

/// Validates a Fast database and copies a consistent snapshot without changing the source.
fn copy_local_database(source: &Path, target: &Path) -> Result<()> {
    let connection =
        rusqlite::Connection::open_with_flags(source, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    connection.prepare("SELECT id, white, black, moves, start_fen, chess960 FROM games LIMIT 0")?;
    connection.prepare("SELECT hash, game_id, ply, next_san FROM positions LIMIT 0")?;
    connection.prepare("SELECT key, value FROM meta LIMIT 0")?;
    connection.execute("VACUUM INTO ?1", [target.to_string_lossy().as_ref()])?;
    Ok(())
}

/// Installs a complete collection atomically; errors and cancellation remove all staged work.
pub fn install(
    directory: &Path,
    source: Source,
    name: &str,
    description: &str,
    options: &ImportOptions,
    control: &ImportControl,
    progress: impl Fn(String),
) -> Result<PathBuf> {
    std::fs::create_dir_all(directory)?;
    let staged = tempfile::NamedTempFile::new_in(directory)?;
    let local_pgn = match &source {
        Source::Local(path)
            if path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("pgn")) =>
        {
            Some(path.clone())
        }
        _ => None,
    };
    let downloaded;
    let (file, filename) = match source {
        Source::Local(path) => {
            let name = path.to_string_lossy().into_owned();
            (path, name)
        }
        Source::Web(url) => {
            downloaded = download(&url, control, &progress)?;
            (downloaded.path().to_owned(), url)
        }
        Source::Twic => {
            let agent: ureq::Agent = ureq::Agent::config_builder()
                .https_only(true)
                .timeout_global(Some(Duration::from_secs(30)))
                .build()
                .into();
            let html = agent
                .get("https://theweekinchess.com/twic")
                .call()?
                .body_mut()
                .read_to_string()?;
            let url = latest_twic(&html)?;
            downloaded = download(&url, control, &progress)?;
            (downloaded.path().to_owned(), url)
        }
    };
    ensure!(!control.is_cancelled(), "Cancelled");
    if filename.to_ascii_lowercase().ends_with(".sqlite") {
        copy_local_database(&file, staged.path())?;
    } else {
        import_path(&file, &filename, staged.path(), options, control, &progress)?;
    }
    {
        let mut database = Database::open(staged.path())?;
        ensure!(database.game_count()? > 0, "The file contains no games");
        database.set_details(
            name,
            if description.is_empty() && filename.starts_with("https://") {
                &filename
            } else {
                description
            },
        )?;
        if let Some(path) = &local_pgn {
            database.set_source_file(path)?;
        }
        let busy: i64 = db::open_sqlite(staged.path())?.query_row(
            "PRAGMA wal_checkpoint(TRUNCATE)",
            [],
            |row| row.get(0),
        )?;
        ensure!(busy == 0, "Database is busy");
    }
    ensure!(!control.is_cancelled(), "Cancelled");
    for number in 1_u64.. {
        let path = directory.join(format!("collection-{number}.sqlite"));
        if path.exists() {
            continue;
        }
        staged.persist_noclobber(&path)?;
        return Ok(path);
    }
    unreachable!("an unused collection name")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Archive links are chosen by issue number, independent of page order.
    #[test]
    fn finds_latest_twic_pgn() {
        assert_eq!(
            latest_twic("twic99g.zip twic100g.zip twic101c.zip").unwrap(),
            "https://theweekinchess.com/zips/twic100g.zip"
        );
        assert!(latest_twic("no downloads").is_err());
    }

    /// Compressed imports keep PGN annotations and never publish invalid or cancelled collections.
    #[test]
    fn imports_archives_and_rejects_invalid_sources() {
        let dir = tempfile::tempdir().unwrap();
        let pgn = b"[Result \"1-0\"]\n\n1. e4 {A note} e5 1-0";
        let zip_path = dir.path().join("games.zip");
        let mut zip = zip::ZipWriter::new(File::create(&zip_path).unwrap());
        zip.start_file("nested/games.pgn", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(pgn).unwrap();
        zip.finish().unwrap();
        let gzip_path = dir.path().join("games.pgn.gz");
        let mut gzip = flate2::write::GzEncoder::new(
            File::create(&gzip_path).unwrap(),
            flate2::Compression::default(),
        );
        gzip.write_all(pgn).unwrap();
        gzip.finish().unwrap();
        let target = dir.path().join("databases");
        let options = ImportOptions {
            keep_annotations: true,
            ..Default::default()
        };
        let control = ImportControl::default();
        for source in [zip_path, gzip_path] {
            let path = install(
                &target,
                Source::Local(source),
                "Games",
                "",
                &options,
                &control,
                |_| {},
            )
            .unwrap();
            let database = Database::open(&path).unwrap();
            assert_eq!(database.game_count().unwrap(), 1);
            assert!(
                database
                    .game(1)
                    .unwrap()
                    .unwrap()
                    .to_pgn()
                    .contains("A note")
            );
        }
        let invalid = dir.path().join("bad.sqlite");
        std::fs::write(&invalid, b"invalid database").unwrap();
        assert!(
            install(
                &target,
                Source::Local(invalid),
                "Bad",
                "",
                &options,
                &control,
                |_| {}
            )
            .is_err()
        );
        assert_eq!(
            std::fs::read_dir(&target)
                .unwrap()
                .filter_map(|entry| entry.ok())
                .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "sqlite"))
                .count(),
            2
        );
    }

    /// Native copying reads the source without modifying it, and failed imports stay hidden.
    #[test]
    fn local_installs_are_atomic_and_preserve_the_source() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.pgn");
        std::fs::write(&source, "[Result \"1-0\"]\n\n1. e4 e5 1-0").unwrap();
        let options = ImportOptions::default();
        let control = ImportControl::default();
        let directory = dir.path().join("databases");
        let installed = install(
            &directory,
            Source::Local(source.clone()),
            "Games",
            "Notes",
            &options,
            &control,
            |_| {},
        )
        .unwrap();
        let copy = install(
            &directory,
            Source::Local(installed.clone()),
            "Copy",
            "",
            &options,
            &control,
            |_| {},
        )
        .unwrap();
        assert_eq!(Database::open(&copy).unwrap().game_count().unwrap(), 1);
        assert_eq!(Database::open(&installed).unwrap().name().unwrap(), "Games");
        assert_eq!(
            Database::open(&installed).unwrap().description().unwrap(),
            "Notes"
        );
        control.cancel();
        assert!(
            install(
                &directory,
                Source::Local(source),
                "Cancelled",
                "",
                &options,
                &control,
                |_| {}
            )
            .is_err()
        );
        assert_eq!(
            std::fs::read_dir(directory)
                .unwrap()
                .filter_map(|entry| entry.ok())
                .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "sqlite"))
                .count(),
            2
        );
    }
}
