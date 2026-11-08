# Architecture

## Crates

| Crate | Depends on GPUI | Responsibility |
| --- | --- | --- |
| `chess_core` | no | Game tree (mainline, variations, NAGs, comments), PGN in/out, notation, opening table |
| `engine` | no | UCI parser, supervised engine processes, analysis events |
| `db` | no | One SQLite file per database, streaming PGN import, filters, position search, editing and deleting games, copying a database, notes |
| `trainer` | no | Repertoire tree, SM-2 scheduler, leak and gap detection |
| `ui` | yes | Design tokens and shared views: `BoardView`, scoresheet, timeline, buttons |
| `fast` (`crates/app`) | yes | Window, routing, shortcuts, the five screens, configuration, demo mode |

The four domain crates are pure and unit-tested, including a fake UCI engine for process tests
(`crates/engine/tests`).

## Release packaging

`.github/workflows/release.yml` is started manually and builds native executables on
Linux, Windows and both Mac architectures. Once all package jobs pass it creates a
`v` tag from the workspace version and publishes the GitHub release. `scripts/package_release.py` stages
the executable, icons and license notices, invokes native packaging tools and
writes SHA-256 checksums. Linux uses dpkg-shlibdeps for Debian dependencies and
linuxdeploy for AppImage library bundling; Windows uses `packaging/windows.iss`
with Inno Setup; macOS creates an ad-hoc signed app bundle and DMG with Apple's
tools. Packaging does not include chess engines or modify user profiles.
See `packaging/README.md` for release steps and validation.

The app build script embeds `assets/app/fast.ico` through `fast.rc` in Windows
executables, including plain CI downloads. The installer uses the same icon.
Windows release builds use the GUI subsystem so opening Fast does not create a
console; debug builds retain the console for development logs.

`app_update.rs` exposes the Cargo package version in Settings › General and
checks the latest stable release of `Disservin/fast` only on user request.
Settings runs release requests, bounded downloads, SHA-256 verification and
installation work in `cx.background_spawn`. Asset URLs must match the official
release and checksums come from its `SHA256SUMS.txt`. AppImage updates use an
atomic replacement in the installation directory and keep a `.previous` backup.
Other installations open the verified Windows installer, Mac DMG or Debian
package through the platform. The user finishes installation and restarts Fast;
the updater does not stop active engines or modify user data. It never downloads
drafts or prereleases and does not check automatically at startup.

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

Futuristic, NES, Piano and SFX are Lichess' free sound sets by Enigmahack (AGPL-3.0-or-later),
kept with their MP3 originals in `assets/sounds/lichess` and converted by the same script.
They play Lichess' move, capture and check sounds, the move sound for castling and the
notify sound at game end.

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
- Batch review starts as many processes of the review engine as `config.review_engines`
  (set by the "Engines at once" stepper), each on its own `background_spawn` worker with its
  own database connection. The workers take the next game from a shared atomic index, and an
  error in one cancels the rest.
- The standard initial position is sent as `position startpos`, other starts as
  `position fen`, with the moves after `moves`.
- Analysis engines that offer `UCI_AnalyseMode` get it set to `true`.

## Engine tournaments

The optional Tournament tab (opened from the new-tab menu, or `--screen tournament`) runs
engine matches with fastchess. fastchess is not bundled: the pinned release in
`fastchess.rs` is downloaded on first use into `<data>/tools/fastchess/<version>/`, verified by
SHA-256 like catalog engines, or the user points Fast at their own executable
(`fastchess_path` in the config). A tournament comes from the form, a pasted command line or a
pasted `config.json`, and runs in its own folder, `<data>/tournaments/<id>/`, with `games.pgn`
and `config.json`. Fast reads fastchess's human-readable stdout on two reader threads. Stop
sends SIGINT on Unix, so fastchess saves its config and Continue resumes with
`-config file=…`; on Windows it ends fastchess and its engines, and the last autosave counts.

