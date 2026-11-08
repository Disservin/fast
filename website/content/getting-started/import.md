# Import games

## Build a database

1. Select the database to import into on the left.
2. Open **Database › Import**, leave **Keep comments and variations** on, and choose
   **Choose PGN file…**.
3. Set your name under **Settings › Databases** so Fast can identify your games.
4. Open any database game to analyse it in its own Analyse tab.

## Comments, variations and NAGs

Imported games keep their comments, side variations and move symbols such as `!?`. The games
table marks these games with a speech-bubble icon next to White, the preview shows their
comments and variations, and opening a game in Analyse shows the full tree. Clock and
evaluation commands such as `[%clk 0:03:00]` are kept for Analyse but hidden in the preview.

- Only the moves actually played count in the opening tree and in position searches. Moves
  that appear only in variations do not.
- A variation with an illegal move is left out and the rest of the game is kept. The summary
  after the import counts these, and **Show details** lists the games by their number in the
  file.
- A game with an illegal move in its main line is skipped.
- Turn the option off for very large reference files where database size matters. Games
  without annotations take no extra space either way.

To add comments to games imported before this, right-click the database and choose
**Re-import from file…**. This replaces its games with a fresh import of the file it came
from, so edits and reviews of those games are lost.

## Open and save single files

| Menu     | Use it to                      |
| -------- | ------------------------------ |
| **File** | Open or save PGN and FEN files |
| **Edit** | Copy or paste PGN and FEN      |
