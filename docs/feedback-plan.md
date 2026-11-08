# Plan: what En Croissant users ask for

Reddit threads about En Croissant name the gaps that keep people on ChessBase, Scid or Lucas
Chess. Fast already covers many of them: tabs for any screen, saving annotations back to a
database, Play from any position, engine tournaments, clearing arrows with a left click, native
Mac and Linux builds. This plan covers the requests Fast does not meet yet, in the order
they should be built. Comments, variations and NAGs now survive database import.

It refines items 4 and 6 of `docs/roadmap.md`. When an item ships, remove it here and from the
roadmap, and update `ARCHITECTURE.md` where the item changes crates, threads, protocol or data
flow.

| # | Item | Size | Touches |
| --- | --- | --- | --- |
| 2 | Opening tree from any database | S | Analyse, config |
| 3 | Sort the games list by file order and by column | S | `db`, Database tab |
| 4 | Download games from Lichess and Chess.com | L | new app module, `db`, Database tab, Settings |
| 5 | More search filters | M (phase A), L (phase B) | `db`, Database tab |
| 6 | Chess960 in Analyse and Play | L | `chess_core`, `engine`, board, Analyse, Play |

Items 2 and 3 are small and fix the complaints people repeat most. Item 4 comes next because it
needs a scope decision (see its section). Item 5's metadata filters can ship at any time.
Item 6 touches the most code, so it goes last.

## Rules for every item

Each item follows `AGENTS.md`. In particular:

- **Looks right everywhere.** Check every new state in all six palettes, including Paper and
  Sand, and at the 1100×720 minimum window. Take every colour from `ui::T` tokens.
- **Keyboard first.** Every new control is reachable with Tab, works with Enter or Space and
  shows the 2px `accent` focus ring. Dropdowns and menus work with the arrow keys and close
  with Escape.
- **Never blocks.** Database and network work runs in `cx.background_spawn`. Starting a new
  query drops the old task, so stale results never overwrite newer ones. Long tasks show a
  2px progress bar and a Cancel button that works at once.
- **Text.** Wrap all text in `ui::i18n::t()`, add the German strings, use sentence case and
  verbs on buttons, and show missing values as `—`.
- **Demo mode.** `--demo` seeds every new state (annotated games, a remembered Lichess
  account, a Chess960 game, a sorted table) so screenshots and reviews need no real data.
  Update `docs/screenshots/` when a screen changes visibly.
- **Old data keeps working.** Schema changes use the existing `ALTER TABLE` migration pattern
  in `crates/db/src/lib.rs`. Config fields get `serde` defaults, so older `config.toml` files
  still load.
- **Measured, not assumed.** Before an item that touches import or queries ships, time it on a
  database of about one million games (a monthly Lichess dump) and record the numbers in the PR.

---

## 2. Opening tree from any database

**Problem.** `AppModel::reference_db()` picks the first database that is not My games. Users
cannot ask whether they already played a line in their repertoire database or in the database
the open game came from, short of reordering databases.

**What already exists.** The "Moves from here" table has a Reference column and a My games
column. `query_databases` (`crates/app/src/screens/analyse.rs:671`) queries both in one
background task. The `DbGame` link knows which database the open game came from.

### Integration

1. **A choice per tab.** `AnalyseScreen` gets `explorer_db: Option<PathBuf>`.
   `query_databases` uses it instead of `reference_db()`.
2. **Default order.** Use the tab's saved choice first. If there is none, use the open game's
   `DbGame` database. Otherwise use `config.explorer_default` (the last database picked
   anywhere), then the current rule.
3. **Persistence.** Save the choice per tab in `config.session`, and save the last choice in
   `config.explorer_default`. Restore the session choice only if that database still exists.
   Otherwise fall back silently.
4. **Removed databases.** When the chosen database is removed, the column falls back with a
   toast: "Opening tree now uses Masters".
5. **Cache.** Keep a small LRU of `(database, hash) → Vec<MoveStat>` per tab (about 512
   entries), so moving back and forth costs nothing. Clear the entries for a database when
   that database changes (import, edit, delete).

### UX

- **The column header is the picker.** The Reference column's `section_label` becomes a
  compact dropdown: the database name, truncated, with a 12px `chevron-down`, focusable, and
  opened with Enter. The list shows each database with its game count in mono. The open game's
  database comes first, marked "This game's database". The header width stays the same as
  today (`COL_REF`), so nothing shifts.
