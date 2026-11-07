# Fast

A native desktop chess workstation for analysis, game databases, opening training and
playing against engines. Offline and single-user. Built with Rust and
[GPUI](https://www.gpui.rs).

## Workspace tabs

Click **+** in the tab bar to open a picker with a card for each kind of view (Analyse, Play,
Database, Tournament). Each opens an independent view. Opening a database game or analysing an engine game creates a new analysis
tab. Each tab keeps its own position and game state. Analysis runs only in the
selected tab; games in other open Play tabs keep their clocks running.

**Ctrl/Cmd+T** opens another tab of the current type (Analyse from Train/Settings),
**Ctrl/Cmd+Tab** and **Ctrl/Cmd+Shift+Tab** switch tabs, **Ctrl/Cmd+W** closes a tab,
and **Ctrl/Cmd+Shift+T** or **Reopen** restores the most recently closed tab (up to
ten kept in memory). Closing a Play tab pauses that game; reopening resumes it.
Every open game saves automatically and is back in its tab after a restart.
Train and Settings each have a single shared tab.

A king in check is highlighted in red on every board, including checkmate. The
highlight clears when check ends and is hidden while editing a position.

Tournament setup includes a searchable dropdown of [Stockfish opening books](https://github.com/official-stockfish/books),
with `UHO_Lichess_4852_v1.epd` selected and recommended by default. Books download on
first use and are cached for offline reuse. Choose **No book** for the initial position,
or **Custom file…** to supply a local EPD or PGN.

## Run it

```sh
cargo run --release -p fast            # your real profile
cargo run --release -p fast -- --demo  # a generated profile with every screen populated
```

`--demo` uses its own directories (`fast-demo`), never touching your data. Its engine output
is scripted, so no engine needs to be installed. A first start generates its databases, which
takes a few seconds. `--screen analyse|database|train|play|settings[:engines|board|databases|shortcuts]`
picks the opening screen.

Choose a full interface palette under **Settings › General › Appearance**: Slate, Paper, Sand,
Forest, or Plum. Themes apply immediately and are remembered across restarts; chessboard colors
remain a separate choice under **Settings › Board & pieces**.

In normal mode, choose **Settings › Engines › Download engine** to install a catalog engine,
or use **Add from file** for a local UCI executable. Downloads need internet access;
installed engines run offline. Import PGNs
with **Database › Import**. Set your name under **Settings › Databases** so Fast can tell which
games are yours.

On a shell without a Wayland session variable set correctly (some WSLg terminals), set
`XDG_RUNTIME_DIR=/mnt/wslg/runtime-dir`, or unset `WAYLAND_DISPLAY` to run through X11 instead.

### Linux build dependencies

```sh
sudo apt install libfontconfig-dev libfreetype-dev libxkbcommon-dev \
  libxkbcommon-x11-dev libwayland-dev libxcb1-dev libasound2-dev
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
- **Analyse backwards** (above the graph, or **Engine › Analyse game backwards**) searches every
  position from the last move to the first, like Nibbler, so each search benefits from what the
  engine learned about the moves after it. Each position gets the search limit, or 3 seconds
  when it is infinite; moving elsewhere or **Stop** ends the run.
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

## Play

Against an engine you can **premove**, as on Lichess: while the engine thinks, drag or click a
piece to queue a move (its squares turn blue). It is played the moment the engine replies, if
it is still legal, and dropped otherwise; a right click or a click on an empty square cancels
it, and promotions become queens. Turn it off with **Allow premoves** before starting a game.

## Keyboard shortcuts

Every shortcut can be changed, cleared or reset under **Settings › Keyboard shortcuts**. A key
taken by another action moves to the new one. `Ctrl` is `Cmd` on macOS. The single keys work
in Analyse while no text field has focus.

| Key | Action |
| --- | --- |
| `←` `→` | Previous / next move (forward returns into the line you came from) |
| `Home` `End` | Start of game / end of line |
| `F` | Flip board |
| `Ctrl+N` / `Ctrl+O` | New game / open PGN or FEN |
| `Ctrl+S` / `Ctrl+Shift+S` | Save to My games / save as PGN |
| `Ctrl+Shift+V` | Paste FEN or PGN |
| `Ctrl+Shift+F` / `Ctrl+Shift+P` | Copy FEN / copy PGN |
| `Space` | Engine on / off |
| `P` | Pause / resume the search |
| `B` | Analyse the game backwards |
| `[` `]` | Fewer / more lines (MultiPV) |
| `Ctrl+Shift+L` | Engine output window |
| `A` | All moves |
| `R` | Quality arrows |
| `D` | Sort lines by eval / depth |
| `G` | Next graph |
| `I` | Table / inline scoresheet |
| `Ctrl+K` | Search (a FEN opens in Analyse, anything else filters players) |
| `Ctrl+1`…`Ctrl+4` | Analyse, Database, Train, Play |
| `Ctrl+,` | Settings |
| `Ctrl+Q` | Quit |

## Develop

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
python3 scripts/third_party.py   # after changing dependencies
```

**Settings › Licenses** lists every crate and bundled asset with its licence. The crate list is
`crates/app/third_party.json`, generated from `cargo metadata`; a test fails when it no longer
matches `Cargo.lock`.

See [ARCHITECTURE.md](ARCHITECTURE.md). Package installers with
[cargo-bundle](https://github.com/burtonageo/cargo-bundle): `cargo bundle --release -p fast`.

The bundle uses the app icon in `assets/app/fast.png`; its editable source is
`assets/app/fast.svg`. For a local Linux install with a launcher and taskbar icon:

```sh
cargo build --release -p fast
install -Dm755 target/release/fast ~/.local/bin/fast
install -Dm644 assets/app/fast.svg ~/.local/share/icons/hicolor/scalable/apps/dev.fast.app.svg
install -Dm644 assets/app/dev.fast.app.desktop ~/.local/share/applications/dev.fast.app.desktop
```

Ensure `~/.local/bin` is on your `PATH`, then launch **Fast** from the application menu.

Engine download entries live in [config/engines.json](config/engines.json). See
[the catalog guide](config/README.md) and [JSON schema](config/engines.schema.json)
for adding pinned, checksummed platform builds without changing app code.
