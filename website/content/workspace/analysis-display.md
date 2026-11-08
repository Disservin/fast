# Analysis display

## Moves from here

The **Moves from here** table lists engine lines (MultiPV), database moves and repertoire moves.

- **All moves** adds every legal move, like Nibbler.
- **Lines − +** sets MultiPV for every analysis engine and saves it in the engine's options.
  The **Engine** panel sets it per engine.

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