fastchess must not outlive Fast, even when Fast crashes or is killed (`process_guard.rs`). On
Unix, Fast starts fastchess through a guard, its own binary run as
`fast --fastchess-guard <fastchess> …`, and keeps the guard's stdin open. When Fast dies the
guard reads end-of-file and sends fastchess SIGINT, so it saves its config as on Stop; the
guard also forwards SIGINT/SIGTERM/SIGHUP, kills fastchess on SIGUSR2 (a second Stop), and
exits with fastchess's status. On Linux fastchess additionally gets SIGINT if the guard
itself dies. On Windows fastchess runs in a job object that kills it and its engines once
Fast's handle closes, which happens however Fast exits.

Live boards come from fastchess's `-events` stream. Fast checks each executable once by
running `fastchess -events file=- -version`, which builds without the stream reject, and then
adds `-events file=- info=true`: one JSON object per line for game starts, moves, throttled
engine output and game ends, with fastchess's game numbers, and `[GameId]` tags in the PGN
that "open this game" uses. The stdout reader thread passes event lines to `LiveGames`
(`tournament_live.rs`) instead of the UI, and the tab copies a snapshot into its boards at
most five times a second. Builds without `-events`, such as older releases, and
commands that set their own `-events` run without live boards. fastchess does not save the
event options into `config.json`. `docs/fastchess-integration.md` lists fastchess changes that
would replace the output parsing and other workarounds.

## Test positions

The optional Test positions tab (new-tab menu, or `--screen test-positions`) runs engines over
an EPD suite. `chess_core::epd` parses each line and keeps a reason for any that fail: bad FEN,
illegal position, a `bm`/`am` move that is not legal there, a bad `dm`. Lines that fail, or that
lack the operation the chosen goal checks (`bm`/`am` for best move, `dm` for mate), are shown in
red and never sent to an engine. `test_suite.rs` runs the rest on worker threads, each owning one
engine process with `MultiPV 1` and the tab's Hash and `SyzygyPath`. In parallel mode every engine
runs at once, split over N processes sharing a queue; otherwise one process runs, engine after
engine. Clearing the hash gives each position its own game id, so it gets `ucinewgame`, and
presses `Clear Hash` where the engine has it. Mate searches send `go mate N` with the limit as a
cap, and count a mate no longer than N.

## Analysis cache

`LiveAnalysis` keeps a `LineCache` of each engine's lines per position hash (Zobrist), so lines
survive navigation and transpositions share them. Restored lines are marked `cached` and stay
until the new search reaches their depth. Changing an engine's options or configuration
forgets its entries; the cache starts over past 50,000 positions.

## Online lookups

`online.rs` queries the ChessTB Explorer tablebase (`POST /probe`, up to 8 pieces) and
chessdb.cn (`queryall`) with blocking `ureq` calls on background tasks. Analyse's Online panel
is the only caller and sends nothing while closed. Lookups wait 300 ms so quick navigation only
queries where it stops, results are cached per FEN for the session, and clicking a move plays it.

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
  Import writes each game's full movetext (comments, NAGs and legal variations) to
  `games.annotated` while parsing, using a position stack per open variation; plain games keep
  it NULL. Only mainline moves go into `moves` and the `positions` index. A variation with an
  illegal move is cut from the text and counted; an illegal mainline move skips the game. The
  source file is kept in `meta` for re-importing.
  Editing or deleting a game removes its position-index rows by recomputing its own hashes, so
  it never scans the whole `positions` table. A database's display name lives in its `meta`
  table; duplicating uses `VACUUM INTO`.
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
Settings exposes storage paths, and a retry control only while a save has failed.
Settings › Data lists each kind of saved data with its files (`storage::STORES`) and
measures their disk use, and the size and free space of the drive holding them, in
`cx.background_spawn` whenever the page opens. A cancelled
PGN dialog does not mutate the game or its saved baseline.

The original Analyse/Play views use `game-recovery.json` and `play-recovery.json`;
additional document tabs use `recovery/analysis-<id>.json` and `recovery/play-<id>.json`.
They are autosaves: Analyse writes its game after every change (an empty board removes the
file), and Play after every move. They are scanned at startup and included in backups, and
store PGN including clock metadata. Startup restores Analyse games silently; an unfinished
engine game opens in Analyse, which keeps it from then on, rather than restarting an engine.
Saving to My games or a PGN file is a separate, explicit copy.
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

