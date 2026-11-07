# Architecture

## Crates

| Crate | Depends on GPUI | Responsibility |
| --- | --- | --- |
| `chess_core` | no | Game tree (mainline, variations, NAGs, comments), PGN in/out, notation, opening table |
| `engine` | no | UCI parser, supervised engine processes, analysis events |
| `db` | no | One SQLite file per database, streaming PGN import, filters, position search, notes |
| `trainer` | no | Repertoire tree, SM-2 scheduler, leak and gap detection |
| `ui` | yes | Design tokens and shared views: `BoardView`, scoresheet, timeline, buttons |
| `outpost` (`crates/app`) | yes | Window, routing, shortcuts, the five screens, configuration, demo mode |

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
and about 2% in Outpost's code, and every screen costs about the same. Views should still avoid
needless frames: the board positions pieces proportionally so a resize needs only one draw.

## Files

- Config: `<config dir>/outpost/config.toml`, including per-engine options re-applied at launch.
- Data: `<data dir>/outpost/{databases/*.sqlite, repertoire.json, notes.sqlite}`.
- Demo mode uses `outpost-demo` instead and is regenerated if its stamp file is missing.

## Known limits

- **WSLg frame rate:** measured at 25–28 fps at 1440×900, including through an RTX 4090 via Mesa's
  D3D12 driver, so a faster renderer does not lift it there.

- **Set up** takes a FEN rather than offering a drag-and-drop board editor.
- **Move sounds** is stored but no audio is played.
- **Language** only offers English; Light and Match system themes are disabled, as planned.
- **Tablebases:** `SyzygyPath` is passed to engines, but there is no built-in WDL/DTZ badge, so
  `shakmaty-syzygy` is not a dependency yet.
- **Check for update** cannot check anything, since Outpost is offline; it says so.
- The Line preview explanation is a hook with no content outside demo mode.
