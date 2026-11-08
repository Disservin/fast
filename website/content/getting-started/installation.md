# Install & open

## Run Fast

```sh
cargo run --release -p fast            # your real profile
cargo run --release -p fast -- --demo  # a generated profile with every screen filled in
```

### Demo mode

- `--demo` uses its own directories (`fast-demo`) and never touches your data.
- Engine output is scripted, so no engine needs to be installed.
- The first start generates the demo databases, which takes a few seconds.

### Choose the first screen

Add `--screen` to pick the screen Fast opens on:

```sh
cargo run --release -p fast -- --screen settings:engines
```

| Value                                                             | Opens              |
| ----------------------------------------------------------------- | ------------------ |
| `analyse`, `database`, `train`, `play`, `tournament`, `test-positions`, `settings`                | That tab           |
| `settings:engines`, `:board`, `:databases`, `:shortcuts`, `:data` | That settings page |

## First steps

1. **Add an engine.** Open **Settings › Engines** and choose **Download engine** for a catalog
   engine, or **Add from file** for a local UCI executable. See [Engines & profiles](engines.md).
2. **Import your games.** Use **Database › Import** to add PGN files. See [Import games](import.md).
3. **Set your name** under **Settings › Databases** so Fast can tell which games are yours.
4. **Pick a look** under **Settings › General › Appearance**. See [Appearance & language](../reference/settings.md).

## Troubleshooting

### WSLg or Wayland does not start

Some WSLg terminals do not set the Wayland session variables correctly. Either:

- set `XDG_RUNTIME_DIR=/mnt/wslg/runtime-dir`, or
- unset `WAYLAND_DISPLAY` to run through X11 instead.

### Linux build dependencies

```sh
sudo apt install libfontconfig-dev libfreetype-dev libxkbcommon-dev \
  libxkbcommon-x11-dev libwayland-dev libxcb1-dev libasound2-dev
```
