# Architecture

## Crates

| Crate | Depends on GPUI | Responsibility |
| --- | --- | --- |
| `chess_core` | no | Game tree (mainline, variations, NAGs, comments), PGN in/out, notation, opening table |
| `engine` | no | UCI parser, supervised engine processes, analysis events |
| `db` | no | One SQLite file per database, streaming PGN import, filters, position search, editing and deleting games, copying a database, notes |
| `trainer` | no | Repertoire tree, SM-2 scheduler, leak and gap detection |
| `ui` | yes | Design tokens and shared views: `BoardView`, scoresheet, timeline, buttons |
| `fast` (`crates/app`) | yes | Window, routing, shortcuts, screens, configuration, demo mode |

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

## Module layout

Each screen is a root file and a folder of the same name. The root file (`screens/database.rs`)
holds the screen's state struct, its constructor, its `Render` impl and the module list. Each file
in the folder (`screens/database/filter.rs`, `import.rs`, `table.rs` and so on) adds an
`impl DatabaseScreen` block for one concern and starts with `use super::*;`. The root file
imports the items its submodules share, so siblings reach each other through it. Tests sit in
the file whose code they cover. The shell follows the same pattern (`shell.rs` and `shell/`:
actions, tabs, tab bar, session, top bar, window chrome).
Large modules outside the screens are split the same way: `ui::board` (arrows, input, moves,
pieces, render), `fastchess` (files, setup, command, output, process) and `live` (cache,
engines, search) and `demo` (games, repertoire, engines).

Code shared across screens lives outside them:

- `ui::widgets`: `btn`, `stepper`, `sort_heading`, `dialog`, `panel_header`, `segmented`,
  `check`, `meter` and the other design-system controls. `Listen` turns a view handle into
  widget handlers (`this.listen(|screen, window, cx| ...)`), and `TextExt::text_as` sets text
  size and colour together.
- `controls.rs`: styled inputs, time and clock inputs, `toggle` (a checkbox bound to a
  setting) and `Rename`, the inline rename field used by tabs and databases.
- `event_reader.rs`: `spawn_batched`, the loop that takes engine or test-suite events off a
  channel in batches and applies them to a view, and `spawn_with_progress`, which runs a
  download on a background thread and shows its progress messages.
- `model.rs`: `ToastExt` and `ConfigExt` show a toast or change and save the settings through
  a handle to the model.
- `format.rs`: counts, byte sizes, durations and the current time.
- `review.rs`: engine review of stored games (accuracy and findings), used by Database and
  Analyse. It has no GPUI code.

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

On macOS the title bar is transparent with its title hidden, and the top bar extends under
it. The traffic lights are moved into the top bar (`TRAFFIC_LIGHTS_AT`), centred at its left
end, and the bar's content starts after them except in full screen, where macOS hides them.
The empty bar drags the window with `start_window_move`, and a double click runs
`titlebar_double_click`, which zooms or minimizes as set in System Settings.

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

- Settings probes allow 60 seconds for first launch, stop the probe process on success or
  failure, and report recent engine output on failure. Catalog retries reuse installed files.
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
- `info` lines are read field by field in any order. Unknown fields (newer protocol
  extensions, engine-specific ones) are skipped up to the next known field, the PV ends
  at the first token that is not a UCI move, decimals are rounded, and a field that
  cannot be read is dropped instead of the line. A line without a PV that still carries
  nodes, speed, time, hash use or tablebase hits is search progress: it arrives with an
  empty `pv`, and Analyse merges its numbers into the last line of that `multipv`. Only a
  line with neither is ignored.

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

## Kibitz engine

