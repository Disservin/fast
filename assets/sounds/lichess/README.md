# Lichess sound sets

The Futuristic, NES, Piano and SFX sets by [Enigmahack](https://github.com/Enigmahack),
from [lichess-org/lila `public/sound`](https://github.com/lichess-org/lila/tree/master/public/sound)
at commit `ea90849364d6b8063a77cccedf762a6c04f9d593`. Lila's `COPYING.md` lists them under
**AGPL-3.0-or-later**; see https://www.gnu.org/licenses/agpl-3.0.txt.

Only the files Fast plays are bundled: `Move`, `Capture`, `Check` and `GenericNotify`
(played at the game's end). The MP3 files are unmodified. The matching `.pcm` files are
signed 16-bit little-endian mono samples at 44,100 Hz, made by
`scripts/prepare_chess_sounds.py` (silence trimmed, peak normalized to 0.8, 2 ms tail fade).

Lichess' Standard, Robot, Woodland and other sets are not free licensed, and Lisp is
CC BY-NC-SA, so they are not bundled.

Retrieved 2026-11-08.

Fast itself is GPL-3.0-or-later. GPL-3.0 section 13 allows shipping these AGPL files with
it; they keep their AGPL terms (see `../../LICENSES.md`).
