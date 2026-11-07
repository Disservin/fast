# Fast: additions and changes

A standalone product backlog for the native, offline chess workstation. Priorities reflect data safety, study correctness and daily usefulness. This is a proposal, not a claim that every listed failure has been reproduced. Current behavior was checked against the source; no application code changes are included.

## First: make analysis reusable

1. **Add — persistent analysis records.** Save evaluations, principal variations, depth and search limits automatically. Identify results by position, engine version and relevant options. Reopen saved results immediately, show when they are stale, and allow explicit replacement. Keep game-specific context where draw history changes the result.
2. **Change — make batch review configurable.** Replace the fixed per-position time budget with selectable engine, time/depth/node limits and player side. Review the filtered list or an explicit selection. Store per-position results and both players’ summaries, rather than only a game accuracy value.
3. **Change — make review jobs resumable.** Show game and position progress, pause, resume, cancel and retry. Preserve completed results, skip results that meet the requested quality, and distinguish incomplete games from finished reports. Cancellation and engine failure must release processes and leave an honest status.
4. **Change — review the game’s actual starting position.** Respect custom FEN starts and the side to move, preserve repetition history where available, and handle terminal positions and missing engine scores explicitly. Never turn absent analysis into an apparently measured zero score.
5. **Add — explain supported tactical findings.** Fill the explanation area with concrete, checkable observations such as a hanging piece, fork, lost material or missed mate. Link each explanation to a board highlight and a supporting line; label uncertain interpretations and avoid unsupported strategic claims.
6. **Add — navigate review findings.** Jump between mistakes, missed opportunities and opening deviations. Compare the played move with an alternative and save that line as a variation without disturbing the original game.
7. **Change — expose engine health and effective settings.** Show startup, loading, searching, paused, failed and stopped states. Make the applied play/review limits visible, offer retry after failure, and support relocating a missing binary without losing profiles. Verify fast final results are displayed even when intermediate updates are throttled.

**Completion checks:** reopening analysis needs no engine rerun; changed engine settings do not silently reuse incompatible results; interrupted jobs resume correctly; custom-start games review accurately; failed searches remain distinguishable from neutral evaluations.

## Then: improve the daily workspace

8. **Add — workspace restoration and recent games.** Remember the active screen, open study documents, selected positions, orientation and panel sizes. Restore saved engine selections, with an explicit preference controlling whether computation restarts. Keep recent games easy to find.
9. **Change — make navigation and editing predictable.** Add a clear branch picker, searchable move/variation navigation and inline comment editing. Keep text-entry focus separate from move-navigation shortcuts. Offer undo for destructive edits and a way to return from exploratory analysis to the original game.
10. **Change — make layouts work across window sizes.** Save panel sizes per screen, provide a reset action, and allow panels to stack when space is tight. Keep the board square, tables aligned and essential actions reachable with keyboard navigation and visible focus.
11. **Change — make PGN imports tolerant and transparent.** Accept unknown ratings, common encoding artifacts and valid annotation variants. Preserve comments, variations and custom starts. Report imported, skipped and failed games with useful diagnostics; cancellation must retain committed work and report partial completion accurately.
12. **Add — import deduplication and database cleanup.** Detect exact repeated games first. Offer optional prefix/metadata-tolerant matching, preview proposed merges, preserve annotations and allow the user to choose which version survives. Never silently delete games based on a fuzzy match.
13. **Add — database management tools.** Merge selected databases, save filters, sort by useful game attributes and manage reference collections. Keep searches, imports and large result lists responsive; verify this with representative large datasets before choosing optimizations.
**Completion checks:** a reopened workspace returns to the same study context; cancelling an import reports exactly what was retained; deduplication offers a reviewable preview; common tasks remain reachable in a narrow window.

## Later: expand offline study and preparation

14. **Add — a repertoire library.** Manage separate named repertoires for either side, choose the side when creating or importing one, and practise a selected branch or a full line. Show coverage and gaps using shared position knowledge.
15. **Add — personal puzzle sets.** Create exercises from reviewed mistakes and imported puzzle files. Require validated solutions, allow post-exercise analysis, and save session history. Support repeatable fixed-set cycles with time and accuracy comparisons.
16. **Add — guess-the-move training.** Hide the next move in a selected game, let the user choose a side, and optionally accept engine-supported alternatives. Explain scoring and resume unfinished sessions.
17. **Add — opening and player reports.** Summarize candidate moves, results, opponent strength and trends from local databases. Show sample sizes, filters and the underlying games so users can assess the statistics.
18. **Add — model-game and structure search.** Find representative games from a position or matching pawn structure. Make exact and approximate matches distinguishable and allow users to open results directly in analysis or training.
19. **Add — local endgame guidance.** Display tablebase outcomes and distance-to-zeroing information when suitable local files are configured. Explain unavailable coverage and distinguish theoretical results from practical fifty-move-rule outcomes.
20. **Add — engine opening books.** Configure books per engine, book depth and move variety for engine practice. Practice games already use separate tabs and have recovery snapshots; workspace restoration across app restarts remains on the workspace roadmap item.
21. **Add — persistent exploration trees.** Let users queue candidate positions, deepen selected branches and resume exploration across sessions. Make CPU, memory and disk budgets explicit. Evaluate parallel workers only after measuring throughput and preserving a clear analysis-quality policy.

## Release readiness

22. **Change — validate native distribution on each supported platform.** Exercise installation, first launch, file dialogs, audio, engine startup, high-DPI layout and keyboard navigation on Windows, macOS and Linux. Linux development checks exist; complete equivalent checks on Windows and macOS and publish clear compatibility requirements, signed packages where available and a manual offline update path.
23. **Add — localization support when the core workflow is stable.** Move visible strings into translation resources and localize dates, numbers and review intervals. Keep notation and shortcuts readable across languages and longer labels.

## Existing foundations to extend

Fast already provides multiple analysis engines, saved engine profiles, configurable shortcuts, position-based live analysis caching, backwards analysis, graph views, streaming cancellable imports, paged database rows, batch accuracy review, repertoire imports, spaced repetition, repertoire leak detection, engine play, premoves, configurable board/piece appearance, selectable move sounds and independent workspace tabs. These should be extended and verified rather than rebuilt.

The source shows several useful boundaries for the work above:

- Live analysis caches currently live in memory; durable analysis needs its own storage and invalidation policy.
- Batch review currently uses a fixed 40 ms per position, begins at the standard initial position and stores aggregate accuracy.
- Repertoire analysis has a position index, while practice cards, drill queues, gap traversal and grading remain attached to move-tree paths.
- Practice currently compares the answer with one queued move, so feedback needs to distinguish a different prepared reply from an unknown move.
- Configuration and repertoire persistence now use atomic replacement and retain a previous valid copy; continue extending those safeguards when adding new persistent data.

Fast remains an offline, single-user desktop application. The proposed work uses local engines, databases and user-controlled files. Network accounts, cloud synchronization, online play, mobile clients and a plugin platform would require a separate scope decision.

Recommended implementation order: 1–7, then 8–13, then 14–21. Choose items 22–23 according to user feedback and release needs. Start with persistence contracts before building features that depend on them.