With Settings › Analysis › Kibitz engine on, clicking a move in Line preview no longer moves
the main analysis. Each Analyse tab owns a `Kibitz` (`kibitz.rs`): its own process of the
chosen engine (or the first analysis engine), started on first use with the engine's analysis
options plus `MultiPV 1` and the kibitz's Threads and Hash (1 and 16 MB by default), outside
the shared registry. It searches the previewed position (`go infinite`) and its best line is
shown in a Kibitz row under the line. While the preview lasts (`pv_kibitz`), `preview_moves` is
empty, so the main search, the table, explorer and online lookups stay on the game position;
the board shows the preview with the kibitz's best move as its only arrow. The previewed line
is a copy and the table's selection is held, so the main engine's changing lines replace it
only once the preview ends. Leaving the preview or the tab stops the kibitz; the process is
replaced when its engine, binary, threads or hash change, and quits when the tab closes.

## Analysis cache

`LiveAnalysis` keeps a `LineCache` of each engine's lines per position hash (Zobrist), so lines
survive navigation and transpositions share them. Restored lines are marked `cached` and stay
until the new search reaches their depth. Changing an engine's options or configuration
forgets its entries; the cache starts over past 50,000 positions.

Each Analyse tab also keeps a `MoveCache` (`explorer.rs`) of its opening-tree answers, keyed by
database, position hash and player, holding the 512 most recent. Only answers missing from it
are queried, in one background task. Any `ModelEvent::Databases` (import, edit, delete, rename)
clears it. `explorer::pick_database` chooses the Reference column's database: the tab's
choice, the `DbGame` database, `config.explorer_default`, then `AppModel::reference_db()`.
The Database tab beside Engines and Search options renders these same answers through
`analyse/database_explorer.rs`: all recorded continuations in popularity order, game
frequencies, White/draw/Black result shares and totals. Unfinished games stay separate
from draws. Changing the picker updates both views. Uncached queries clear stale rows,
show a loading state and report lookup errors without caching them; clicking a move
adds it to the analysed line and advances the board.

The Database screen uses a searchable collection picker with Add database and Manage
in a compact header. The picker follows collection changes and selections made by
imports or restored tabs. A separate games toolbar shows matching counts, toggles
the filter sidebar with an active-group count, and offers import. Review/export,
rename, duplicate, re-import and account updates live under More actions. Score and
opening leak use one compact summary row. The optional preview stays beside the table.
File selection first uses GPUI's platform-native picker. If the Linux desktop portal
is unavailable, `file_dialog.rs` runs a native alternative on a background worker:
Windows common dialogs through PowerShell under WSL (with `wslpath` conversion), or
Zenity/KDialog on Linux. Cancel returns no paths. The typed-path fallback is used only
when native alternatives fail; open, save, folder and multi-file requests share this flow.
Database exports use `model::save_pgn`: Save As suggests a portable filename from the
collection name and lets the user choose the folder and filename.
The Windows fallback uses a PGN file filter and confirms overwrites.

Database collection management lives in `screens/database/management.rs`. It loads
collection statistics, saves names/descriptions in SQLite `meta`, merges exact player
names and previews duplicate or empty-game IDs on background workers. Cleanup uses
the existing deletion confirmation and transactional index/review cleanup; duplicates
with different annotations remain distinct. Reference selection saves
`config.explorer_default`. Whole-collection export ignores the table's active filters.
`database_sources.rs` installs local PGN, gzip/ZIP archives and consistent read-only
snapshots of Fast SQLite files. Web installation fetches an explicit HTTPS PGN URL or
resolves the latest issue from the TWIC publisher. Streaming downloads and imports
report progress and accept cancellation; only a complete checkpointed temporary
database is published to the database directory. Normal use stays offline until an
installation is requested. `AppModel::remove_database` serves both Settings and the
management dialog, preserving removed files in the recoverable `removed` folder.

## Online lookups

`online.rs` queries the ChessTB Explorer tablebase (`POST /probe`, up to 8 pieces) and
chessdb.cn (`queryall`) with blocking `ureq` calls on background tasks. Analyse's Online panel
is the only caller and sends nothing while closed. Lookups wait 300 ms so quick navigation only
queries where it stops, results are cached per FEN for the session, and clicking a move plays it.

