//! Stockfish's public opening-book collection, downloaded and cached on demand.
use anyhow::{Result, ensure};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};

/// Recommended book first; remaining entries from official-stockfish/books.
pub const BOOKS: &[&str] = &[
    "UHO_Lichess_4852_v1.epd",
    "2moves_v1.epd",
    "2moves_v2.pgn",
    "3moves_FRC.epd",
    "4mvs_+90_+99.epd",
    "6mvs_+90_+99.epd",
    "8moves_v3.pgn",
    "8mvs_+90_+99.epd",
    "8mvs_big_+80_+109.epd",
    "DFRC_4852_v1.epd",
    "DFRC_openings.epd",
    "Drawkiller_balanced_big.epd",
    "FRC_openings.epd",
    "UHO_4060_v1.epd",
    "UHO_4060_v2.epd",
    "UHO_4060_v3.epd",
    "UHO_4060_v4.epd",
    "UHO_MEGA_2022_+110_+149.pgn",
    "UHO_XXL_+0.80_+1.09.pgn",
    "UHO_XXL_+0.90_+1.19.pgn",
    "UHO_XXL_+1.00_+1.29.pgn",
    "UHO_XXL_2022_+100_+129.pgn",
    "UHO_XXL_2022_+110_+139.pgn",
    "UHO_XXL_2022_+120_+149.pgn",
    "bjbraams_chessdb_198350_lines.pgn",
    "closedpos.epd",
    "endgames.epd",
    "endgames_cdb95105.epd",
    "hybrid_book_beta.epd",
    "noob_2moves.epd",
    "noob_3moves.epd",
    "noob_4moves.epd",
    "noob_5moves.epd",
    "popularpos_lichess.epd",
    "popularpos_lichess_v2.epd",
    "popularpos_lichess_v3.epd",
    "stalemates_200d30_v1.epd",
    "startpos.epd",
    "transpositions_cdb95105.epd",
];

pub fn path(data: &Path, name: &str) -> PathBuf {
    data.join("books").join(name)
}

pub fn install(data: &Path, name: &str) -> Result<()> {
    ensure!(BOOKS.contains(&name), "Unknown Stockfish book");
    let destination = path(data, name);
    if destination.is_file() {
        return Ok(());
    }
    let directory = destination.parent().unwrap();
    std::fs::create_dir_all(directory)?;
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(300)))
        .build()
        .into();
    let url =
        format!("https://raw.githubusercontent.com/official-stockfish/books/master/{name}.zip");
    let mut response = agent.get(&url).call()?;
    let mut archive_file = tempfile::tempfile_in(directory)?;
    const LIMIT: u64 = 512 * 1024 * 1024;
    let copied = std::io::copy(
        &mut response.body_mut().as_reader().take(LIMIT + 1),
        &mut archive_file,
    )?;
    ensure!(copied <= LIMIT, "Book archive exceeds download limit");
    extract(archive_file, &destination, name)
}

fn extract(archive_file: std::fs::File, destination: &Path, name: &str) -> Result<()> {
    const LIMIT: u64 = 512 * 1024 * 1024;
    let directory = destination.parent().unwrap();
    let mut archive = zip::ZipArchive::new(archive_file)?;
    let mut entry = archive.by_name(name)?;
    ensure!(
        entry.size() > 0 && entry.size() <= LIMIT,
        "Invalid book size"
    );
    let mut output = tempfile::NamedTempFile::new_in(directory)?;
    let copied = std::io::copy(&mut entry.by_ref().take(LIMIT + 1), &mut output)?;
    ensure!(copied > 0 && copied <= LIMIT, "Invalid extracted book size");
    output.flush()?;
    output.as_file().sync_all()?;
    output.persist(&destination)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn archive(name: &str, contents: &[u8]) -> std::fs::File {
        let mut zip = zip::ZipWriter::new(tempfile::tempfile().unwrap());
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(contents).unwrap();
        zip.finish().unwrap()
    }

    #[test]
    fn extracts_only_the_requested_book_and_reuses_cache() {
        let data = tempfile::tempdir().unwrap();
        let name = BOOKS[0];
        let destination = path(data.path(), name);
        std::fs::create_dir_all(destination.parent().unwrap()).unwrap();
        extract(archive(name, b"position\n"), &destination, name).unwrap();
        assert_eq!(std::fs::read(&destination).unwrap(), b"position\n");
        install(data.path(), name).unwrap(); // Must not require a network request.
        assert!(extract(archive("../other.epd", b"wrong"), &destination, name).is_err());
        assert_eq!(std::fs::read(&destination).unwrap(), b"position\n");
    }

    #[test]
    fn rejects_empty_books_without_caching_them() {
        let data = tempfile::tempdir().unwrap();
        let destination = data.path().join(BOOKS[0]);
        assert!(extract(archive(BOOKS[0], b""), &destination, BOOKS[0]).is_err());
        assert!(!destination.exists());
        assert!(install(data.path(), "../unknown.epd").is_err());
    }
}
