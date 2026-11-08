//! User-requested application updates from Fast's published GitHub releases.

use anyhow::{Context, Result, bail, ensure};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{fs, io::Read, path::PathBuf, time::Duration};

/// The installed application version, shared with release packaging.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
const RELEASES: &str = "https://api.github.com/repos/Disservin/fast/releases/latest";
const DOWNLOADS: &str = "https://github.com/Disservin/fast/releases/download/";

/// A published update with the exact package and checksum for this installation.
#[derive(Clone, Debug)]
pub struct Update {
    pub version: String,
    asset: Asset,
    checksums: Asset,
    appimage: Option<PathBuf>,
}

/// Public GitHub release metadata used to select stable application updates.
#[derive(Deserialize)]
struct Release {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    assets: Vec<Asset>,
}

/// A named download belonging to the selected GitHub release.
#[derive(Clone, Debug, Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}

/// Read a strict release version so malformed or prerelease tags cannot be installed.
fn version(value: &str) -> Result<[u64; 3]> {
    let parts = value.split('.').collect::<Vec<_>>();
    ensure!(parts.len() == 3, "Invalid release version");
    let mut numbers = [0; 3];
    for (number, part) in numbers.iter_mut().zip(parts) {
        ensure!(
            !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()),
            "Invalid release version"
        );
        *number = part.parse()?;
    }
    Ok(numbers)
}

/// Build a bounded HTTPS client for background update checks and downloads.
fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .https_only(true)
        .timeout_connect(Some(Duration::from_secs(20)))
        .timeout_recv_body(Some(Duration::from_secs(30)))
        .timeout_global(Some(Duration::from_secs(600)))
        .build()
        .into()
}

/// Read a bounded response without allowing oversized release metadata into memory.
fn read_url(agent: &ureq::Agent, url: &str, limit: u64) -> Result<Vec<u8>> {
    let mut response = agent
        .get(url)
        .header("User-Agent", concat!("Fast/", env!("CARGO_PKG_VERSION")))
        .call()?;
    let mut bytes = Vec::new();
    response
        .body_mut()
        .as_reader()
        .take(limit + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= limit,
        "Update response exceeds size limit"
    );
    Ok(bytes)
}

/// Select the package that can update the current platform and installation format.
fn package_name(release: &str, os: &str, arch: &str, appimage: bool) -> Result<String> {
    let suffix = match (os, arch, appimage) {
        ("windows", "x86_64", _) => "windows-x86_64-setup.exe",
        ("macos", "aarch64", _) => "macos-aarch64.dmg",
        ("macos", "x86_64", _) => "macos-x86_64.dmg",
        ("linux", "x86_64", true) => "linux-x86_64.AppImage",
        ("linux", "x86_64", false) => "linux-x86_64.deb",
        _ => bail!("No update package is available for this platform"),
    };
    Ok(format!("Fast-{release}-{suffix}"))
}

/// Require the asset URL to belong to the selected official release.
fn validate_asset(asset: &Asset, tag: &str) -> Result<()> {
    ensure!(
        asset.browser_download_url == format!("{DOWNLOADS}{tag}/{}", asset.name),
        "Unexpected update download URL"
    );
    Ok(())
}

/// Check the latest public stable release without changing the installation.
pub fn check() -> Result<Option<Update>> {
    let bytes = read_url(&agent(), RELEASES, 2 * 1024 * 1024)
        .context("Checking published Fast releases")?;
    let release: Release = serde_json::from_slice(&bytes)?;
    ensure!(
        !release.draft && !release.prerelease,
        "Release is not stable"
    );
    let latest = release
        .tag_name
        .strip_prefix('v')
        .context("Invalid release tag")?;
    if version(latest)? <= version(VERSION)? {
        return Ok(None);
    }
    let appimage = std::env::var_os("APPIMAGE").map(PathBuf::from);
    let name = package_name(
        latest,
        std::env::consts::OS,
        std::env::consts::ARCH,
        appimage.is_some(),
    )?;
    let find = |name: &str| -> Result<Asset> {
        let asset = release
            .assets
            .iter()
            .find(|asset| asset.name == name)
            .with_context(|| format!("Release is missing {name}"))?
            .clone();
        validate_asset(&asset, &release.tag_name)?;
        Ok(asset)
    };
    Ok(Some(Update {
        version: latest.to_owned(),
        asset: find(&name)?,
        checksums: find("SHA256SUMS.txt")?,
        appimage,
    }))
}

/// Find an unambiguous SHA-256 checksum for the exact selected download.
fn checksum(text: &str, name: &str) -> Result<String> {
    let matches = text
        .lines()
        .filter_map(|line| line.split_once("  "))
        .filter(|(_, file)| *file == name)
        .collect::<Vec<_>>();
    ensure!(matches.len() == 1, "Missing or duplicate update checksum");
    let digest = matches[0].0;
    ensure!(
        digest.len() == 64 && digest.bytes().all(|b| b.is_ascii_hexdigit()),
        "Invalid update checksum"
    );
    Ok(digest.to_ascii_lowercase())
}