`game_download.rs` downloads a player's games, only after the user clicks Download games or
Update. It sends the username and a `since` time and nothing else, with the User-Agent
`Fast chess app (github.com/Disservin/fast)`.

- **Lichess:** `GET /api/games/user/{name}` with `Accept: application/x-chess-pgn`,
  `sort=dateAsc`, `clocks`, `evals`, `opening` and `since`. The streamed body is the `Read`
  passed to `db::import_games`, so progress and Cancel work as for a file. Oldest-first order
  means the newest committed game is always safe to resume from.
- **Chess.com:** `GET /pub/player/{name}/games/archives`, then each month's `/pgn` one at a
  time, from the month of the last game on. Only archive links on the API host are followed.
- The dialog enters Connecting immediately. Worker events distinguish fetching the archive
  list, receiving games and saving them, before the first import batch has committed. Both
  dialog and banner acknowledge Cancel immediately while an outstanding request unwinds.
- An HTTP 429 waits 60 s with a countdown and retries once; 404 means no such user.
- Downloads import with `skip_known_urls`: a game whose Site or Link URL is already stored
  (the `games.site` column with a partial index) is counted as already there, not stored
  again. File imports keep every game.
- `config.accounts` remembers site, name, database and the newest game's start time, saved
  after every committed batch, so Update asks only for newer games. Account names are added to
  `config.player_aliases`, which My games, the opening tree, leak detection and the Player
  filter match together with `player_name`.

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
  table; duplicating uses `VACUUM INTO`. Settings › Databases edits the aliases used
  by My games and leak detection. Removing a reference database checkpoints its WAL
  in a background task, moves the file into `databases/removed`, clears linked accounts
  and saved database preferences, then emits `ModelEvent::Databases`. My games is protected.
  The Database sidebar can create an empty
  database on a background task, reserving a unique file with `create_new`, then selecting
  it and opening the inline name editor.
- **Games table:** ids for the filter are loaded once and rows are paged on demand as
  `uniform_list` reveals them, so memory and render cost follow the viewport, not the row count.
  `Filter::sort` picks a fixed SQL expression per `SortKey` and always ends with `id` in the same
  direction; ascending `id` is file order. The sort is saved per database in
  `config.database_sorts`. With any filter set the sort column is written `+column`, so SQLite
  scans the table and sorts the matches instead of walking the sort index row by row (0.27 s
  against up to 5 s on a million games). Filters run 200 ms after the last change; while a
  query runs the old rows stay and a 2px bar shows.

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

- **Tablebases:** `SyzygyPath` is passed to engines, but there is no built-in WDL/DTZ badge, so
  local tablebase probing is not exposed in Analyse.
- The Line preview explanation is a hook with no content outside demo mode.

## Recoverable work and study

Profile and PGN replacement stages a file beside its destination, syncs it and atomically
replaces the destination. A `.previous` sibling retains the preceding version; config and
repertoire saves preserve only valid previous data and startup falls back to that version.
Config serialization, replacement and disk flushes run on `background_spawn` workers.
`ConfigWriter` serializes those writes; revision tokens skip stale queued snapshots and keep
an older completion from clearing the unsaved state of newer settings. Resize debounce also
invalidates the previous revision immediately. Shutdown awaits a save of the latest snapshot;
profile restore invalidates pending writes and shares the writer lock before replacing files.
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
Analyse keeps the best eval known for each move (from the game's own comments, a
stored review, or the engine, whichever searched deepest, with mate distances
when known). The scoresheet shows it beside each mainline move. Save, Save as,
Copy PGN and Copy PGN from here write it into the move's comment as
`[%eval 0.35,24]` or `[%eval #-3,30]`, replacing an older, shallower `[%eval]`.
The tab's own game is not changed, so analysing alone never marks it unsaved.
The tab strip scrolls horizontally; tab keyboard actions remain available during
text entry without enabling move-navigation keys inside inputs.

