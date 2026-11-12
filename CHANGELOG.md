# Changelog

Settings › Changelog shows this file. Describe the finished changes since each release, combining related work into one short line.

## Since 0.1.3

- Chess960: Random picks any of the 960 start positions, not only every tenth, and Play's random colour is no longer always White on Windows.

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