- **Shortcut.** An action to step through the databases, "Next opening-tree database"
  (unbound by default, configurable in Settings › Shortcuts).
- **Empty states.** "No games reach this position" in `muted` instead of a blank column, and
  `—` in the counts.
- **From the count to the games.** Clicking a move's count opens the Database tab on that
  database, filtered to the position after that move. This uses the existing
  `Filter::position`.

### Verification

- Tests for the default order: a `DbGame` link, a saved choice, a deleted database, no
  databases at all.
- Manual: open a game from My games and check that the tree shows My games without any clicks.
  Switch databases while moving quickly through a game and check that no stale counts flash.

---

## 3. Sort the games list by file order and by column

**Problem.** `filtered_ids` always sorts by `date DESC, id DESC`. Books and tournament files
with one date come out in reverse file order, and no column can be sorted.

### Integration

1. **`db`.** Add `Sort { key: SortKey, descending: bool }` to `Filter`. `SortKey` covers File
   order, White, Black, Result, Moves, Date, Source and Accuracy. Each key maps to a fixed SQL
   expression, so user input never reaches the SQL text. Every order ends with `id` in the same
   direction, so the order is stable.
2. **Indexes.** `date`, `white` and `black` are already indexed, and `id` is the primary key.
   Measure sorting by `plies` and `accuracy` on one million rows. If it takes more than about
   300 ms, add the index in the migration.
3. **Remember per database.** Save the sort per database path in `config` (`database_sorts`).
   New databases default to Date, newest first. A database imported from a single PGN file
   defaults to File order.
4. **Everything follows the table order.** Batch review and "open the next game" (when it
   exists) use the table's current order.

### UX

- **Clicking a header sorts.** Clicking a column heading sorts by it, and clicking again
  reverses it. The sorted heading turns `fg` with a 12px `arrow-up`/`arrow-down` icon in
  `accent`. The others stay `muted`. Headings are focusable buttons (Enter sorts) with the
  focus ring. The icon takes space reserved in advance, so column widths never change.
- **A `#` column for file order.** A narrow first column (48px, mono, `faint`,
  right-aligned) shows the game's number in the file. Clicking its heading sorts by file order.
  Book readers see the game numbers they know from the book.
- **Feedback.** While a large sort runs, the current rows stay visible and a 2px bar runs
  under the header. The new rows replace them in one go.
- **Selection.** The selected game stays selected and scrolls into view after a sort.

### Verification

- `db` tests: every `SortKey` in both directions, ties broken by `id`, and file order
  matching the import order.
- Manual: import a 300-game book, check that the games appear in book order, and switch sorts
  at 1100×720 to check that nothing wraps or jumps.

---

## 4. Download games from Lichess and Chess.com

**Problem.** En Croissant users value automatic downloads of their own games. Fast has no
account import, so users export and import PGN by hand.

**Scope decision needed.** `docs/roadmap.md` describes Fast as offline. Fast already talks to
the network when the user asks: the Online panel, engine downloads and the update check. This
item follows the same rule. It only connects after an explicit click, never at startup, and the
dialog says that it connects to the site. Confirm this before building.

### Integration

1. **New module `crates/app/src/game_download.rs`.** This keeps the network out of `db`. It
   uses blocking `ureq` (already a dependency) on a background task, with a generic
   User-Agent (`Fast chess app (github.com/Disservin/fast)`) and no personal data.
   - **Lichess:** `GET https://lichess.org/api/games/user/{name}` with
     `Accept: application/x-chess-pgn` and `since`, `clocks=true`, `evals=true`,
     `opening=true`. The response streams, so it is passed straight into
     `db::import_games` as its `Read`, with the same progress and cancellation as file
     import. On HTTP 429, wait 60 s, show a countdown and retry once.
   - **Chess.com:** `GET https://api.chess.com/pub/player/{name}/games/archives`, then one
     `/games/{yyyy}/{mm}/pgn` request per month, one at a time. Progress counts months and
     games.
