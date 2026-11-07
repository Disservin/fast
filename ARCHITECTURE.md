# Architecture

## Crates

| Crate | Depends on GPUI | Responsibility |
| --- | --- | --- |
| `chess_core` | no | Game tree (mainline, variations, NAGs, comments), PGN in/out, notation, opening table |
| `engine` | no | UCI parser, supervised engine processes, analysis events |
| `db` | no | One SQLite file per database, streaming PGN import, filters, position search, notes |
| `trainer` | no | Repertoire tree, SM-2 scheduler, leak and gap detection |
| `ui` | yes | Design tokens and shared views: `BoardView`, scoresheet, timeline, buttons |
| `fast` (`crates/app`) | yes | Window, routing, shortcuts, the five screens, configuration, demo mode |

The four domain crates are pure and unit-tested, including a fake UCI engine for process tests
(`crates/engine/tests`).

## Pinned dependencies

`gpui-component` is pinned to a git rev. That rev builds against the `gpui-pre` 0.3.11 snapshot of
Zed's crates from crates.io, so `gpui` and `gpui_platform` use that exact snapshot rather than
a separate Zed git rev, which would give two incompatible copies of GPUI. Bump them together.

## State and data flow

- `AppModel` (`Entity`) holds configuration, the database list, the repertoire, notes, toasts and
  the engine manager. Screens read it and subscribe to `ModelEvent`.
- Each screen is a `Render` struct holding an `Entity<BoardView>` and its own state. Boards report
  moves through `BoardEvent`; they never change a game themselves.
- `Workspace` routes between screens and owns global shortcuts. Its key context is dropped while
  a text field has focus, so typing never triggers shortcuts.

## Window decorations

The window asks for native decorations. Some Wayland compositors, including WSLg, decline, and
GPUI then reports `Decorations::Client`. `Workspace` responds by adding minimize, maximize and
close to the top bar, making its empty area drag the window, and placing resize strips on
the edges. `--client-decorations` forces the controls on, to test them elsewhere.

On Windows the system title bar is hidden (`appears_transparent`) and the top bar takes its
place, so the drawn menus share a line with minimize, maximize and close. Those controls and the
empty bar are marked with `window_control_area`, so Windows itself handles them: dragging,
double-click to maximize, snapping and Snap Layouts. Everything clickable in the bar uses
`occlude()`, because a click on the drag area would otherwise go to the system caption.

## Live engine options

`EngineHandle::set_option` is safe during a search. The supervisor sends `stop`, waits for
`bestmove`, applies the option, then resumes the same search, so UCI's rule that options are
only set while idle always holds. Each handle also keeps a bounded `EngineLog` of stdin, stdout
and stderr, which outlives the process so a crash can still be read.

## Sound

Classic (the default for new profiles) uses real wooden-board recordings by el_boss,
released under CC0. MP3 originals, source links and the license are in `assets/sounds/classic`.
`scripts/prepare_chess_sounds.py` converts them to embedded mono 44.1 kHz PCM; playback
needs no decoder, external files or network. Classic plays placement/capture recordings,
two placements for castling and a board-reset recording at game end, without electronic
check tones. Existing saved sound choices remain selectable; Classic is available in Settings.

Wood, Soft, Click, Bell, Deep wood and Thud remain as optional synthesized profiles.
Previews and saved volume apply to every profile. PCM loading, synthesis and playback run
on the sound thread with a bounded request queue. The output device opens there on first
use, because its stream cannot move between threads on every platform; unavailable audio
is skipped without blocking the UI.

## UCI protocol

- `uci`, then the saved options once `uciok` arrives.
- Before a search, whenever options or `ucinewgame` were sent since the last `readyok`
  (always, before the first one), the supervisor sends `isready` and holds `position`/`go`
  until `readyok`, so an engine loading a network or resizing its hash can finish first.
- Each `SearchRequest` names its game. A search from a different game is preceded by
  `ucinewgame` (and so by `isready`). Analyse bumps the game when it loads another one, Play
  starts a process per game, and batch review uses the database game id.
- The standard initial position is sent as `position startpos`, other starts as
  `position fen`, with the moves after `moves`.
- Analysis engines that offer `UCI_AnalyseMode` get it set to `true`.

## Analysis cache

`LiveAnalysis` keeps a `LineCache` of each engine's lines per position hash (Zobrist), so lines
survive navigation and transpositions share them. Restored lines are marked `cached` and stay
until the new search reaches their depth. Changing an engine's options or configuration
forgets its entries; the cache starts over past 50,000 positions.

