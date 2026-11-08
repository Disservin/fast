# En Croissant feedback implementation

The scoped implementation is complete:

- PGN comments, variations and NAGs survive import and export, with diagnostics for dropped
  illegal variations.
- The opening tree can query any local database and open matching games.
- Games sort by file order or any table column.
- Lichess and Chess.com account downloads are explicit, incremental and cancellable.
- Date, event, result, rating and length filters support saved searches.
- Exact pawn-structure and material searches use an optional index, built during import or
  later for an existing database. Results open at a matching position. See
  [the storage and latency measurement](position-pattern-benchmark.md) for its cost and
  the repeated-sample benchmark limitation.
- Chess960 works in Analyse and Play, including start numbers, Variant-tagged PGN,
  X-FEN/Shredder-FEN, castling entry, engine support checks and batch review.

Approximate structure matching, representative-game ranking and opening results in training
remain in [roadmap item 11](roadmap.md). Exact matching shipped first as agreed; it provides
shared signatures and an optional index for that later work. A varied million-game collection
and full interactive game sessions remain broader release validation work. A local Stockfish
check covered both castling directions, a stationary king and return to standard chess.

The user guides live under `website/content/workspace/`; crate, protocol and database changes
are documented in `ARCHITECTURE.md`.
