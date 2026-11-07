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

In **Line preview**, hover a move to see it as an arrow. Click a move to go there: the board, engine
and table all follow, without changing the game. Click a table row to continue the line that way,
or drag across moves to highlight them as arrows. `Ctrl/Cmd+C` copies the highlight and `Esc`
returns to the game.

## Analysis display

- **Moves from here** lists engine lines (MultiPV), database and repertoire moves. **By eval /
  By depth** changes the order, **All moves** adds every legal move like Nibbler, and **Arrows**
  draws one arrow per engine line, coloured from best (blue) through good (green), inaccuracy
  (yellow) and mistake (orange) to blunder (red).
- Click an engine arrow on the board to play its move, as in Nibbler (turn it off under
  **Settings › Board & pieces**). Clicking one of your pieces still selects it.
- **Lines − +** in the table sets MultiPV for every analysis engine, saved in its options; the
  **Engine** panel sets it per engine.
- Lines are remembered per position, so stepping back and forth keeps them, and a position
  reached by another move order (a transposition) shows what was already found.
- Stepping forward returns into the variation you came back from.
- The graph under the board has tabs: **Eval** (symmetric log scale), **Time** per move from
  `[%clk]`/`[%emt]` or TCEC `mt=` comments, **Depth**, and the original **Bars** timeline.
  Annotated games (`[%eval]`, TCEC `wv=`/`d=`) fill the graphs without an engine.
- The scoresheet toggles between **Table** (Lichess style) and **Inline** (Nibbler style, each
  variation depth in its own colour).

## Engines

Several engines can run side by side, including two entries for one binary: **Duplicate** an
engine in Settings, then give each its own options (for example Lc0 with BT2 and Lc0 with Maia).
Options are read from the engine and laid out in a compact grid; every UCI type is supported.
**Profiles** save and switch whole option sets. Click the icon to choose a picture, otherwise a
rook is shown; an image named after the binary, `logo.*` or `icon.*` next to it is picked up
when the engine is added. The engine's `UCI_EngineAbout` text is shown as its description.
**Debug** sends `debug on` and opens a window following the raw output (also under
**Engine › Engine output window**, or **Window** in the Log panel).

## Menus

File, Edit, View, Go and Engine menus are native on macOS and drawn in the top bar elsewhere.
**File** opens and saves PGN files (FEN too), and **Edit** copies or pastes FEN and PGN.

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
