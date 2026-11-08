# Build & contribute

For how the code is organised, see
[ARCHITECTURE.md](https://github.com/Disservin/fast/blob/master/ARCHITECTURE.md).

## Checks

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
python3 scripts/third_party.py   # after changing dependencies
```

## Licences

**Settings › Licenses** lists every crate and bundled asset with its licence.

- The crate list is `crates/app/third_party.json`, generated from `cargo metadata`.
- A test fails when it no longer matches `Cargo.lock`. Run `scripts/third_party.py` to refresh it.

## Releases

Run the **Release** workflow from GitHub Actions. It builds the packages, tags the workspace
version (for example `v0.1.0`) and publishes a release with SHA-256 checksums.

| Platform | Packages                           |
| -------- | ---------------------------------- |
| Windows  | Installer and ZIP                  |
| macOS    | DMGs for Apple Silicon and Intel   |
| Linux    | x86-64 AppImage and Debian package |

Packaging details are in
[packaging/README.md](https://github.com/Disservin/fast/blob/master/packaging/README.md).

## Local Linux install

This adds Fast with a launcher and taskbar icon:

```sh
cargo build --release -p fast
install -Dm755 target/release/fast ~/.local/bin/fast
install -Dm644 assets/app/fast.svg ~/.local/share/icons/hicolor/scalable/apps/dev.fast.app.svg
install -Dm644 assets/app/dev.fast.app.desktop ~/.local/share/applications/dev.fast.app.desktop
```

Make sure `~/.local/bin` is on your `PATH`, then launch **Fast** from the application menu.

The app icon is `assets/app/fast.png`; its editable source is `assets/app/fast.svg`.

## Engine catalog

Engine download entries live in
[config/engines.json](https://github.com/Disservin/fast/blob/master/config/engines.json).
To add pinned, checksummed platform builds without changing app code, see
[the catalog guide](https://github.com/Disservin/fast/blob/master/config/README.md) and the
[JSON schema](https://github.com/Disservin/fast/blob/master/config/engines.schema.json).
