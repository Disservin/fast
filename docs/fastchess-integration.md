# fastchess integration: wishlist

Fast's Tournament tab drives fastchess as an external process (`crates/app/src/fastchess.rs`,
`crates/app/src/screens/tournament.rs`). It works, but several pieces lean on details of
fastchess's human-readable output and on-disk files. This is a list of changes that could go
into fastchess itself to make that integration robust, roughly by value. Each item notes what
Fast does today, so it is clear what could be deleted once fastchess offers the feature.

Tested against fastchess 1.8.2-alpha (the release Fast downloads) and master at `7b16efe8`.

## 1. Machine-readable event stream

**Today:** Fast parses stdout line by line: `Started game N of M (W vs B)`,
`Finished game N (W vs B): RESULT {reason}`, and results blocks between lines of dashes, then
picks `Elo:`, `LLR:`, `Games:`, `Ptnml(0-2):`, `LOS:` out by prefix. Any wording change breaks
it, and for more than two engines Fast can only show the ranking table as text.

**Wish:** `-output format=json` (or `-events file=…|stdout`) emitting one JSON object per line:

```json
{"event":"game_started","game":12,"round":6,"white":"new","black":"base","opening":"…"}
{"event":"game_finished","game":12,"white":"new","black":"base","result":"1-0","termination":"adjudication","reason":"White wins by adjudication","pgn_index":11}
{"event":"results","pairs":[{"first":"new","second":"base","wins":40,"losses":30,"draws":30,"penta":[1,10,20,15,4]}],"elo":{"diff":12.3,"error":5.7,"nelo":…},"llr":1.23,"llr_bounds":[-2.94,2.94],"los":0.99}
{"event":"engine_error","engine":"new","game":12,"kind":"timeout|crash|illegal_move|disconnect","detail":"…"}
{"event":"sprt_finished","accepted":"H1"}
{"event":"tournament_finished","reason":"completed|sprt|interrupted|drained","resume":"config.json"}
```

Numbers as numbers (not `-0.00 +/- 0.00`, `-nan %`), names as fields (no splitting on
` vs `, which breaks for engines whose name contains it), and a schema version on the first
line. Human output can stay the default.

## 2. Live moves for a board view

**Today:** Fast shows live boards by adding
`-log file=/dev/stdout level=warn engine=true realtime=true` and rebuilding each game from the
UCI traffic (`crates/app/src/tournament_live.rs`): the logging thread id stands in for a game
id, `ucinewgame` marks the next game on that thread, `position … moves` carries the moves,
`go` the clocks and `info` the evaluations. This costs a lot of output (about 4,000 lines per
game at 1+0.01; 61,706 lines for 16 games), depends on the log line format, cannot be
matched to the game numbers in `Finished game N`, and does not work on Windows, which has no
`/dev/stdout`. The log options are also saved into `config.json`, so Fast resets `file` and
`engine_coms` after each run, or a later `fastchess -config file=config.json` in a terminal
would print every engine message.

**Wish:** an opt-in per-move event (`-events moves=true`): game id, move (UCI), FEN after the
move, clocks, and the engine's last score/depth/nodes/PV. With concurrency this is a lot of
output, so it should be opt-in and ideally rate-limited, or available per game on request (see
the control channel below).

## 3. A control channel that works on every platform

