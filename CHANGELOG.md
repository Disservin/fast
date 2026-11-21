# Changelog

Settings › Changelog shows this file. Describe the finished changes since each release, combining related work into one short line.

## 0.1.4

- Analyse: each engine row shows its node count and hash usage again, next to depth, speed and time.
- Analyse: the Log panel has an engine menu in its header, so you can read any engine's log, not only the one the panels show.
- Analyse: the Explorer keeps the last position's moves on screen until the new position's moves load, instead of collapsing to its column headings.
- Analyse: the engine lines keep their height while a new position waits for its first lines, so the panels no longer jump at each step of backwards analysis.
- Analyse: the panels beside the board sit closer together and closer to the window edge, and meet the board column with a single line.
- Analyse: the game review tiles name their counts ("1 inaccuracy, 2 mistakes").
- Analyse: the Explorer sources and the game review graphs sit in a tab group, with the chosen one raised on an inset track.
- Analyse: backwards analysis for the game review runs one engine, chosen in Settings › Analysis, instead of all analysis engines at once.
- Analyse: backwards analysis shows the depth its node limit reaches, and its results replace deeper evaluations left from earlier searches under another limit.
- Analyse: narrow windows put the engine, explorer and note panels under the board, and screen readers hear names for buttons, tabs, the eval bar and the graphs.
- Analyse: a status bar shows the position's FEN with a copy button and the main shortcuts.
- Analyse: the position note says it is kept for the position in every game and offers quick tags such as Critical and Review later.
- Analyse: the Explorer panel shows moves from a database, your own games or your repertoire with results and average rating, and says where the game left theory when no game reaches the position.
- Analyse: one compact Engines panel lists each engine with its lines, WDL, cloud scores and an add-to-game button, with Online, Log, search settings and pause in its header and the ChessDB and tablebase results in its footer.
- Analyse: the game review shows each side's accuracy with counts of inaccuracies, mistakes and blunders, and the eval graph shades White's and Black's share with marked errors and move numbers.
- Analyse: the bar under the board adds auto-play and a scrubber along the line, and its tooltips name the shortcuts.
- Analyse: the board header says what the evaluation means, the players show a colour swatch, a material badge and Add name, and a legend under the board names each engine's arrow and best move.
- Analyse: the moves have Moves, Notes and Headers tabs, coloured move symbols, muted book moves with a row where the game leaves theory, and a note saying who plays next in an unfinished game.
- Analyse: the annotation toolbar adds Delete from here and shows which symbols the current move has.
- Analyse: a game card above the moves shows both players, ratings, event, date, time control, result and opening, with a form to edit the headers.
- Top bar: the engine button says how many engines are running and opens a menu to pick engines, pause them or open their settings.
- Analyse: group candidates by engine with compact inline PVs, hover previews and a chevron to expand each line.
- Analyse: expanded PVs fit their content without repeating figures or moves, and collapsed lines leave room for the chevron.
- Analyse: Lines, Reset and Max in Search options change only the selected engine.
- Analyse: Bars fills the game graph height so evaluation bars are visible around the centre line.
- Analyse: Line preview keeps its evaluation when the main engine drops the previewed move while a kibitzer is running.
- Board: larger pieces pick up immediately on press, stay where dropped and deselect when clicked again.
- Board: draw arrows and circles in green, red, blue or yellow with Lichess-style modifier keys.
- Analyse: choose Time, Depth, Nodes or Infinite search limits with an editable value.
- Analyse: Cloud scores use White's perspective, matching engine evaluations.
- Analyse: show ! and !? badges on the board; live analysis waits for depth 10 before showing mistake badges, which can be limited to game reviews in Settings › Board.
- Analyse: Line preview draws hover arrows only for the next playable move.
- Play: follow engine matches with per-move depth, nodes, hash use and thinking time, and open either engine's UCI log.
- Play: engine match actions stay visible, New game stops the running match and game setup shows the chosen start position.
- Chess960: Random chooses from all 960 positions, and random colour works correctly on Windows.
- Performance: large PGN imports, database recounts, tournament game loading and backup restores keep the window responsive.
- Interface: clearer table headings, consistent panel headers and missing values, keyboard sorting in tournaments and readable analysis colours in light themes.
- Settings: board and piece set names fit under their previews; sound sets are simplified, with removed sets falling back to Classic.

