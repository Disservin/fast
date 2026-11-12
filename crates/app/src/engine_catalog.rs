//! Declarative, pinned engine downloads. All network and archive work runs off the UI thread.
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashSet},
    fs::{self, File},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

const MAX_DOWNLOAD: u64 = 256 * 1024 * 1024;
const MAX_UNPACKED: u64 = 512 * 1024 * 1024;
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Catalog {
    pub schema_version: u32,
    pub engines: Vec<Entry>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub homepage: String,
    pub source_url: String,
    pub license: String,
    pub license_url: String,
    pub protocol: String,
    #[serde(default)]
    pub defaults: BTreeMap<String, String>,
    pub downloads: Vec<Download>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Download {
    pub os: String,
    pub arch: String,
    pub url: String,
    pub sha256: String,
    pub format: Format,
    pub executable: String,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Format {
    Binary,
    Zip,
    TarGz,
    /// An uncompressed tar, as fastchess releases are.
    Tar,
}
impl Catalog {
    pub fn bundled() -> Result<Self> {
        Self::parse(include_str!("../../../config/engines.json"))
    }
    pub fn parse(text: &str) -> Result<Self> {
        let catalog: Self = serde_json::from_str(text).context("reading engine catalog")?;
        ensure!(
            catalog.schema_version == 1,
            "Unsupported engine catalog version"
        );
        let mut ids = HashSet::new();
        for engine in &catalog.engines {
            ensure!(
                token(&engine.id) && token(&engine.version),
                "Invalid engine id or version"
            );
            ensure!(ids.insert(&engine.id), "Duplicate engine id: {}", engine.id);
            ensure!(engine.protocol == "uci", "Only UCI engines are supported");
            for url in [&engine.homepage, &engine.source_url, &engine.license_url] {
                ensure!(url.starts_with("https://"), "Engine URLs must use HTTPS");
            }
            let mut platforms = HashSet::new();
            for asset in &engine.downloads {
                ensure!(
                    platforms.insert((&asset.os, &asset.arch)),
                    "Duplicate platform for {}",
                    engine.name
                );
                ensure!(
                    asset.url.starts_with("https://"),
                    "Download URLs must use HTTPS"
                );
                ensure!(
                    asset.sha256.len() == 64 && asset.sha256.bytes().all(|b| b.is_ascii_hexdigit()),
                    "Invalid SHA-256 for {}",
                    engine.name
                );
                safe_relative(&asset.executable)?;
                ensure!(
                    matches!(asset.os.as_str(), "linux" | "macos" | "windows"),
                    "Unknown OS"
                );
                ensure!(
                    matches!(asset.arch.as_str(), "x86_64" | "aarch64"),
                    "Unknown architecture"
                );
            }
        }
        Ok(catalog)
    }
}
impl Entry {
    pub fn native_download(&self) -> Option<&Download> {
        self.downloads
            .iter()
            .find(|d| d.os == std::env::consts::OS && d.arch == std::env::consts::ARCH)
    }
    pub fn install_dir(&self, data: &Path, asset: &Download) -> PathBuf {
        self.install_dir_in(&data.join("engines"), asset)
    }
    /// The version directory under `root`, which holds one directory per catalog id.
    pub fn install_dir_in(&self, root: &Path, asset: &Download) -> PathBuf {
        root.join(&self.id)
            .join(&self.version)
            .join(format!("{}-{}", asset.os, asset.arch))
    }
}
fn token(text: &str) -> bool {
    !text.is_empty()
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
        && text != "."
        && text != ".."
}
fn safe_relative(text: &str) -> Result<PathBuf> {
    ensure!(
        !text.is_empty() && !text.contains(['\\', ':', '\0']),
        "Invalid archive path: {text}"
    );
    let path = Path::new(text);
    ensure!(
        !path.is_absolute()
            && path
                .components()
                .all(|p| matches!(p, Component::Normal(_) | Component::CurDir)),
        "Unsafe archive path: {text}"
    );
    ensure!(
        path.components().any(|p| matches!(p, Component::Normal(_))),
        "Empty archive path"
    );
    Ok(path.to_owned())
}
fn cancelled(cancel: &AtomicBool) -> Result<()> {
    ensure!(!cancel.load(Ordering::Relaxed), "Download cancelled");
    Ok(())
}
fn copy_limited(
    mut from: impl Read,
    mut to: impl Write,
    limit: u64,
    cancel: &AtomicBool,
    mut progress: impl FnMut(u64),
) -> Result<u64> {
    let mut buffer = [0u8; 64 * 1024];
    let mut total = 0;
    loop {
        cancelled(cancel)?;
        let count = from.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        total += count as u64;
        ensure!(total <= limit, "Download or archive exceeds size limit");
        to.write_all(&buffer[..count])?;
        progress(total);
    }
    Ok(total)
}
fn download(
    agent: &ureq::Agent,
    url: &str,
    path: &Path,
    limit: u64,
    cancel: &AtomicBool,
    progress: impl FnMut(u64),
) -> Result<()> {
    cancelled(cancel)?;
    let mut response = agent
        .get(url)
        .call()
        .with_context(|| format!("Downloading {url}"))?;
    if let Some(size) = response.body().content_length() {
        ensure!(size <= limit, "Download exceeds size limit");
    }
    let mut file = File::create(path)?;
    copy_limited(
        response.body_mut().as_reader(),
        &mut file,
        limit,
        cancel,
        progress,
    )?;
    file.sync_all()?;
    Ok(())
}
fn verify(path: &Path, expected: &str) -> Result<()> {
    let mut source = File::open(path)?;
    let mut hash = Sha256::new();
    std::io::copy(&mut source, &mut hash)?;
    ensure!(
        format!("{:x}", hash.finalize()).eq_ignore_ascii_case(expected),
        "SHA-256 mismatch: download was not installed"
    );
    Ok(())
}
/// Writes one archive member of `size` bytes to `path`, keeping the whole archive under
/// [`MAX_UNPACKED`] when `total` bytes are already out. Returns the bytes written.
fn unpack_file(
    entry: impl Read,
    size: u64,
    path: &Path,
    total: u64,
    cancel: &AtomicBool,
) -> Result<u64> {
    ensure!(size <= MAX_UNPACKED - total, "Archive exceeds size limit");
    fs::create_dir_all(path.parent().context("archive directory")?)?;
    copy_limited(
        entry,
        File::create(path)?,
        MAX_UNPACKED - total,
        cancel,
        |_| {},
    )
}

fn unpack(asset: &Download, archive: &Path, destination: &Path, cancel: &AtomicBool) -> Result<()> {
    let mut total = 0u64;
    match asset.format {
        Format::Binary => {
            let path = destination.join(safe_relative(&asset.executable)?);
            fs::create_dir_all(path.parent().context("executable directory")?)?;
            copy_limited(
                File::open(archive)?,
                File::create(path)?,
                MAX_UNPACKED,
                cancel,
                |_| {},
            )?;
        }
        Format::Zip => {
            let mut archive = zip::ZipArchive::new(File::open(archive)?)?;
            ensure!(archive.len() <= 8192, "Too many files in archive");
            for index in 0..archive.len() {
                cancelled(cancel)?;
                let mut entry = archive.by_index(index)?;
                let path = destination.join(safe_relative(entry.name()?.trim_end_matches('/'))?);
                ensure!(!entry.is_symlink(), "Archive contains a symbolic link");
                if entry.is_dir() {
                    fs::create_dir_all(path)?;
                    continue;
                }
                let size = entry.size();
                total += unpack_file(&mut entry, size, &path, total, cancel)?;
            }
        }
        Format::TarGz | Format::Tar => {
            let file = File::open(archive)?;
            let reader: Box<dyn Read> = match asset.format {
                Format::TarGz => Box::new(flate2::read::GzDecoder::new(file)),
                _ => Box::new(file),
            };
            let mut archive = tar::Archive::new(reader);
            for (index, entry) in archive.entries()?.enumerate() {
                cancelled(cancel)?;
                ensure!(index < 8192, "Too many files in archive");
                let mut entry = entry?;
                let relative = entry.path()?.to_string_lossy().into_owned();
                let path = destination.join(safe_relative(relative.trim_end_matches('/'))?);
                let kind = entry.header().entry_type();
                if kind.is_dir() {
                    fs::create_dir_all(path)?;
                    continue;
                }
                ensure!(kind.is_file(), "Archive contains a link or special file");
                let size = entry.size();
                total += unpack_file(&mut entry, size, &path, total, cancel)?;
            }
        }
    }
    Ok(())
}
/// Download into staging, verify before extraction, and publish a complete version atomically.
/// The caller performs a UCI handshake before adding it to the configured-engine list.
pub fn install(
    entry: &Entry,
    data: &Path,
    cancel: Arc<AtomicBool>,
    progress: impl Fn(String),
) -> Result<PathBuf> {
    install_into(entry, &data.join("engines"), cancel, progress)
}
/// [`install`] into another root, for downloaded tools that are not engines.
pub fn install_into(
    entry: &Entry,
    root: &Path,
    cancel: Arc<AtomicBool>,
    progress: impl Fn(String),
) -> Result<PathBuf> {
    let asset = entry
        .native_download()
        .context("No build is available for this operating system and CPU")?;
    let target = entry.install_dir_in(root, asset);
    let executable = target.join(safe_relative(&asset.executable)?);
    if target.exists() {
        let receipt = fs::read(target.join("fast-install.json"))
            .context("Incomplete previous installation; remove its version directory and retry")?;
        let installed: Entry = serde_json::from_slice(&receipt)?;
        ensure!(
            installed
                .native_download()
                .is_some_and(|d| d.sha256 == asset.sha256)
                && executable.is_file(),
            "Installed files do not match this catalog version"
        );
        return Ok(executable);
    }
    let parent = target.parent().context("installation directory")?;
    fs::create_dir_all(parent)?;
    let stage = tempfile::tempdir_in(parent)?;
    let archive = stage.path().join("download");
    let payload = stage.path().join("payload");
    fs::create_dir(&payload)?;
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .https_only(true)
        .timeout_connect(Some(Duration::from_secs(20)))
        .timeout_recv_body(Some(Duration::from_secs(30)))
        .timeout_global(Some(Duration::from_secs(600)))
        .build()
        .into();
    let mut last_mb = 0;
    progress(format!("Downloading {} {}…", entry.name, entry.version));
    let what = if entry.protocol == "uci" {
        "engine"
    } else {
        "tool"
    };
    download(
        &agent,
        &asset.url,
        &archive,
        MAX_DOWNLOAD,
        &cancel,
        |bytes| {
            let mb = bytes / (1024 * 1024);
            if mb > last_mb {
                last_mb = mb;
                progress(format!("Downloading {}: {mb} MiB", entry.name));
            }
        },
    )?;
    progress("Verifying SHA-256…".into());
    verify(&archive, &asset.sha256)?;
    progress(format!("Extracting {what} and supporting files…"));
    unpack(asset, &archive, &payload, &cancel)?;
    let staged_executable = payload.join(&asset.executable);
    ensure!(
        staged_executable.is_file(),
        "The catalog executable was not found in the archive"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&staged_executable, fs::Permissions::from_mode(0o755))?;
    }
    progress("Saving license and source information…".into());
    download(
        &agent,
        &entry.license_url,
        &payload.join("ENGINE-LICENSE.txt"),
        1024 * 1024,
        &cancel,
        |_| {},
    )?;
    fs::write(
        payload.join("SOURCE.txt"),
        format!(
            "{} {}\nLicense: {}\nCorresponding source: {}\nHomepage: {}\n",
            entry.name, entry.version, entry.license, entry.source_url, entry.homepage
        ),
    )?;
    fs::write(
        payload.join("fast-install.json"),
        serde_json::to_vec_pretty(entry)?,
    )?;
    cancelled(&cancel)?;
    fs::rename(&payload, &target).with_context(|| format!("Publishing {what} installation"))?;
    Ok(executable)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn cancel() -> AtomicBool {
        AtomicBool::new(false)
    }
    #[test]
    fn catalog_has_valid_unique_platforms_and_complete_provenance() {
        let catalog = Catalog::bundled().unwrap();
        assert!(catalog.engines.len() >= 2);
        for engine in &catalog.engines {
            assert!(
                !engine.name.is_empty()
                    && !engine.description.is_empty()
                    && !engine.license.is_empty()
            );
            assert!(!engine.downloads.is_empty());
        }
    }
    #[test]
    fn rejects_insecure_downloads_and_unsafe_paths() {
        let catalog = Catalog::bundled().unwrap();
        for path in [
            "../engine",
            "/engine",
            "C:/engine.exe",
            "a\\..\\engine",
            "..",
            ".",
        ] {
            assert!(safe_relative(path).is_err(), "{path}");
        }
        let mut bad = catalog.clone();
        bad.engines[0].downloads[0].url = "http://example.com/engine".into();
        assert!(Catalog::parse(&serde_json::to_string(&bad).unwrap()).is_err());
        let mut bad = catalog.clone();
        bad.engines[0].downloads[0].sha256 = "unknown".into();
        assert!(Catalog::parse(&serde_json::to_string(&bad).unwrap()).is_err());
        let mut bad = catalog.clone();
        bad.engines.push(catalog.engines[0].clone());
        assert!(Catalog::parse(&serde_json::to_string(&bad).unwrap()).is_err());
    }
    #[test]
    fn checksum_and_cancellation_prevent_untrusted_or_partial_installation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("binary");
        fs::write(&path, b"abc").unwrap();
        verify(
            &path,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        )
        .unwrap();
        assert!(verify(&path, &"0".repeat(64)).is_err());
        let mut destination = Vec::new();
        assert!(copy_limited(&b"abc"[..], &mut destination, 2, &cancel(), |_| {}).is_err());
        assert!(destination.is_empty());
        assert!(
            copy_limited(
                &b"abc"[..],
                &mut destination,
                10,
                &AtomicBool::new(true),
                |_| {}
            )
            .is_err()
        );
        assert!(destination.is_empty());
    }
    #[test]
    fn zip_slip_is_rejected_and_support_files_are_preserved() {
        use zip::write::SimpleFileOptions;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("engine.zip");
        let mut zip = zip::ZipWriter::new(File::create(&path).unwrap());
        zip.start_file("bin/engine", SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"engine").unwrap();
        zip.start_file("bin/network.nnue", SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"network").unwrap();
        zip.finish().unwrap();
        let mut asset = Catalog::bundled().unwrap().engines[0].downloads[0].clone();
        asset.format = Format::Zip;
        let output = dir.path().join("output");
        unpack(&asset, &path, &output, &cancel()).unwrap();
        assert_eq!(
            fs::read(output.join("bin/network.nnue")).unwrap(),
            b"network"
        );
        let mut zip = zip::ZipWriter::new(File::create(&path).unwrap());
        zip.start_file("../escape", SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"bad").unwrap();
        zip.finish().unwrap();
        assert!(unpack(&asset, &path, &output, &cancel()).is_err());
        assert!(!dir.path().join("escape").exists());
    }
    #[test]
    fn tar_links_are_rejected_and_networks_preserved() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("engine.tar.gz");
        let encoder = flate2::write::GzEncoder::new(
            File::create(&path).unwrap(),
            flate2::Compression::default(),
        );
        let mut tar = tar::Builder::new(encoder);
        let mut header = tar::Header::new_gnu();
        header.set_size(7);
        header.set_mode(0o644);
        header.set_cksum();
        tar.append_data(&mut header, "bin/network.nnue", &b"network"[..])
            .unwrap();
        tar.into_inner().unwrap().finish().unwrap();
        let asset = Catalog::bundled().unwrap().engines[0].downloads[0].clone();
        let output = dir.path().join("output");
        unpack(&asset, &path, &output, &cancel()).unwrap();
        assert_eq!(
            fs::read(output.join("bin/network.nnue")).unwrap(),
            b"network"
        );
        let encoder = flate2::write::GzEncoder::new(
            File::create(&path).unwrap(),
            flate2::Compression::default(),
        );
        let mut tar = tar::Builder::new(encoder);
        let mut header = tar::Header::new_gnu();
        header.set_size(0);
        header.set_mode(0o777);
        header.set_entry_type(tar::EntryType::Symlink);
        header.set_link_name("/tmp/escape").unwrap();
        header.set_cksum();
        tar.append_data(&mut header, "link", std::io::empty())
            .unwrap();
        tar.into_inner().unwrap().finish().unwrap();
        assert!(unpack(&asset, &path, &output, &cancel()).is_err());
    }
    /// Check previously downloaded release archives without using the network.
    #[test]
    #[ignore = "requires FAST_ENGINE_ARCHIVES containing official release archives"]
    fn verify_downloaded_release_archives() {
        let catalog = Catalog::bundled().unwrap();
        let archives =
            PathBuf::from(std::env::var("FAST_ENGINE_ARCHIVES").expect("archive directory"));
        for engine in &catalog.engines {
            for asset in &engine.downloads {
                let filename = asset.url.rsplit('/').next().unwrap();
                let path = archives.join(filename);
                if !path.exists() {
                    continue;
                }
                verify(&path, &asset.sha256).unwrap();
                let dir = tempfile::tempdir().unwrap();
                unpack(asset, &path, dir.path(), &cancel()).unwrap();
                assert!(
                    dir.path().join(&asset.executable).is_file(),
                    "{}",
                    asset.executable
                );
            }
        }
    }
}