**Today:** Stop sends SIGINT on Unix, which makes fastchess save `config.json` and exit
(exit code 1, same as an error). On Windows there is no clean way for a GUI parent to deliver
Ctrl+C to a console process it started with `CREATE_NO_WINDOW` (it would have to attach to
the child's console and signal the whole group), so Stop has to
`TerminateProcess` fastchess and the tournament falls back to the last autosave
(every 20 games by default). The graceful `SIGUSR1` drain on master is Unix-only and not in a
release yet.

**Wish:** read commands from stdin (opt-in, e.g. `-control stdin`): `stop` (save and exit now),
`drain` (finish running games, then save and exit), `pause`/`resume`, `status` (reply with a
results event), and maybe `concurrency N`. This gives every platform the same graceful stop
and lets a GUI pause a long SPRT overnight without killing engines.

## 4. Distinct exit codes

**Today:** interrupted runs and real errors both exit with 1; Fast tells them apart only by
remembering that it asked for a stop.

**Wish:** documented codes, e.g. 0 completed, 2 SPRT accepted H0, 3 SPRT accepted H1
(or 0 for both with the outcome in the event stream), 4 interrupted/drained (resumable),
1 configuration error, 5 engine failure.

## 5. Resuming tournaments with more than two engines

**Today:** `-config file=…` drops stats for more than two engines ("Warning: Stats will be
dropped for more than 2 engines."), so continuing a round robin restarts its results from zero,
while the PGN keeps the old games. Fast's "Continue" button can't fix this. The same warning is
also printed on fresh runs that only pass `-config outname=…`, which is misleading.

**Wish:** store per-pair stats (it is already keyed `"a vs b"`) and the schedule position, so
any tournament resumes exactly; only warn when stats are actually dropped.

## 6. Map games in the PGN to game ids

**Today:** fastchess writes games to the PGN in finishing order, and the `Round` tag is the
pairing round, so two games share it. Fast relies on the k-th `Finished game` line matching
the k-th game in the PGN (offset by games already in the file when resuming), which breaks if
anything else appends to the file.

**Wish:** a `[GameId "12"]` tag (and the total game index), or the PGN byte offset /
`pgn_index` in the finished-game event.

## 7. Validate and normalise without playing

**Today:** Fast checks its own form, but a pasted command line is only validated when fastchess
starts, and errors arrive as free text on stdout.

**Wish:** `-dry-run` (or `--check`): parse everything, probe nothing or optionally probe engines
(`uci`/`uciok`), print the resolved configuration as JSON (the same schema as `config.json`)
and exit with 0/1. A GUI could then show "2 engines, 10+0.1, 1000 rounds × 2 games" before
starting, and convert a command line into a config in one call.

## 8. A friendlier `config.json`

- Enums are stored as integers (`"format": 2`, `"order": 1`, `"output": 0`, `"type": 0`,
  `"notation": 0`, `"variant": 0`). Strings (`"epd"`, `"random"`, `"roundrobin"`) would make the
  file readable and editable by people and GUIs, and stable if enum order changes.
- Add a `"version"`/schema field.
- Relative paths (`pgn.file`, `opening.file`, engine `cmd`/`dir`) are resolved against the
  current directory at resume time, not where the tournament started. Fast works around this
  by running each tournament in its own folder and remembering the folder a pasted command ran
  in. Storing absolute paths, or resolving relative to the config file, would remove that.
- Document the format (or ship a JSON schema) so tools can generate it instead of a command
  line.

## 9. Release assets

- Archives contain the whole source tree (417 files, ~14 MB) next to a 3.5 MB binary. A
  binary-only archive (binary, LICENSE, README) would download faster.
- Linux/macOS assets are plain `.tar`; `.tar.gz`/`.tar.xz` would be a quarter of the size.
- Publish a `SHA256SUMS` file (Fast pins the digests GitHub reports per asset today), and
  ideally a stable `latest` download URL or a small JSON manifest (version → asset → digest),
  so tools can offer updates without hard-coding each release.
- `--version` prints `fastchess alpha 1.8.2 compiled for … (CI)`; a machine-readable
  `--version-json` (version, commit, build date, platform) would let Fast check a user's own
  executable.

## 10. Smaller things noticed

- Without `-openings`, fastchess prints both "No opening book specified!" and
  "Unknown opening format, 2. All games will be played from the starting position." The second
  message is noise (and exposes the enum value).
- On Windows, engines are started with `CREATE_NEW_PROCESS_GROUP` only. When fastchess itself
  runs without a visible console (as from a GUI), console engines can open a window each unless
  fastchess has a hidden console; passing `CREATE_NO_WINDOW` for engines would make that
  independent of how fastchess was launched.
- `Started game N of M` with SPRT and no `-rounds` reports M = 1,000,000 (500,000 rounds × 2), which is not a
  useful total; an "until SPRT finishes" marker (or `null`) in the event stream would let a GUI
  show LLR progress instead.
- Per-move comments in the PGN (`{+0.25/20 0.762s}`) are fastchess-specific. Also emitting the
  standard `[%eval]`/`[%clk]` commands (or offering it as an option) would let GUIs, Fast's
  Analyse graph included, read evaluations and clocks without a custom parser.