## 0.1.3

- Play: match two engines with separate strength, threads, hash, clocks and search limits, plus a PGN or EPD opening book.
- Play: follow engine matches with live evaluations, PV previews and graphs for evaluation, nodes, depth and thinking time.
- Play: browse earlier moves with the scrollbar or arrow keys while engines continue playing.
- Play: saved match PGNs include engine names, event, date, move times, remaining clocks and evaluations with depth.
- Play: choose Full strength to play an engine without a rating limit.
- Play and Tournament: set time and increment separately, with selectable units and fractional durations.
- Files: browse for opening books, tournament engines, working folders and Syzygy folders.
- Analyse: use a kibitz engine to explore previewed lines while the main engine analyses the game.
- Analyse: analysis runs for 30 seconds by default, with Infinite available for longer searches.
- Analyse: choose whether background tabs keep analysing, pause or stop, with tab indicators showing their status.
- Analyse: engines stay loaded across tab switches, preserving their networks and hash.
- Analyse: see evaluations in the scoresheet, including imported mate distances, and preserve evaluations and timing when editing PGN comments.
- Analyse: see database move frequencies, game counts and results, and use My moves to show or hide personal move columns.
- Analyse: start Chess960 from the File menu or the position editor.
- Analyse: read engine statistics from more UCI output formats and browse the log without losing your place.
- Analyse: the game graph fills the available space, with square side panels and readable previews in light themes.
- Database: select collections from a searchable toolbar and add local or web collections.
- Database: manage collection descriptions, reference selection, statistics, player aliases, cleanup and removal.
- Database: fold search filters, see the active filter count and find review and export under More actions.
- Database: choose the folder and filename for PGN exports, with a Windows file picker fallback under WSL.
- Train: use a Drill panel beside the board, import from the Repertoire panel and practise listed leaks within the current session.
- Train: the repertoire stays hidden until you answer, with repeated positions after mistakes or reveals and clear grading and session feedback.
- Settings: find each setting on one page, with backup under Data and large engine hash sizes shown in GB.
- Settings: engine checks allow more startup time, explain failures and let you retry with installed files.
- Tabs: close all tabs from the context menu to start a fresh Analyse tab with numbering reset.
- Dialogs: Escape closes dialogs while editing text fields.
- Themes: disabled primary buttons are readable in Paper and Sand, and tournament colours follow the active palette.
- macOS: the title bar is merged into the top bar.

## 0.1.1–0.1.2

- Database: create, rename and duplicate databases, and edit or delete games.
- Database: preserve PGN comments, variations and NAGs, and re-import games from their source file.
- Database: download and update Lichess and Chess.com games, skipping games already stored, with progress and cancellation.
- Database: filter by date, event, result, rating and length, save searches and get player-name suggestions.
- Database: search exact pawn structures and material with an optional, cancellable index.
- Database: sort games by file order or any column, with a highlighted database selection and readable game counts.
- Analyse and Play: start Chess960 by position number or Random, with PGN and FEN support.
- Analyse: use Chess960 castling controls and see warnings for unsupported engines.
- Analyse: choose any database for opening-tree counts, open matching games and save comments back to the source database.
- Analyse: fold the scoresheet and use a fixed-height Line preview with controls that stay in place.
- Play: run engine self-play, start from a pasted FEN and open finished games in Analyse.
- Train: hear board sounds and see clearer feedback for wrong answers.
- Tournament: use Chess960, engine arguments, colour-order options, the SPRT Elo model and tablebase adjudication.
- Tournament: view games file notation and per-move search details, including nodes, depth, speed and PV.
- Tabs: cycle with Alt+arrows as well as Ctrl+Tab.
- Settings: browse the changelog and keep using the app while account downloads start.