2. **No duplicates.** Add a `site` column (already parsed in `Tags`, not stored yet) and a
   unique partial index on it for URLs (`WHERE site LIKE 'http%'`). The import uses
   `INSERT OR IGNORE` for those games and counts "already in the database" separately from
   "skipped".
3. **Incremental updates.** `config.accounts: Vec<Account { site, name, database, last_game_ms }>`.
   "Update" asks only for games after `last_game_ms`. The value is saved after each committed
   batch, so a cancelled update keeps what it fetched.
4. **Several usernames.** Today `config.player_name` is one string, and Lichess and Chess.com
   names often differ. Change it to `player_names: Vec<String>` (migrating the old value), and
   make `moves_from_player` and the Player filter match any of them. Adding an account offers
   to add its name.
5. **Where games go.** My games by default, so the My games column and repertoire leak
   detection pick the games up at once. Any database can be chosen instead.

### UX

- **Entry point.** The Database tab's Import button opens a small menu: "PGN file…",
  "From Lichess…" and "From Chess.com…".
- **Dialog.** One line: a site `segmented()` (Lichess / Chess.com), Username, and Since
  (All / Last year / Last month / Since last update). Below it, the target database select and
  a `check()` "Include clocks and evaluations" (Lichess only). The primary button is
  "Download games". The dialog states plainly: "Fast connects to lichess.org to fetch the
  games. Nothing else is sent."
- **Progress.** Inside the dialog: games so far in mono, a 2px bar (for Chess.com by months
  done, for Lichess an indeterminate bar), and Cancel. Closing the dialog keeps the download
  running in the background with a status line in the Database tab's databases list.
- **Errors** are shown inline, short, with what to do next: "No Lichess user named
  'magnuss'", "You're offline. Try again later.", "Lichess asks us to wait. Retrying in 42 s."
- **Remembered accounts.** Settings › Databases lists accounts with their last update date and
  "Update" and "Forget" buttons. The databases list's context menu also has "Update from
  Lichess" for a database that has an account. Updates never run on their own.
- **Result.** A toast: "Downloaded 86 new games (412 already in My games)", with a
  "Show games" action that sorts the table by date.

### Verification

- Unit tests with a local `TcpListener` HTTP stub: streamed PGN, 429 then success, an unknown
  user (404), a dropped connection halfway, the archive list parsed, and duplicates ignored on
  a second run. CI never contacts the real sites.
- Manual: download a real account with a few thousand games, cancel halfway, update, and
  check that the counts add up.
- Update `ARCHITECTURE.md` (Online lookups) with the new endpoints and their rules.

---

## 5. More search filters

**Problem.** The filter panel has Player, Colour, ECO, Opponent rating, time class and exact
position. Users comparing Fast with ChessBase and Scid miss date, event and result filters,
and ways to search by material or pawn structure.

### Phase A: metadata filters

1. **`db::Filter`.** New fields:
   - Date from and to: accept `YYYY`, `YYYY.MM` and `YYYY.MM.DD`. Pad them to PGN date
     strings (`2020` from becomes `2020.00.00`, `2020` to becomes `2020.99.99`) so the
     `games_date` index works and `??` parts behave.
   - Event or site contains.
   - Result: Win / Draw / Loss when a player is set, 1-0 / ½-½ / 0-1 otherwise.
   - Player's rating range, next to the existing opponent rating.
   - Moves, minimum and maximum.
   - Only annotated games (item 1).
2. **Saved searches.** A name plus a `Filter` in `config.saved_filters`, listed under the
   Filter heading. One click applies a saved search.

### Phase B: position patterns

- **Pawn structure:** a `pawn_hash` per indexed position (a Zobrist hash of the pawns only)
  answers "same pawn structure" searches.
- **Material:** a compact material signature per position (piece counts) answers searches
  like "rook endings" or "queen against two rooks".
