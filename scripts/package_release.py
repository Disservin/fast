#!/usr/bin/env python3
"""Package the built Fast executable for the current operating system."""

import argparse
import hashlib
import os
from pathlib import Path
import platform
import plistlib
import re
import shutil
import subprocess
import tempfile
import tomllib
import zipfile

ROOT = Path(__file__).resolve().parents[1]
DIST = ROOT / "dist"
VERSION = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["package"]["version"]


def run(*args, **kwargs):
    """Run a packaging tool and stop if it cannot produce a valid package."""
    return subprocess.run([str(arg) for arg in args], check=True, **kwargs)


def licenses(destination):
    """Include Fast's license and the bundled dependency and asset notices."""
    destination.mkdir(parents=True, exist_ok=True)
    for source in ("LICENSE", "assets/LICENSES.md", "crates/app/third_party.json"):
        shutil.copy2(ROOT / source, destination / Path(source).name)


def package_linux(work):
    """Create a Debian package with runtime dependencies and a bundled AppImage."""
    name = f"Fast-{VERSION}-linux-x86_64"
    appdir = work / "Fast.AppDir"
    binary = appdir / "usr/bin/fast"
    binary.parent.mkdir(parents=True)
    shutil.copy2(ROOT / "target/release/fast", binary)
    desktop = appdir / "usr/share/applications/dev.fast.app.desktop"
    desktop.parent.mkdir(parents=True)
    shutil.copy2(ROOT / "assets/app/dev.fast.app.desktop", desktop)
    icon = appdir / "usr/share/icons/hicolor/256x256/apps/dev.fast.app.png"
    icon.parent.mkdir(parents=True)
    shutil.copy2(ROOT / "assets/app/fast.png", icon)
    licenses(appdir / "usr/share/doc/fast")

    # Compute dependencies before linuxdeploy bundles libraries into the AppDir.
    (work / "debian").mkdir()
    (work / "debian/control").write_text("Source: fast\n\nPackage: fast\nArchitecture: amd64\n")
    result = run("dpkg-shlibdeps", "--ignore-missing-info", "-O", "-e" + str(binary),
                 cwd=work, capture_output=True, text=True)
    dependencies = next(line.removeprefix("shlibs:Depends=")
                        for line in result.stdout.splitlines() if line.startswith("shlibs:Depends="))
    # Vulkan is loaded at runtime, so the linker dependency scan cannot see it.
    dependencies += ", libvulkan1"
    control = appdir / "DEBIAN/control"
    control.parent.mkdir()
    control.write_text(
        f"Package: fast\nVersion: {VERSION}\nArchitecture: amd64\n"
        "Maintainer: Fast contributors\nSection: games\nPriority: optional\n"
        f"Depends: {dependencies}\n"
        "Description: Desktop chess workstation\n"
        " Analysis, databases, opening training and engine play.\n")
    run("dpkg-deb", "--root-owner-group", "--build", appdir, DIST / f"{name}.deb")
    shutil.rmtree(control.parent)
    run(os.environ.get("LINUXDEPLOY", "linuxdeploy"), "--appdir", appdir,
        "--executable", binary, "--desktop-file", desktop, "--icon-file", icon,
        "--output", "appimage", cwd=work,
        env={**os.environ, "ARCH": "x86_64", "APPIMAGE_EXTRACT_AND_RUN": "1",
             "OUTPUT": str(DIST / f"{name}.AppImage"), "VERSION": VERSION})


def package_windows(work):
    """Create a portable ZIP and a per-user Windows installer."""
    portable = work / "Fast"
    portable.mkdir()
    shutil.copy2(ROOT / "target/release/fast.exe", portable)
    licenses(portable / "licenses")
    with zipfile.ZipFile(DIST / f"Fast-{VERSION}-windows-x86_64.zip", "w",
                         zipfile.ZIP_DEFLATED) as archive:
        for source in sorted(portable.rglob("*")):
            if source.is_file():
                archive.write(source, source.relative_to(work))
    run(os.environ.get("ISCC", "iscc"), f"/DAppVersion={VERSION}", ROOT / "packaging/windows.iss")


def package_macos(work):
    """Create an ad-hoc signed Mac app and DMG."""
    arch = "aarch64" if platform.machine() == "arm64" else "x86_64"
    app = work / "image/Fast.app"
    contents = app / "Contents"
    (contents / "MacOS").mkdir(parents=True)
    resources = contents / "Resources"
    resources.mkdir()
    shutil.copy2(ROOT / "target/release/fast", contents / "MacOS/fast")
    licenses(resources / "licenses")
    iconset = work / "Fast.iconset"
    iconset.mkdir()
    for size in (16, 32, 128, 256, 512):
        for scale in (1, 2):
            suffix = "@2x" if scale == 2 else ""
            run("sips", "-z", size * scale, size * scale, ROOT / "assets/app/fast.png",
                "--out", iconset / f"icon_{size}x{size}{suffix}.png", stdout=subprocess.DEVNULL)
    run("iconutil", "-c", "icns", iconset, "-o", resources / "Fast.icns")
    with (contents / "Info.plist").open("wb") as output:
        plistlib.dump({"CFBundleName": "Fast", "CFBundleDisplayName": "Fast",
                      "CFBundleIdentifier": "dev.fast.app", "CFBundleExecutable": "fast",
                      "CFBundlePackageType": "APPL", "CFBundleIconFile": "Fast.icns",
                      "CFBundleShortVersionString": VERSION, "CFBundleVersion": VERSION,
                      "NSHighResolutionCapable": True, "LSMinimumSystemVersion": "12.0"}, output)
    # Ad-hoc signing lets the app start on Apple Silicon; it is not notarized.
    run("codesign", "--force", "--sign", "-", app)
    run("codesign", "--verify", "--deep", "--strict", app)
    (work / "image/Applications").symlink_to("/Applications")
    dmg = DIST / f"Fast-{VERSION}-macos-{arch}.dmg"
    run("hdiutil", "create", "-volname", "Fast", "-srcfolder", work / "image",
        "-ov", "-format", "UDZO", dmg)


def main():
    """Validate the release version and write packages and SHA-256 checksums."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.parse_args()
    if not re.fullmatch(r"\d+\.\d+\.\d+", VERSION):
        parser.error("release versions must use major.minor.patch")
    DIST.mkdir(exist_ok=True)
    if any(DIST.iterdir()):
        parser.error("dist must be empty; move previous packages before packaging again")
    with tempfile.TemporaryDirectory(prefix="fast-package-") as temporary:
        work = Path(temporary)
        system = platform.system()
        if system == "Linux":
            package_linux(work)
        elif system == "Windows":
            package_windows(work)
        elif system == "Darwin":
            package_macos(work)
        else:
            parser.error(f"unsupported platform: {system}")
    checksums = []
    for artifact in sorted(DIST.iterdir()):
        with artifact.open("rb") as source:
            digest = hashlib.file_digest(source, "sha256").hexdigest()
        checksums.append(f"{digest}  {artifact.name}\n")
    (DIST / f"SHA256SUMS-{platform.system()}-{platform.machine()}.txt").write_text("".join(checksums))


if __name__ == "__main__":
    main()
