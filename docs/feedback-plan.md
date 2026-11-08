# Plan: what En Croissant users ask for

Reddit threads about En Croissant name the gaps that keep people on ChessBase, Scid or Lucas
Chess. Fast already covers many of them: tabs for any screen, saving annotations back to a
database, Play from any position, engine tournaments, clearing arrows with a left click, native
Mac and Linux builds. This plan covers the requests Fast does not meet yet, in the order
they should be built. Already shipped from it: comments, variations and NAGs survive database
import; the opening tree can use any database; the games table sorts by file order and by
column; Lichess and Chess.com games download on request; and the Database tab filters by
date, event, result, rating and length, with saved searches.

It refines items 4 and 6 of `docs/roadmap.md`. When an item ships, remove it here and from the
roadmap, and update `ARCHITECTURE.md` where the item changes crates, threads, protocol or data
flow.

| # | Item | Size | Touches |
| --- | --- | --- | --- |
| 5 | Position pattern search (phase B of more filters) | L | `db`, Database tab |
| 6 | Chess960 in Analyse and Play | L | `chess_core`, `engine`, board, Analyse, Play |

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

## 5. More search filters

**Problem.** Users comparing Fast with ChessBase and Scid miss ways to search by material
or pawn structure.

Phase A has shipped: date, event or site, result, player rating, moves and annotated
filters, saved searches, live filtering with "Clear filters", and the "More filters"
disclosure. Phase B remains.

### Phase B: position patterns

- **Pawn structure:** a `pawn_hash` per indexed position (a Zobrist hash of the pawns only)
  answers "same pawn structure" searches.
- **Material:** a compact material signature per position (piece counts) answers searches
  like "rook endings" or "queen against two rooks".
- Both make the `positions` table larger. Make them an import option ("Build structure
  index"), measure the size on one million games first, and build it per database on request
  for existing ones. This overlaps roadmap item 11 and should be designed together with it.

### Verification

- Phase B: measure the index size on one million games before building it, and time each
  pattern search.

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
