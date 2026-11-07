# Fast

A native desktop chess workstation for analysis, game databases, opening training and
playing against engines. Offline and single-user. Built with Rust and
[GPUI](https://www.gpui.rs).

## Run it

```sh
cargo run --release -p fast            # your real profile
cargo run --release -p fast -- --demo  # a generated profile with every screen populated
```

`--demo` uses its own directories (`fast-demo`), never touching your data. Its engine output
is scripted, so no engine needs to be installed. A first start generates its databases, which
takes a few seconds. `--screen analyse|database|train|play|settings[:engines|board|databases|shortcuts]`
picks the opening screen.

In normal mode, add a UCI engine under **Settings › Engines › Add from file**, and import PGNs
with **Database › Import**. Set your name under **Settings › Databases** so Fast can tell which
games are yours.

On a shell without a Wayland session variable set correctly (some WSLg terminals), set
`XDG_RUNTIME_DIR=/mnt/wslg/runtime-dir`, or unset `WAYLAND_DISPLAY` to run through X11 instead.

### Linux build dependencies

```sh
sudo apt install libfontconfig-dev libfreetype-dev libxkbcommon-dev \
  libxkbcommon-x11-dev libwayland-dev libxcb1-dev
```

## Engine controls in Analyse

**Engine** (top of the Moves table) switches engines on or off and changes Lines, Threads, Hash and
the search limit while the search runs, without restarting the engine. **Log** shows the raw UCI
traffic and stderr; tick **Info** to include search output.

In **Line preview**, click a move to see that position on the board, or drag across moves to
highlight them as arrows. `Ctrl/Cmd+C` copies the highlight and `Esc` returns to the game.

## Keyboard shortcuts

Rebindable under **Settings › Keyboard shortcuts**.

| Key | Action |
| --- | --- |
| `←` `→` | Previous / next move |
| `Home` `End` | Start / end of game |
| `Space` | Engine on / off |
| `F` | Flip board |
| `Ctrl/Cmd+K` | Search (a FEN opens in Analyse, anything else filters players) |
| `Ctrl/Cmd+S` | Save the game to My games |

## Develop

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

See [ARCHITECTURE.md](ARCHITECTURE.md). Package installers with
[cargo-bundle](https://github.com/burtonageo/cargo-bundle): `cargo bundle --release -p fast`.
