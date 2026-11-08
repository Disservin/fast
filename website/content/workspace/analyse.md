# Engine controls

## Engine panel

Click **Engine** at the top of the Moves table to:

- switch engines on or off;
- change Lines, Threads, Hash and the search limit.

Changes apply while the search runs, without restarting the engine.

## Log panel

**Log** shows the raw UCI traffic and stderr. Tick **Info** to include search output.

## Line preview

| Do this           | Result                                                            |
| ----------------- | ----------------------------------------------------------------- |
| Hover a move      | Shows it as an arrow                                              |
| Click a move      | Goes there; board, engine and table follow, the game is unchanged |
| Click a table row | Continues the line that way                                       |
| Drag across moves | Highlights them as arrows                                         |
| `Ctrl/Cmd+C`      | Copies the highlighted moves                                      |
| `Esc`             | Returns to the game                                               |

## Analyse Chess960

Open the **New tab** picker, choose a position number (0–959) or **Random**, then click
**New Chess960 game…**. The back rank is shown beside the number. You can also paste an
X-FEN, Shredder-FEN or PGN with a Chess960 Variant tag. Games keep their variant when saved.

The scoresheet shows a **Chess960** badge. Engines without the UCI_Chess960 option show
**No Chess960 support**; compatible engines continue analysing. To castle, move the king
onto the rook, or use its final square when that move is unambiguous.