## Threads

The UI thread never blocks on engines or databases.

- **Engines:** every process has a supervisor thread that owns stdin and a reader thread. These are
  dedicated OS threads rather than GPUI background tasks because each one blocks for the engine's
  whole lifetime and would otherwise permanently occupy a pool worker. The app consumes events
  over async channels. `LiveAnalysis` coalesces them to at most 10 UI updates per second per engine.
- **Search cancellation:** the supervisor sends `stop`, discards output until the old `bestmove`,
  then starts the newest request. Stale lines cannot overwrite a new position's analysis.
- **Databases:** queries run in `cx.background_spawn` with a new connection per task. Dropping the
  task cancels a stale query's delivery. Imports commit every 2,000 games so readers keep working.
- **Games table:** ids for the filter are loaded once and rows are paged on demand as
  `uniform_list` reveals them, so memory and render cost follow the viewport, not the row count.

## Rendering cost

Profiling a full redraw at 1440×900 under llvmpipe puts about 97% of frame time in rasterization
and about 2% in Fast's code, and every screen costs about the same. Views should still avoid
needless frames: the board positions pieces proportionally so a resize needs only one draw.

## Files

- Config: `<config dir>/fast/config.toml`, including per-engine options re-applied at launch.
- Data: `<data dir>/fast/{databases/*.sqlite, repertoire.json, notes.sqlite}`.
- Demo mode uses `fast-demo` instead and is regenerated if its stamp file is missing.

## Known limits

- **WSLg frame rate:** measured at 25–28 fps at 1440×900, including through an RTX 4090 via Mesa's
  D3D12 driver, so a faster renderer does not lift it there.

- **Language** only offers English; Light and Match system themes are disabled, as planned.
- **Tablebases:** `SyzygyPath` is passed to engines, but there is no built-in WDL/DTZ badge, so
  `shakmaty-syzygy` is not a dependency yet.
- **Check for update** cannot check anything, since Fast is offline; it says so.
- The Line preview explanation is a hook with no content outside demo mode.

## Recoverable work and study

Profile and PGN replacement stages a file beside its destination, syncs it and atomically
replaces the destination. A `.previous` sibling retains the preceding version; config and
repertoire saves preserve only valid previous data and startup falls back to that version.
Settings exposes storage paths, persistent unsaved status and retry controls. A cancelled
PGN dialog does not mutate the game or its saved baseline.

Analyse recovery uses `game-recovery.json`; engine games use `play-recovery.json` (PGN,
including clock metadata). Startup offers a preview and explicit restore/discard. Engine-game
recovery opens the preserved game in Analyse rather than automatically restarting an engine.
Study recovery lives inside `repertoire.json`, alongside cards and append-only review events,
so queue advancement and scheduling commit together. Evidence records hints, reveals,
failures, active training time, requested/applied grades and before/after cards. Review IDs
reject duplicate submission. Failed or revealed attempts schedule Again; hints cap at Hard.

The visible repertoire remains a tree. Reply lookup, card propagation, unique coverage,
drills, gaps and leak grouping compare full legal-en-passant FEN identity without clocks.
Legacy path cards are retained in `legacy_cards` when merging schedules. Any-reply practice
accepts prepared alternatives; every-reply practice asks for the queued answer explicitly.

Settings exports versioned profile archives containing config, repertoire (including study
history), recovery records, notes and all managed game databases. Export and preview validation
run in background tasks. SQLite snapshots/restoration use its online backup API, including WAL
contents. Restore validates payloads, migrates legacy JSON/TOML defaults, previews files and
offers keep/replace conflict policies. It stages rollback data before applying any files and
rolls back on an apply failure. Applying a restore is currently exclusive on the UI thread;
large restores can pause the window. Restart after restoring engine and shortcut settings.

## Optional engine downloads

`config/engines.json` is the versioned bundled catalog, documented and accompanied
by a JSON schema in `config/`. Settings lists compatible builds and can download,
verify, install and register them. `engine_catalog.rs` uses blocking HTTPS on a
background task, streams bounded downloads, checks pinned SHA-256, extracts only
safe ordinary files, and atomically publishes a staged version directory. License
and corresponding-source references remain with every installation. Cancellation
and failures preserve existing engines; UCI handshake success is required before
registration. Installed engines need no network connection. Catalog entries carry
options as data and never run shell install commands.
