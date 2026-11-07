# Lichess chess opening names

Source: https://github.com/lichess-org/chess-openings
Revision: `3ba6b20d31b2e6125cb0e6a2e792f400717d9d39`

The five unmodified upstream TSV files contain all 3,865 opening lines across
ECO volumes A–E. The data is CC0-1.0; see COPYING.txt.

`chess_core/build.rs` validates and converts these files into a sorted Rust table
embedded in the binary. Builds and runtime lookup need no network or external
data files. Classification uses the longest matching SAN prefix, ignoring check
and mate marks; it does not resolve transpositions.

From the repository root, update with:

```sh
python3 scripts/update_openings.py
```

Requires Python 3, Git, Cargo, and internet access. Use `--ref <commit-or-tag>`
to select a particular upstream revision. The command validates all five files,
updates the license and revision here, and runs the chess-core and database tests.
If tests fail, it restores the previous data. Rebuild Fast to bundle the update.
