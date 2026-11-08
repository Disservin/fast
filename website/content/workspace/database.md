# Database

Each database is listed on the left with its game count.

## Filter games

Filters apply as you type. Player, Colour, Result and Date are always shown; **More filters**
holds the rest and shows how many of them are set.

- **Result** reads Win, Draw and Loss for the player you typed, and 1–0, ½–½ and 0–1 without
  one.
- **Date** takes `2020`, `2020.05` or `2020.05.17` for each end. Games with unknown months or
  days still count inside a year.
- **More filters:** ECO range, time control, opponent rating, the player's rating range, game
  length in moves, text in the event or site, only annotated games, and only games where you
  left your repertoire.
- Typing your player name also finds games under your Lichess and Chess.com names.

Tick **Reached the Analyse board position** to see only games that passed through the position
on the Analyse board.

Above the table, "1,204 of 48,310 games" shows how many games match, and **Clear filters**
empties every filter.

### Saved searches

**Save search** stores the current filters under a name, listed under the Filter heading.
Click one to apply it; right-click it to delete it.

## Sort the games

Click a column heading to sort by it, and click it again to reverse the order. The sorted
heading shows an arrow. The **#** column numbers the games in the order they were imported,
which is their order in the PGN file, so a book's games keep the numbers you know.

- Each database remembers its sort. A database imported from a file starts in file order;
  others start newest first.
- **Analyse all** reviews the games in the table's order.

## Read the results

- The table shows where each game left your repertoire, and its accuracy.
- The panel above the table shows your score and your most common leak.

## Review and export

- **Analyse all** reviews the filtered games with the engine. **Review settings** sets the limits.
- **Export PGN** saves the filtered games.

## Find exact pawn structures or material

Open **More filters**, then paste a **Reference FEN** under **Position patterns**. Choose
**Same pawn structure**, **Same material**, or both. Pawn structure compares the squares
occupied by each colour's pawns. Material compares each colour's piece counts, including
pawns and promoted pieces. Side to move and castling rights do not matter. When both
options are checked, both must match in the same position.

Click **Build structure index** once for an existing database, or enable it in the PGN import
dialog. The index uses extra disk space and is optional. Building it shows progress and can
be cancelled; cancellation keeps the previous complete index. Future imports and game edits
maintain an enabled index automatically. Search results preview and open in Analyse at the
first matching position. You can save these criteria with other filters.

These are exact matches. Similar pawn structures and representative-game ranking are planned
separately. Chess960 PGNs keep their variant during import, export and engine review.
