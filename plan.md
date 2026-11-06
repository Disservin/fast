Build "Outpost": a cross-platform desktop chess workstation in Rust + GPUI

You are a senior Rust engineer. Build a native desktop chess application called Outpost for Windows, macOS and Linux using GPUI (Zed's GPU-accelerated UI framework). It is an offline, single-user tool for analysis, game databases, opening training and playing against engines. There is no online or friend play.

Work in milestones (bottom of this prompt). After each one the app must compile with cargo build on all three platforms, pass cargo clippy -- -D warnings and cargo test, and run.

1. Tech stack
Concern	Choice
UI	gpui + gpui_platform (git: zed-industries/zed, pin a specific rev in Cargo.toml)
Components	gpui-component (git: longbridge/gpui-component, pin rev): inputs, selects, sliders, checkboxes, tables, tabs, dock/resizable panels. Call gpui_component::init(cx) at startup
Chess rules	shakmaty (move generation, FEN, SAN/UCI, legality)
PGN	pgn-reader (streaming import of large PGN files)
Tablebases	shakmaty-syzygy (optional, for the 3–6 piece WDL/DTZ badge)
Storage	rusqlite (bundled feature) with one SQLite file per database, plus position hashes (Zobrist from shakmaty) indexed for position search
Engines	UCI over stdin/stdout, spawned with std::process and read on GPUI background tasks (cx.background_spawn). Write a small typed UCI layer yourself (uci module)
Config	serde + toml in the platform config dir (dirs crate)
Errors/logging	anyhow, thiserror, log + env_logger

Don't add tokio. Use GPUI's executors (cx.spawn, cx.background_spawn) and smol/futures primitives where needed.

2. Workspace layout
outpost/
  Cargo.toml            # workspace
  crates/
    app/                # binary: window, routing, global actions, theme
    ui/                 # shared GPUI views: Board, Scoresheet, EvalTimeline, MovesTable, TopBar…
    chess_core/         # game tree (mainline + variations + NAGs + comments), PGN in/out, no UI deps
    engine/             # UCI process manager, EngineHandle, analysis streams, option schema
    db/                 # SQLite schema, import, queries, position search, stats
    trainer/            # repertoire tree + spaced-repetition scheduler (FSRS-style or SM-2)
  assets/
    fonts/              # IBM Plex Sans + IBM Plex Mono (SIL OFL), loaded via cx.text_system().add_fonts
    pieces/             # SVG piece set — only use a set whose license allows bundling; record it in assets/LICENSES.md
    icons/              # Lucide SVGs (ISC)

chess_core, engine, db and trainer must have no GPUI dependency and must be unit-tested.

3. Design system (dark only for v1, keep tokens in one Theme struct)

Neutral monochrome. No blue accent anywhere. The "accent" is off-white.

bg            #0E1013   app background
surface       #16191E   panels, header, side columns
surface_2     #12151A   table header rows
hover         #1B2028
selected      #22272E   selected row/item (+ 3px inset left bar in fg)
border        #262B33
border_strong #2C323B   buttons, inputs
divider       #1D2127   row separators
fg            #E8EAED   primary text AND accent (primary buttons: fg bg, #0E1013 text)
fg_2          #C3C9D1   secondary text, side-variations in scoresheet
muted         #9AA3AE   labels, metadata
faint         #7F8893   move numbers
good          #3FB950   "Known", engine ready
warn          #F0A33A   "Due", leaks, repertoire deviation
arrow         #E8892B   board arrows (orange)
board_light   #DCD8D0   "Stone" board
board_dark    #9A948A
last_move     #F3E3A6 / #C9B56E  (light / dark square highlight)
Type: IBM Plex Sans for UI (14px base), IBM Plex Mono for all notation, evals, numbers and FEN.
Section labels: 11px, 600 weight, uppercase, 0.08em letter spacing, muted.
Radius 6px on panels and buttons, 4px on inputs. Flat surfaces with 1px borders, no shadows, no gradients.
Buttons 36px high; primary = fg background. Segmented controls are rows of buttons where the selected one gets selected bg + fg border.
Board: square, coordinates drawn inside the edge squares (rank top-left, file bottom-right, 11px semibold), 1px border_strong outline. Board themes: Stone (default), Walnut #E9D7B4/#A97A50, Moss #E6E9D8/#7F956B, Graphite #C9CBCE/#55595F.
4. App shell

Single window, min 1100×720, default 1440×900.

Top bar (surface, 1px bottom border), three zones:

Left: logo mark (2×2 squares, two filled) + "Outpost" (700, 16px) + / <context> breadcrumb in muted.
Center: segmented mode switch Analyse · Database · Train · Play (active = fg bg, dark text).
Right: context controls + a gear button that opens Settings.

Global keyboard shortcuts (GPUI actions + keymap, user-editable): ←/→ prev/next move, Home/End, Space engine on/off, F flip, Ctrl/Cmd+K search, Ctrl/Cmd+S save.

5. Screens
5.1 Analyse (the main screen)

Three columns, left to right:

Scoresheet (300–340px, surface, right border). Header: White/Black names with ratings, ECO + opening name. Body: an OTB-style grid, [move no | white | black], mono 14px, one row per full move, divider between rows. Side variations are indented blocks under their move (2px left border, fg_2, mono 13px). Comments are italic sans 13px under the move. The current move is a pill (fg bg, dark text). NAGs show in warn. Clicking a move jumps there. Footer buttons: ! ? !? Comment Promote. Right-click menu: delete from here, promote variation, copy PGN from here.
Board column: a heading "White to move" + "after 13…Ne8" (muted) + the current eval at right (mono 18px). Then a 576px board (scales with the window, always square). Drag-and-drop and click-click moves, legal-move dots, last-move highlight, right-drag arrows (orange), right-click circles. Under the board, the Game timeline: one thin bar per ply showing eval (up = White, down = Black in warn), current ply in fg, others 
#6B7480. Click or drag on it to scrub the game; it replaces both an eval bar and an eval graph. Below that: first/prev/next/last, Flip, Set up, Copy FEN.
Moves from here (fills remaining width): one table that merges engine, database and repertoire for each candidate move. Columns: Move | SF eval | Lc0 W/D/L bar | Reference DB "games · score%" | My games "n · score%" | Repertoire status (Main in good / "You deviate" in warn / Side line / —). Rows come from the union of the engine MultiPV lines, DB moves and repertoire moves, sorted by engine eval. Clicking a row makes it selected and draws its arrow on the board. Under the table, Line preview: the selected row's full PV (mono), engine/depth/speed meta, a one-line explanation slot (empty for now; leave a hook), and buttons Play line on board, Add as variation, Add to repertoire. Under that, a Position note text area saved with the position.

The engine runs continuously while Analyse is open and the engine is on. Throttle UI updates to ≤ 10 Hz. Changing position must cancel the search immediately (stop, then a new position/go infinite).

5.2 Database
Left column: Databases list (name + game count, mono) and a Filter form: player, colour, ECO range, min opponent rating, time control, checkboxes Reached the Analyse board position and Only games where I left my repertoire, plus a primary "Show N games" button.
Main: a title with the game count + filter summary, Analyse all (batch engine review) and Export PGN. A stats strip with Score and Most common leak (the most frequent position where the user left their repertoire, shown as a mono move in warn). A virtualized games table (must stay smooth with 1M+ rows): White+rating, Black+rating, Result (winner in bold), Left rep. (the move where the user deviated, warn, or —), Moves, Date, Source, Accuracy. Selected row = selected + inset bar.
Right: a preview panel with a small board, game header, a mono move snippet around the key moment, and Open in Analyse.
Import: PGN files (streaming, progress, cancel) into SQLite. Index Zobrist hashes per ply for position search.
5.3 Train (repertoire trainer)
Left column: the Repertoire tree (indented mono nodes, status tag at right: Known good, Learning fg_2, Due warn, New muted), a coverage bar (known/learning/new) and Add line, Find gaps.
Center: "DRILL · 7 OF 24 DUE" with a progress bar, the prompt "Your move as White" + the preceding moves, the 576px board (the user plays the answer), Hint, Show answer, Why this move?, Skip, then four recall buttons Again / Hard · 1d / Good · 4d / Easy · 11d with intervals from the scheduler.
Right: Leaks from your games: positions where the user's real games left the repertoire, with counts and score impact, plus Drill these lines.
Scheduler lives in trainer, is deterministic and is unit-tested.
5.4 Play (engine only)
Left column "NEW GAME VS ENGINE": Opponent select (configured engines, e.g. Stockfish with UCI_LimitStrength, Maia), a Strength slider (UCI_Elo range), time control grid 3+2 5+0 10+5 15+10 30+0 Custom, colour White/Random/Black, Start from (starting position / current Analyse position / a repertoire line), checkboxes Hide evaluation during the game (default on) and Allow takebacks, and a primary Start game.
Center: an opponent clock bar above the board and the user's clock bar below it. The active clock gets an fg border and an inverted time readout.
Right: a compact moves sheet + opening name, Offer draw / Takeback / Resign, and Analyse this game (opens it in Analyse).
5.5 Settings

Settings layout: left nav (General, Board & pieces, Engines, Databases, Keyboard shortcuts) and a content column (max 1180px) of bordered sections. Each option row is a two-column grid: label + hint on the left, control on the right.

General: theme (Dark only active for now; Light / Match system disabled), language.
Board & pieces: board theme swatches (2×2 mini boards), piece set, coordinates (On squares / Outside / Off), move animation slider (0–400 ms), notation (SAN / figurine / long algebraic), show legal moves, highlight last move, auto-queen, move sounds.
Engines (engine manager): a table of installed engines (name + version, protocol, CPU/GPU, Used for, status Ready/Disabled/Error). Header buttons Add from file and Download engine. Selecting a row shows its detail: executable path, then options generated from the engine's option lines at uci handshake (spin → number input/slider, check → checkbox, combo → select, string → text, button → button) with sensible hints for Threads, Hash, MultiPV, SyzygyPath, UCI_ShowWDL. Lc0 nets appear as its WeightsFile option. Then "Use this engine for": Live analysis / Batch game review / Play. Buttons: Run benchmark (bench where supported, else fixed-time search on a position suite), Check for update, Remove. Persist per-engine options and re-apply them with setoption on every launch.
Keyboard shortcuts: an editable list rendered with key caps.
6. Engine layer requirements
EngineManager owns processes; EngineHandle exposes analyse(position, multipv) -> Stream<AnalysisUpdate> and best_move(position, limits).
Parse info fully: depth, seldepth, multipv, score cp/mate, wdl, nodes, nps, tbhits, pv. Convert PVs to SAN with shakmaty.
Handle crashes. Mark the engine Error, show a non-blocking toast and allow restart. Never block the UI thread.
Several engines can analyse the same position at once (Stockfish + Lc0 on Analyse).
7. Non-goals for v1

No online play, no accounts or cloud sync, no light theme implementation, no mobile, no web build.

8. Milestones
Workspace, theme, fonts, app shell with top bar and mode routing (empty screens).
chess_core game tree + PGN round-trip tests; Board view with legal moves, drag/drop, arrows, flip.
Analyse screen: scoresheet, timeline, navigation, editing, variations, comments.
engine crate + Settings › Engines manager; live MultiPV in Moves from here.
db crate: PGN import, games table, filters, position search; wire DB columns into Moves from here.
trainer crate + Train screen; repertoire column in Moves from here; leaks detection.
Play screen with clocks and engine opponent.
Settings › General, Board & pieces, Shortcuts; persistence; polish, packaging (cargo-bundle or platform installers) and CI for all three OSes.
9. Quality bar
60 fps while dragging pieces and scrubbing the timeline; importing 1M games must not freeze the UI.
Everything keyboard-reachable; visible focus rings (fg, 2px).
Text contrast ≥ 4.5:1 on its background.
Keep views small: each screen is a Render struct composed from reusable ui components; state in Entity<T> models, communication via cx.notify() and events.
Write a short ARCHITECTURE.md and keep it updated as you go.

Start with milestone 1. Before writing code, list the exact pinned git revisions you'll use for gpui, gpui_platform and gpui-component, and confirm they build together.
