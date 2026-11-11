# Repertoire training

Train drills your opening repertoire with spaced repetition. It shows a position from your
repertoire, you play the move you prepared, and you say how well you knew it. Moves you know
well come back after longer and longer breaks; moves you miss come back at once.

The tab has three columns: the **Repertoire** on the left, the board in the middle, and the
**Drill** and **Leaks from your games** panels on the right.

## Build your repertoire

Fast keeps one repertoire, played as White. Every line starts from the starting position.

- **Add line** opens a field under the tree. Type the moves, such as `1.e4 c5 2.Nf3 d6`, and
  press `Enter`. Move numbers are optional. A line with an illegal move is not added, and
  Fast says which move was wrong.
- **Import PGN** adds every line of a PGN file, variations included, and says how many lines
  it added.
- **Find gaps** tells you how many lines end on an opponent's move with no answer from you,
  and gives one as an example.

Lines that share their first moves are merged into one tree. A position reached by two move
orders counts once, so you learn its move once.

Only your own moves are drilled. The opponent's moves decide which positions you are asked.

## Read the Repertoire panel

The heading shows the number of positions. The bar under it shows how much of the repertoire
is known, being learned and new.

Each row of the tree is a line. Moves with no alternatives are joined into one row, and a
branch is indented under the move it starts from. The tag on the right is the status of the
line, taken from its most urgent move.

| Status       | Meaning                                                           |
| ------------ | ----------------------------------------------------------------- |
| **New**      | Never drilled                                                     |
| **Learning** | Answered **Repeat** last time, or coming back in less than 7 days |
| **Due**      | Its day has come, so it is in today's session                     |
| **Known**    | Coming back in 7 days or more, and not due yet                    |

While a position is being asked, the tree is hidden so it cannot give the move away. The
panel shows the moves that lead to the position instead, and the tree returns once you answer.

## Drill a session

A session holds every move that is due, the longest overdue first, followed by every new
move in the order of the tree. Above the board you see how many positions are completed and
how many remain, how many of those are due and new, and a progress bar.

1. The board shows a position with you to move. The last moves are written beside
   **Your move as White**.
2. Play your prepared move on the board.
3. If it is right, the Drill panel says so and offers the grades. If it is not in your
   repertoire, Fast says so and you try again.
4. Choose a grade. The next position appears.

The Drill panel also has these buttons while a move is asked:

| Button          | What it does                                             | Effect on the grade           |
| --------------- | -------------------------------------------------------- | ----------------------------- |
| **Hint**        | Names the piece to move                                  | **Hard** is the highest grade |
| **Show answer** | Draws the move as an arrow                               | Only **Repeat** is offered    |
| **Skip**        | Moves the position to the end of the session, ungraded   | None                          |

A wrong move also leaves only **Repeat**, even if you find the right move on the next try.

After you answer, **Why this move?** shows the note saved for the move. For a leak it says
what you played instead, and in how many games.

`F` flips the board.

The session is saved as you go. Close Fast or switch tabs, and it continues where you
stopped. **Start fresh** discards the session and builds the queue again. The number of
completed reviews is shown beside it.

When no position is left, the tab says whether the session is complete, nothing is scheduled
today, or the repertoire is empty.

## Grade your moves

Each grade button shows in how many days the move will come back, such as **Good (4d)**.

| Grade      | Use it when                    | A new move comes back | Later the break becomes                |
| ---------- | ------------------------------ | --------------------- | -------------------------------------- |
| **Repeat** | You did not know it            | Later in this session | Starts again from the first step       |
| **Hard**   | You found it with effort       | In 1 day              | 1.2 times longer, at least 1 day more  |
| **Good**   | You knew it                    | In 4 days             | Multiplied by the move's ease          |
| **Easy**   | You knew it without thinking   | In 11 days            | Multiplied by the ease, then by 1.3    |

Each move has an ease, which starts at 2.5. **Easy** raises it by 0.15, **Hard** lowers it by
0.15 and **Repeat** lowers it by 0.2. It stays between 1.3 and 3.0. A move you always grade
**Good** comes back after 4, 10, 25 and then 63 days.

## Any reply or every reply

The mode button in the Repertoire panel decides what happens where you prepared more than
one move for the same position. Changing it starts the session again.

- **Mode: any prepared reply** asks each position once and accepts any of your prepared
  moves. The move you play is the one that is graded.
- **Mode: every prepared reply** asks for each prepared move in turn. If you play another
  prepared move, Fast asks you to try again and does not count it as a mistake.

## Leaks from your games

A leak is a move of yours, in one of your own games, that left the repertoire in a position
where you had a move prepared. Fast finds leaks in the **My games** database, in the games
you played as White. It needs your games and your name:

- [Import](../getting-started/import.md) or [download](../getting-started/download.md) your
  games into **My games**.
- Set your player name under **Settings › Databases**.

The panel lists the two most frequent leaks that happened in at least two games. Each row shows:

- The move you played and the move you prepared, as in `6.Be2 for 6.Be3`.
- The number of games.
- Your score with the move you played against your score with the prepared move, when you
  have games with both. Otherwise it shows the moves that led to the position.

A row marked **Not in repertoire yet** is different. It is an opponent's move your repertoire
has no answer to, met in at least five games. Add a line to cover it.

**Drill these lines** puts the listed leaks next in the session. Each one asks for your
prepared move in the position where your games left the repertoire. The rest of the session
stays queued behind them.

## Your repertoire in other tabs

- **Analyse:** the **Moves from here** table has a Repertoire column. See
  [Analysis display](analysis-display.md#moves-from-here).
- **Database:** the table shows where each game left your repertoire, and a filter keeps only
  those games. See [Database](database.md#filter-games).
- **Play:** choose **Repertoire line** under **Start from** to begin a game at the end of your
  repertoire's main line. See [Play against an engine](play.md).
