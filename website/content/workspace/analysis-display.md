# Analysis display

## Moves from here

The **Moves from here** table lists engine lines (MultiPV), database moves and repertoire moves.

- **All moves** adds every legal move, like Nibbler.
- **Lines − +** sets MultiPV for every analysis engine and saves it in the engine's options.
  The **Engine** panel sets it per engine.

## Opening tree

The Reference column counts the games in one database that reached the position, with the
score for the side to move. Its heading names the database; click it, or focus it and press
Enter, to count another one. The list puts the database of the open game first.

- Each tab keeps its own choice, and Fast reopens tabs with it.
- A tab that has not chosen uses the database the open game came from, then the database you
  picked last in any tab, then the first database that is not My games.
- **Next opening-tree database** in **Settings › Shortcuts** steps through the databases. It
  has no key until you give it one.
- Click a count to open the Database tab on that database, showing the games that reached
  the position after that move.
- Only moves actually played count. Moves that appear only in variations do not.

## Arrows

**Arrows** draws one arrow per engine line, coloured by move quality:

| Colour | Quality    |
| ------ | ---------- |
| Blue   | Best       |
| Green  | Good       |
| Yellow | Inaccuracy |
| Orange | Mistake    |
| Red    | Blunder    |

Click an engine arrow to play its move, as in Nibbler. Clicking one of your pieces still selects
it. Turn this off under **Settings › Board & pieces**.

## Analyse backwards

**Analyse backwards** searches every position from the last move to the first, like Nibbler,
so each search benefits from what the engine learned about the moves after it.

- Start it above the graph or from **Engine › Analyse game backwards**.
- Each position gets the search limit, or 3 seconds when the limit is infinite.
- Moving elsewhere or pressing **Stop** ends the run.

## Remembered lines

- Lines are remembered per position, so stepping back and forth keeps them.
- A position reached by another move order (a transposition) shows what was already found.
- Stepping forward returns into the variation you came back from.

## Graph

The graph under the board has four tabs:

| Tab       | Shows                                                         |
| --------- | ------------------------------------------------------------- |
| **Eval**  | Evaluation on a symmetric log scale                           |
| **Time**  | Time per move, from `[%clk]`, `[%emt]` or TCEC `mt=` comments |
| **Depth** | Search depth per move                                         |
| **Bars**  | The original timeline                                         |

Annotated games (`[%eval]`, TCEC `wv=` and `d=`) fill the graphs without an engine.

## Scoresheet

The scoresheet switches between two layouts:

- **Table**: Lichess style.
- **Inline**: Nibbler style, with each variation depth in its own colour.