Tabs can be renamed (double-click, or the context menu); an empty name restores the
screen's own title. Every change to the open tabs, their order, names or the active tab
writes `config.session`: one entry per tab with its kind, id, name and, for Analyse and
Play, the bare file name of its recovery file. Analyse tabs also save their opening-tree
database, restored only if that database still exists. At startup without `--screen`,
`restore_session` rebuilds that list, reusing the screens made at start for the first
tab of each kind, and shows the saved active tab. Without a session, startup falls back
to scanning the recovery directory. Recovery files of tabs closed before quitting are
left on disk but not reopened, and new tab ids start above every id found there so a
new tab never picks up an old file.

Only the visible `LiveAnalysis` searches through the shared engine registry.
Deactivation only sends `stop` and cancels readers while retaining its position cache;
the processes keep running, so networks and hash survive a tab switch, and idle
registry handles remain available to Settings and log windows. Activation attaches
readers to the running processes (quitting those no longer used for analysis; the
registry replaces a process whose binary changed) and restarts from the selected tab's
position. Search ids come from one global counter, so a late event from the previous
tab's search never matches the new tab's. Inactive views ignore
late engine events and configuration notifications cannot start background analysis.
Settings › Engines › Leaving an Analyse tab chooses what a hidden tab does: keep
analysing (`analyse_in_background`, only while a tab other than Analyse is shown),
pause (`pause_in_background`: `set_paused` before deactivating, so it keeps its lines
and stays paused on return, marked with a pause icon in the tab strip) or stop (the
search restarts when the tab is shown again).
Tabs that are not shown carry an `accent` dot while engines work for them: an Analyse
tab analysing in the background, a Play game whose engine is thinking, a Database review,
a running tournament or test-position run. `Workspace` observes every tab entity and
redraws the strip only when a tab's mark changes.
Closing all tabs preserves analysis snapshots and suspends Play games, then opens a fresh
Analyse tab numbered one. Reopening closed tabs assigns new numbers after a reset.
Recovery file numbers advance independently of visible tab numbers to keep snapshots separate.

Each Play game already owns its own engine and clock. In an engine vs engine game
each side has its own process, options and strength limit (the same engine may be
picked twice), and the board stays read-only. Each best move starts the next side’s
search. Play retains each side’s latest analysis, normalises scores to White and
converts PVs from the searched position to SAN. Each engine match move stores standard
`[%emt]`, optional `[%clk]` (for game clocks), and `[%eval]` with depth in its tree
comment before recovery or PGN export. Only scores from that move’s search are used.
PGN annotation parsing retains signed mate distances alongside graph centipawns;
Analyse restores those distances for move-list labels and subsequent export. Starting a match folds setup away
and places moves and both search lines beside the board. The move list scrolls
independently and follows new moves with a visible scrollbar. Play routes move
navigation shortcuts to a separate review cursor, keeping the board and move
highlight together without changing the live game or engine searches; returning
to the latest move resumes following. Each PV move keeps its resulting position
for a read-only hover board. Match starts save both sides’ typed time values, units and search limits
in `config.play_engine_times`, which seeds future match setups. Match setup overrides each engine’s
Threads and Hash within its advertised UCI bounds. Clockless games use a fixed
node or depth limit per move and run no clock task. A local PGN or EPD book loads
on the background executor; its first game or position seeds the game tree. Open games continue when
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

## Chess960 and exact position patterns

`chess_core::Variant` belongs to each `GameTree`; its positions use the matching Shakmaty
castling mode. FEN import detects X-FEN and Shredder-FEN castling rights. PGN import also
honours the Variant tag, including when castling rights have disappeared. Chess960 PGNs
always carry Variant, SetUp and FEN. Standard numbering and its inverse live in
`chess_core::chess960`; the last chosen number is saved in config.
File › New Chess960 game (Ctrl/Cmd Shift N) opens a new Analyse tab on a random start
position, and the position editor's Chess960 switch gives any set-up position Chess960
castling, taking each side's outermost rooks beside the king.

