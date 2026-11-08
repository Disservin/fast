# Fast: additions and changes

A standalone product backlog for the native, offline chess workstation. Priorities reflect study correctness and daily usefulness. Completed roadmap work is removed; remaining entries are proposals.

## First: improve the daily workspace

1. **Add — workspace restoration and recent games.** Remember the active screen, open study documents, selected positions, orientation and panel sizes. Restore saved engine selections, with an explicit preference controlling whether computation restarts. Keep recent games easy to find.
2. **Change — make navigation and editing predictable.** Add a clear branch picker, searchable move/variation navigation and inline comment editing. Keep text-entry focus separate from move-navigation shortcuts. Offer undo for destructive edits and a way to return from exploratory analysis to the original game.
3. **Change — make layouts work across window sizes.** Save panel sizes per screen, provide a reset action, and allow panels to stack when space is tight. Keep the board square, tables aligned and essential actions reachable with keyboard navigation and visible focus.
4. **Change — make PGN imports tolerant and transparent.** Accept unknown ratings, common encoding artifacts and valid annotation variants. Report imported, skipped and failed games with useful diagnostics; cancellation must retain committed work and report partial completion accurately.
5. **Add — import deduplication and database cleanup.** Detect exact repeated games first. Offer optional prefix/metadata-tolerant matching, preview proposed merges, preserve annotations and allow the user to choose which version survives. Never silently delete games based on a fuzzy match.
6. **Add — database management tools.** Merge selected databases, save filters and manage reference collections. Keep searches, imports and large result lists responsive; verify this with representative large datasets before choosing optimizations.

**Completion checks:** a reopened workspace returns to the same study context; cancelling an import reports exactly what was retained; deduplication offers a reviewable preview; common tasks remain reachable in a narrow window.

## Later: expand offline study and preparation

7. **Add — a repertoire library.** Manage separate named repertoires for either side, choose the side when creating or importing one, and practise a selected branch or a full line. Show coverage and gaps using shared position knowledge.
8. **Add — personal puzzle sets.** Create exercises from reviewed mistakes and imported puzzle files. Require validated solutions, allow post-exercise analysis, and save session history. Support repeatable fixed-set cycles with time and accuracy comparisons.
9. **Add — guess-the-move training.** Hide the next move in a selected game, let the user choose a side, and optionally accept engine-supported alternatives. Explain scoring and resume unfinished sessions.
10. **Add — opening and player reports.** Summarize candidate moves, results, opponent strength and trends from local databases. Show sample sizes, filters and the underlying games so users can assess the statistics.
11. **Add — model-game and structure search.** Find representative games from a position or matching pawn structure. Make exact and approximate matches distinguishable and allow users to open results directly in analysis or training.
12. **Add — local endgame guidance.** Display tablebase outcomes and distance-to-zeroing information when suitable local files are configured. Explain unavailable coverage and distinguish theoretical results from practical fifty-move-rule outcomes.
13. **Add — engine opening books.** Configure books per engine, book depth and move variety for engine practice. Practice games already use separate tabs and have recovery snapshots; workspace restoration across app restarts remains on the workspace roadmap item.
14. **Add — persistent exploration trees.** Let users queue candidate positions, deepen selected branches and resume exploration across sessions. Make CPU, memory and disk budgets explicit. Evaluate parallel workers only after measuring throughput and preserving a clear analysis-quality policy.

## Release readiness

15. **Change — validate native distribution on each supported platform.** Exercise installation, first launch, file dialogs, audio, engine startup, high-DPI layout and keyboard navigation on Windows, macOS and Linux. Linux development checks exist; complete equivalent checks on Windows and macOS and publish clear compatibility requirements, signed packages where available and a manual offline update path.
16. **Add — localization support when the core workflow is stable.** Move visible strings into translation resources and localize dates, numbers and review intervals. Keep notation and shortcuts readable across languages and longer labels.

## Existing foundations to extend

Fast already provides multiple analysis engines, saved engine profiles, configurable shortcuts, durable analysis records keyed by game history and engine settings, backwards analysis, graph views, streaming cancellable imports, paged database rows, configurable and resumable batch review with per-position results and both players’ summaries, review findings with engine lines, repertoire imports, spaced repetition, repertoire leak detection, engine play, premoves, configurable board/piece appearance, selectable move sounds and independent workspace tabs. These should be extended and verified rather than rebuilt.

The source shows several useful boundaries for the work above:

- Live analysis now has durable records keyed by full game move history, engine identity/options and search limit. Stale records are labeled and can be explicitly replaced.
- Batch review can be limited by time, depth or nodes; it honors custom starting positions and resumes from completed per-position scores.
- Repertoire analysis has a position index, while practice cards, drill queues, gap traversal and grading remain attached to move-tree paths.
- Practice currently compares the answer with one queued move, so feedback needs to distinguish a different prepared reply from an unknown move.
- Configuration and repertoire persistence now use atomic replacement and retain a previous valid copy; continue extending those safeguards when adding new persistent data.

Fast remains an offline, single-user desktop application. The proposed work uses local engines, databases and user-controlled files. Like online lookups and engine downloads, downloading the user's own Lichess or Chess.com games connects only when the user asks. Network accounts, cloud synchronization, online play, mobile clients and a plugin platform would require a separate scope decision.

Recommended implementation order: 1–6, then 7–14. Choose items 15–16 according to user feedback and release needs. Start with persistence contracts before building features that depend on them.
