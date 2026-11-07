# Engine catalog

`engines.json` is Fast's bundled engine catalog. JSON is used so entries can be
validated by `engines.schema.json`, edited by standard tools and deserialized into
Rust without a second configuration format.

Settings → Engines → Download engine lists the catalog. Download & install fetches
the matching OS/architecture build, verifies SHA-256, extracts it into Fast's data
directory, saves the license/source references, then checks its UCI handshake and
adds it to the existing engine manager. All subsequent use is offline. A missing
platform build is shown explicitly; users can always add their own executable.
Installed copies are stored under `engines/<id>/<version>/<os>-<arch>/` in Fast's
data directory. Cancelled or failed downloads leave no partial published version.
Configuration and existing engines are not replaced if an installation fails.

## Add or update an engine

1. Copy an entry in `engines.json`; choose a unique lowercase `id` and pinned
   `version`. Only UCI engines are supported. Do not use `latest` download URLs.
2. Provide the project's homepage, exact corresponding source version, SPDX
   license identifier and HTTPS URL to the license text. Include any network or
   bundled-data license requirements in the description and archive.
3. Add one `downloads` entry per supported OS/architecture. Valid OS values are
   `linux`, `macos`, `windows`; architectures are `x86_64`, `aarch64`. Prefer
   baseline/portable builds; universal Stockfish builds detect CPU features. Do
   not advertise an AVX-only binary as a generic x86_64 build.
4. Use an official, pinned HTTPS asset URL and its 64-character SHA-256. GitHub
   release API `assets[].digest` values are usable after removing `sha256:`.
   For older releases, download the exact official artifact and run
   `sha256sum filename`. Never leave a placeholder hash or use a moving URL.
5. Set `format` to `binary`, `zip` or `tar_gz`. `executable` is its exact relative
   path *inside the extracted archive*, or the desired filename for a bare
   binary. Keep all networks, books and other supporting files in their original
   relative paths; the engine layer uses the executable's directory as its cwd.
6. Optional `defaults` map UCI option names to string values. Defaults are applied
   only to options the engine actually advertises. No shell commands, install
   scripts or arbitrary command-line snippets are run from the catalog.
7. Run `cargo test -p fast engine_catalog` and `cargo clippy -p fast -- -D warnings`.
   Rebuild Fast to bundle the updated catalog. No UI or installer code edits are
   required for additional engines using these package formats.

## Checks and maintenance

The catalog uses schema version 1 and rejects unknown fields, duplicate IDs,
duplicate platform entries, invalid checksums, unsafe paths and non-HTTPS URLs.
ZIP/tar extraction rejects traversal, links and special files, and limits file
count and total sizes. Downloads are bounded to 256 MiB; extracted payloads to
512 MiB. Change these limits in the installer only when a reviewed engine needs it.
DMG installers and engines requiring an additional runtime or separately fetched
network are not supported by this schema; do not add misleading download entries.

Live tests are explicit so the normal test suite stays offline:

```sh
cargo test -p fast download_and_handshake_fairy_stockfish -- --ignored --nocapture
FAST_ENGINE_ARCHIVES=/path/to/official/archives cargo test -p fast verify_downloaded_release_archives -- --ignored --nocapture
```

The archive test checks matching files present in that directory using the exact
asset basenames from the URLs. The live test installs into a temporary directory,
checks a repeat installation and verifies the actual engine UCI handshake.

Release assets and license URLs were checked on 2026-11-07. Stockfish and
Berserk checksums come from official GitHub release metadata; Fairy-Stockfish 14
checksums were computed from the pinned official release assets (older release
metadata does not include digests). Source and license references are retained in
each installed version for redistribution compliance; engine binaries are not
bundled with Fast itself.