Analyse, Play, Test positions and database review set `SearchRequest::chess960`. The engine
supervisor checks the advertised option, sets UCI_Chess960 before searching, synchronises
with isready, and resets the option before a subsequent standard game. Move histories and
root restrictions use the game's castling notation; returned moves are resolved against
its positions. The board favours an ordinary king step when a castling destination is
ambiguous; dropping the king on its rook always selects castling. Board animations move both
king and rook to their final squares, including when either piece stays put. The position
editor retains the variant and original rook squares. Databases migrate a
`games.chess960` flag so export and review preserve the variant even without castling rights.

`chess_core::PositionPattern` contains a pawn-only Zobrist hash and a 60-bit signature of
piece counts by colour (six bits for each non-king role). These ignore turn, castling and
move counters. The optional `structure_positions` table stores game id, ply and both
signatures; consecutive unchanged patterns share their first ply. Two secondary indexes
cover pawn and material searches. Both enabled criteria must match the same indexed row.
Normal databases leave this table empty. This keeps the existing positions table and
ordinary import costs unchanged apart from the empty schema.

File import can enable the index, and indexed databases maintain it during later imports,
including account updates. Replacing or deleting games updates the index in the same
transaction. Building it for an existing database runs on `background_spawn`, reports
progress through a channel, and checks cancellation for every position. One transaction
publishes a complete index or rolls back; readers retain the previous complete index under
WAL. Closing the tab cancels the build. Exact-search fields persist in saved searches;
matching games preview and open at the first matching ply. Approximate structure matching
and model-game ranking remain roadmap work.

The Analyse board column fills its available height. The game graph below the board
and navigation grows into the remaining space, retaining a minimum height when
the column needs to scroll. Eval, time, depth and bar views share this layout;
tournament graphs keep their explicit heights.


## Train recall phases

Train hides the repertoire tree during recall and displays only the path to the
question, because tree row labels can include future moves. After a correct answer
or reveal, the tree and learning notes become available. The recall controls
(Hint, Show answer, Skip) and the trainer's message sit in a Drill panel at the
top of the right column, above Leaks, so the board takes the full height of its
column. Once answered, the recall controls are replaced by grading controls: mistakes and reveals allow only Repeat, while hints
cap the available grades at Hard, matching the scheduler's evidence rules.
Progress counts finished queue entries, excluding the unanswered question and
entries awaiting repetition. An exhausted queue distinguishes a completed session
from an empty repertoire or a day with no scheduled positions; the persisted
session counters preserve this distinction after restarting.
Drill these lines practises the leaks the Leaks panel lists (the first two seen in
at least two games) that have a prepared move. They go to the front of the current
session; queued drills for the same position and answer are dropped so nothing is
asked twice, and the rest of the session stays behind them.

Play and Tournament share `controls::ClockInput` and `TimeInput`: separate base time and
increment fields with unit selectors. Decimal values and explicit unit suffixes are parsed
as integer milliseconds; Play passes those directly to its clocks and UCI requests, while
Tournament formats exact decimal seconds for fastchess.

Engine matches in Play keep clock increments and optional per-move depth or node limits
separately for White and Black. The clock task skips a side with an unlimited clock,
and each search uses the moving side’s limit or both sides’ remaining clocks and increments.

Shared dialogs use `ui::widgets::modal` for initial focus, Escape dismissal and
backdrop clicks. Each caller supplies its close callback; dialog dismissal does not
depend on the workspace knowing which screen owns it.

Engine matches in Play retain per-move node counts in `[%nodes]` PGN comments alongside
evaluation, depth and elapsed time. The Eval, Nodes, Depth and Time graph tabs read those annotations
from the current game path, with one series per engine and scrubbing linked to the board.

Play writes event, site, UTC start date and participant headers when creating a game,
then refreshes engine participants from each process’s UCI identification. Recovery
and Analyse use the same game headers.