/// Download and verify a package, then replace the AppImage or open its native installer.
pub fn install(update: &Update) -> Result<bool> {
    let agent = agent();
    let sums = String::from_utf8(read_url(
        &agent,
        &update.checksums.browser_download_url,
        1024 * 1024,
    )?)?;
    let expected = checksum(&sums, &update.asset.name)?;
    let directory = dirs::cache_dir()
        .context("Cannot find the download directory")?
        .join("fast/updates");
    fs::create_dir_all(&directory)?;
    let mut stage = tempfile::NamedTempFile::new_in(&directory)?;
    let mut response = agent.get(&update.asset.browser_download_url).call()?;
    let limit = 512 * 1024 * 1024;
    let bytes = std::io::copy(
        &mut response.body_mut().as_reader().take(limit + 1),
        &mut stage,
    )?;
    ensure!(bytes <= limit, "Update exceeds the download size limit");
    stage.as_file_mut().sync_all()?;
    let mut hasher = Sha256::new();
    let mut downloaded = fs::File::open(stage.path())?;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = downloaded.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    let digest = format!("{:x}", hasher.finalize());
    ensure!(
        digest == expected,
        "Update checksum mismatch; the download was not installed"
    );
    if let Some(image) = &update.appimage {
        replace_appimage(stage.path(), image)?;
        return Ok(true);
    }
    let destination = directory.join(&update.asset.name);
    stage
        .persist(&destination)
        .context("Saving the verified update")?;
    open_installer(&destination)?;
    Ok(false)
}

/// Atomically replace an AppImage and retain a backup beside it for recovery.
#[cfg(unix)]
fn replace_appimage(source: &std::path::Path, destination: &std::path::Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let destination = destination.canonicalize()?;
    let parent = destination
        .parent()
        .context("Cannot find the AppImage directory")?;
    let mut stage = tempfile::NamedTempFile::new_in(parent)?;
    std::io::copy(&mut fs::File::open(source)?, &mut stage)?;
    stage
        .as_file()
        .set_permissions(fs::Permissions::from_mode(0o755))?;
    stage.as_file().sync_all()?;
    let mut backup = destination.as_os_str().to_os_string();
    backup.push(".previous");
    fs::copy(&destination, PathBuf::from(backup)).context("Backing up the installed AppImage")?;
    stage
        .persist(&destination)
        .context("Replacing the AppImage")?;
    Ok(())
}

/// Report unsupported AppImage replacement on Windows.
#[cfg(not(unix))]
fn replace_appimage(_: &std::path::Path, _: &std::path::Path) -> Result<()> {
    bail!("AppImage updates require Linux")
}

/// Open the verified package through the platform installer without a shell command.
fn open_installer(path: &std::path::Path) -> Result<()> {
    #[cfg(target_os = "windows")]
    let mut command = std::process::Command::new(path);
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = std::process::Command::new("open");
        command.arg(path);
        command
    };
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let mut command = {
        let mut command = std::process::Command::new("xdg-open");
        command.arg(path);
        command
    };
    command.spawn().context("Opening the update installer")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Compare numeric versions and reject tags outside the stable release format.
    #[test]
    fn versions_are_numeric_and_stable() {
        assert!(version("0.10.0").unwrap() > version("0.9.9").unwrap());
        for invalid in ["1.0", "1.0.0-beta", "v1.0.0", "1.0.0/../../bad"] {
            assert!(version(invalid).is_err());
        }
    }

    /// Match the artifact names emitted by the release packaging script.
    #[test]
    fn select_platform_packages() {
        for (os, arch, image, suffix) in [
            ("linux", "x86_64", true, "linux-x86_64.AppImage"),
            ("linux", "x86_64", false, "linux-x86_64.deb"),
            ("windows", "x86_64", false, "windows-x86_64-setup.exe"),
            ("macos", "aarch64", false, "macos-aarch64.dmg"),
            ("macos", "x86_64", false, "macos-x86_64.dmg"),
        ] {
            assert_eq!(
                package_name("1.2.3", os, arch, image).unwrap(),
                format!("Fast-1.2.3-{suffix}")
            );
        }
        assert!(package_name("1.2.3", "linux", "aarch64", false).is_err());
    }

    /// Reject ambiguous checksums and assets outside the official release.
    #[test]
    fn checksums_and_urls_are_exact() {
        let hash = "a".repeat(64);
        let line = format!("{hash}  Fast.exe\n");
        assert_eq!(checksum(&line, "Fast.exe").unwrap(), hash);
        assert!(checksum(&(line.clone() + &line), "Fast.exe").is_err());
        assert!(checksum(&line, "other.exe").is_err());
        let mut asset = Asset {
            name: "Fast.exe".into(),
            browser_download_url: format!("{DOWNLOADS}v1.2.3/Fast.exe"),
        };
        validate_asset(&asset, "v1.2.3").unwrap();
        asset.browser_download_url.push_str("?redirect=bad");
        assert!(validate_asset(&asset, "v1.2.3").is_err());
    }

    /// Keep the old AppImage available after an atomic executable replacement.
    #[cfg(unix)]
    #[test]
    fn appimage_replacement_keeps_backup() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join("Fast.AppImage");
        let new = dir.path().join("download");
        fs::write(&old, b"old").unwrap();
        fs::write(&new, b"new").unwrap();
        replace_appimage(&new, &old).unwrap();
        assert_eq!(fs::read(&old).unwrap(), b"new");
        assert_eq!(
            fs::read(dir.path().join("Fast.AppImage.previous")).unwrap(),
            b"old"
        );
        assert_eq!(
            fs::metadata(old).unwrap().permissions().mode() & 0o777,
            0o755
        );
    }
}