## Document tabs

`Workspace` owns a typed list of tab entities and tracks the active tab and last
selected tab per mode. Its public screen fields point at the current entity for
each mode, so existing cross-screen actions target that mode's selected document.
Analyse, Play and Database can have multiple independent entities; Train and
Settings remain singletons. Database/game-to-analysis actions create a new tab.
A game opened from the Database tab keeps a `DbGame` link (file and game id), so
Save in Analyse replaces that stored game instead of adding one to My games. The
mainline goes to `moves` and the position index; the full movetext with comments,
variations and NAGs goes to the `annotated` column, which `GameDetails::to_pgn`
prefers. The scoresheet shows a strip naming the database, whether there are
unsaved changes, and the stored review's findings. The review also fills the eval
graph. Loading any other game clears the link.
The tab strip scrolls horizontally; tab keyboard actions remain available during
text entry without enabling move-navigation keys inside inputs.

Tabs can be renamed (double-click, or the context menu); an empty name restores the
screen's own title. Every change to the open tabs, their order, names or the active tab
writes `config.session`: one entry per tab with its kind, id, name and, for Analyse and
Play, the bare file name of its recovery file. At startup without `--screen`,
`restore_session` rebuilds that list, reusing the screens made at start for the first
tab of each kind, and shows the saved active tab. Without a session, startup falls back
to scanning the recovery directory. Recovery files of tabs closed before quitting are
left on disk but not reopened, and new tab ids start above every id found there so a
new tab never picks up an old file.

Only the visible `LiveAnalysis` searches through the shared engine registry.
Deactivation stops searches and cancels readers while retaining its position cache;
idle registry handles remain available to Settings and log windows. Activation
replaces those handles and restarts from the selected tab's position. Inactive views ignore
late engine events and configuration notifications cannot start background analysis.
Each Play game already owns its own engine and clock. In self-play that one
engine process plays both sides with the shared clock, and the board stays read-only. Open games continue when
switching tabs; closing suspends them and reopening resets the clock tick origin
and resumes the search. Dropping a game quits its process. The last ten closed
entities can be reopened; autosave files keep every document independently.

## Global search

The shell's Ctrl K field shows up to eight local game matches while typing.
After a 200 ms debounce, it searches player names, events and ECO codes in the
registered databases through `db::Database::search_games`. Queries and loading
selected games run in `cx.background_spawn`; replacing the pending UI task
prevents an older query from replacing current suggestions. Arrow keys select a
suggestion and Enter opens it in Analyse. Without a selected suggestion, Enter
keeps the FEN-opening and database player-filter behavior.

## Documentation website

`website/` is a standalone static Vite site with no dependency on the Rust build or app
runtime. It imports the six demo screenshots from `docs/screenshots/`. Vite builds two
HTML entry points: the screenshot gallery (`index.html`) and documentation (`docs.html`).

Guides are Markdown files under `website/content/`, organized by category.
`content/navigation.json` defines category names and page order. The Vite plugin in
`build/docs.js` uses Marked to render guides and inject their articles and sidebar into
the docs HTML template during development and production builds. It validates titles,
unique topic slugs and relative guide links, resolving those links to URL hashes.
Guide titles and section headings have permalink anchors. Section IDs include the topic
slug and disambiguate repeated headings; the browser reveals the containing guide before
scrolling to a linked section.
Content edits reload the development preview; Markdown parsing stays out of the browser.

The docs use categorized sidebar links and hash-based topic navigation. Top-right search
filters the rendered topics locally by all entered words; Ctrl/Cmd+K focuses it and Escape
clears it. There is no backend, account state or database access. Vite emits relative asset
paths so the build works under the GitHub Pages `/fast/` project path or a custom domain.
`.github/workflows/pages.yml` tests and builds the site on relevant pull requests and deploys
`website/dist` to GitHub Pages on pushes to `master` or manual runs.
