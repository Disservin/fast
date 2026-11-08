# Exact position-pattern index measurement

Measured on Linux on 2026-11-08, using the release build of
`crates/db/examples/structure_bench.rs`. The fixture repeats 1,000 local sample games to
1,000,000 games. This is a scale and storage test, **not a varied million-game Lichess dump**;
its repeated structures favour cache locality. Validate against a varied collection before
using these figures as a release performance claim.

The optional table stores the first ply of each consecutive pawn/material pattern. The
primary key is `(game_id, ply)`; secondary indexes cover `(pawn_hash, material, game_id)` and
`(material, game_id)`. Games with unchanged patterns do not repeat index rows for every
piece move. The ordinary position table is unchanged.

- Build time: 390.36 seconds.
- Added database bytes after the connection closed and WAL checkpointed: 2,296,647,680
  (2.30 GB decimal, approximately 2,297 bytes per game).
- The first pawn search returned 59,000 games in 1.007 seconds; the first material search
  returned 974,000 games in 1.015 seconds; the combined search took 0.081 seconds.
- A subsequent pass measured both queries requested by the UI:

| Search | Games | Result ids | Summary statistics | Total |
| --- | ---: | ---: | ---: | ---: |
| Pawn structure | 59,000 | 0.135 s | 0.132 s | 0.267 s |
| Material | 974,000 | 1.133 s | 1.455 s | 2.588 s |
| Both at the same position | 59,000 | 0.080 s | 0.137 s | 0.217 s |

The reference is the position after `1. e4 d5`, so the material-only query deliberately
matches nearly the entire fixture. Searches execute off the UI thread; a changed query
cancels delivery of an older result. Building checks cancellation at every position and
publishes the index atomically, preserving a previous complete index on cancellation.
The measured storage cost is why the index remains optional.

Reproduce with a **copy** of a source database and an unused output path:

```sh
cargo run --release -p db --example structure_bench -- sample.sqlite /tmp/pattern-million.sqlite
cargo run --release -p db --example structure_bench -- --query /tmp/pattern-million.sqlite
```

The source is opened read-only. The fixture contains game rows and the optional index;
it does not populate the existing exact-position index. Builds on a normal database also
need space for WAL while the transaction is running. No database or PGN fixture is committed.
