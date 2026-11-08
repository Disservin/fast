# Release packages

Run the Release workflow from the Actions tab on GitHub. It reads
`workspace.package.version` from `Cargo.toml`, builds and packages Fast on all four
platforms and, once every job passes, creates the `vMAJOR.MINOR.PATCH` tag on the
chosen commit and publishes the GitHub release. It stops early if that tag or release
already exists. No release is published by running the packaging script locally.

| Download | Installation |
| --- | --- |
| Windows x86-64 setup `.exe` | Installs for the current user, adds a Start menu shortcut and supports uninstall. |
| Windows x86-64 `.zip` | Extract and run `Fast/fast.exe`. Settings still use the normal user profile. |
| macOS Apple Silicon or Intel `.dmg` | Open and drag Fast into Applications. The app is not notarized, so the first launch needs right-click › Open (or `xattr -dr com.apple.quarantine /Applications/Fast.app`). |
| Linux x86-64 `.AppImage` | Make executable, then run. Requires a compatible desktop, graphics drivers and FUSE, or `--appimage-extract-and-run`. |
| Linux x86-64 `.deb` | Install with `sudo apt install ./Fast-VERSION-linux-x86_64.deb`. Adds the launcher and installs declared dependencies. |

Linux packages are built on Ubuntu 22.04. Target Ubuntu 22.04 or newer; other
distributions need compatible system libraries and graphics drivers. AppImage bundles
libraries through linuxdeploy but still relies on the host graphics stack.
Engines are downloaded separately through Settings › Engines.
Uninstalling Fast leaves the user's settings, databases and downloaded engines intact.

## Updates from Settings

Settings › General › Fast updates shows the installed version from Cargo's
workspace version. Choose **Check for updates** to look for the latest published
stable GitHub release, then **Download and install update** to download the
matching package and verify its SHA-256 checksum before installation.
Drafts and prereleases are excluded. There are no automatic startup requests.
Until the first release is published, checking reports that the request could
not be completed; release tags alone do not provide downloadable updates.

AppImage installations replace their own file atomically and keep the previous
version alongside it as `Fast.AppImage.previous` (using the installed filename).
The directory must be writable. Close and reopen Fast to run the new version.
Windows opens the per-user setup program, macOS opens the DMG, and other Linux
installations open the Debian package with the desktop's package installer.
Finish the platform's installation flow, then close and reopen Fast. A portable
Windows download also uses the normal installer when updating from Settings.
Linux installations outside Debian/Ubuntu should use their distribution's
installation method instead. Settings, databases and engines remain in the
existing user profile throughout the update.

For a new release, change `workspace.package.version` in `Cargo.toml`, refresh
`Cargo.lock` with Cargo, push, then run the Release workflow. Installed copies will discover it
the next time the user checks for updates.

## Signing

Downloads are not signed with a certificate. Mac apps are ad-hoc signed so they
start on Apple Silicon, but they are not notarized; Windows users may see publisher
warnings.

## Local packaging and rehearsal

Build with `cargo build --locked --release -p fast`, then run
`python3 scripts/package_release.py`. Python 3.11 or newer is required.
The script writes to `dist/`, which must be empty before starting.

- Linux requires `dpkg-dev`, `desktop-file-utils` and
  [linuxdeploy](https://github.com/linuxdeploy/linuxdeploy). Set `LINUXDEPLOY` to its
  executable path if it is not on `PATH`. The workflow uses its continuous AppImage
  release, so changes to that upstream tool should be considered when investigating
  differences between release builds.
- Windows requires [Inno Setup 6](https://jrsoftware.org/isinfo.php). Set `ISCC` to
  `ISCC.exe` if it is not on `PATH`.
- macOS requires Xcode command line tools.

Each job includes license notices and a checksum file. The release combines
these into `SHA256SUMS.txt`; verify downloads with `sha256sum --check SHA256SUMS.txt`
on Linux, or compare individual hashes with `shasum -a 256` on macOS or
`Get-FileHash -Algorithm SHA256` in PowerShell.

Before publishing, install on clean machines without Rust: check launching from
the desktop menu, opening every screen, downloading and starting an engine, and
upgrading/uninstalling without losing a test profile. Verify both Mac architectures
and Linux Wayland and X11 sessions. CI packaging success alone does not cover these
interactive checks.