- Both make the `positions` table larger. Make them an import option ("Build structure
  index"), measure the size on one million games first, and build it per database on request
  for existing ones. This overlaps roadmap item 11 and should be designed together with it.

### UX

- **The panel stays compact.** Player, Colour, Result and Date stay visible. The rest sits
  under a "More filters" disclosure, which shows a `badge()` with the number of active hidden
  filters, so no filter is active without the user seeing it. Number pairs are one-line
  `min – max` inputs, side by side. The left column already scrolls; at 1100×720 the visible
  set must fit without scrolling.
- **Live, debounced.** Filters apply 200 ms after typing stops. Above the table: "1,204 of
  48,310 games" in mono, and a "Clear filters" button when any filter is active.
- **Invalid input.** An unreadable date gets a `warn` outline and a short hint under the field
  ("Use 2020, 2020.05 or 2020.05.17"). The filter is ignored until the date is fixed.
- **Keeping results.** When a filter changes, the current rows stay until the new ones arrive,
  with the 2px bar under the header.

### Verification

- `db` tests for each filter on its own and combined, dates with `??`, and Win / Loss from
  both colours.
- Timing on one million games for each filter. An event search that scans the whole table is
  acceptable if it finishes in about a second; record the time in the PR.

---

## 6. Chess960 in Analyse and Play

**Problem.** `GameTree::from_fen` and EPD parsing use `CastlingMode::Standard`, so Chess960
positions cannot be analysed or played, only run in tournaments. Users work around this with
ChessX.

### Integration

1. **`chess_core`.**
   - `GameTree` gets `variant: Variant { Standard, Chess960 }` and keeps its positions in the
     matching `CastlingMode`.
   - `from_fen` reads X-FEN and Shredder-FEN. It picks Chess960 when the castling rights only
     make sense there (king or rook off its standard square, or file letters such as `HAha`).
   - PGN reads `[Variant "Chess960"]` (and `Fischerandom`, `Chess 960`) and writes
     `[Variant "Chess960"]` plus `SetUp`/`FEN` for these games.
   - Add `chess960_position(n: u16) -> Chess` for the standard numbering 0–959 (518 is the
     normal start position), and its inverse for display.
   - `position_hash` already includes castling rights, so the opening tree and position search
     keep working.
2. **`engine`.** A search for a Chess960 game sets `UCI_Chess960 true` first (and `false` when
   the next game is standard), through the existing set-option path, which handles a running
   search safely. Castling goes to the engine as king-takes-rook. Moves coming back are parsed
   with the game's `CastlingMode`. An engine without the option is not sent the search.
3. **Board (`ui`).** In Chess960, castling is entered by moving the king onto its rook, or two
   squares toward it when that is unambiguous. Legal-move dots show the rook's square as a
   target. Animation moves the king and rook to their final squares.
4. **Databases and review.** Import already keeps `start_fen`. Read the `Variant` tag so
   these games load as Chess960. Batch review sets `UCI_Chess960` per game.

### UX

- **Play.** "Start from" gets "Chess960" with a position number input (0–959, mono) and a
  "Random" button with a dice icon. The back rank shows next to it in mono (`#734 RBBKNQNR`),
  so the user knows the position before starting. The last choice is remembered.
- **Analyse.** The new-tab menu gets "New Chess960 game…" with the same number and Random
  controls. A pasted Chess960 FEN or PGN opens without questions.
- **Badge.** A Chess960 game shows a `badge()` "Chess960" in the scoresheet's top strip.
- **Engines.** In the Engine panel, an engine that cannot play Chess960 shows "No Chess960
  support" in `warn` in place of its lines, and the other engines keep running.
- **Opening tree.** Standard reference databases rarely match a Chess960 position, so the
  columns show the normal empty state, not an error.

### Verification

- `chess_core` tests: FEN detection for X-FEN and Shredder-FEN, castling with the king and
  rook next to each other and with the king on its castled square, PGN round trip with the
  `Variant` tag, number 518 giving the standard position, and all 960 numbers giving
  different, legal positions.
- `engine` tests with the fake engine: `UCI_Chess960` is set before the first search and
  reset for a standard game, and castling is sent and parsed in king-takes-rook form.
- Manual: play a few Chess960 games against Stockfish, castling both ways, then analyse them
  and run a batch review.

---

## When an item is finished

- The four CI checks pass (`fmt`, `clippy -D warnings`, `test`, release build).
- The new states are in `--demo` and look right in all six palettes at 1100×720.
- Everything is reachable by keyboard with a visible focus ring.
- German strings are added.
- `ARCHITECTURE.md` is updated if crates, threads, protocol or data flow changed, and the
  item is removed from this plan and from `docs/roadmap.md`.
- The website docs (`website/content/`) get a short guide for user-visible features
  (database import, account downloads, Chess960).
