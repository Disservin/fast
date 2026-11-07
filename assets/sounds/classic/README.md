# Classic chess recordings

Recorded by **el_boss** for Chess Puzzle Blitz on a real wooden chessboard.
Licensed under **CC0 1.0 Universal** (see CC0-1.0.txt).
These are independent recordings, not Lichess or Chess.com audio files.

| Bundled file | Original | Public high-quality preview downloaded |
| --- | --- | --- |
| placement.mp3 | [Piece Placement.mp3, #546119](https://freesound.org/people/el_boss/sounds/546119/) | https://cdn.freesound.org/previews/546/546119_9129912-hq.mp3 |
| capture.mp3 | [Piece Capture.mp3, #546120](https://freesound.org/people/el_boss/sounds/546120/) | https://cdn.freesound.org/previews/546/546120_9129912-hq.mp3 |
| reset.mp3 | [Board Start.mp3, #546121](https://freesound.org/people/el_boss/sounds/546121/) | https://cdn.freesound.org/previews/546/546121_9129912-hq.mp3 |

The MP3 files are unmodified public previews. The matching `.pcm` files are signed
16-bit little-endian mono samples at 44,100 Hz, converted by
`scripts/prepare_chess_sounds.py`. Processing removes leading/trailing silence,
normalizes peak amplitude to 0.8, resamples and applies a 2 ms tail fade. Pitch and
tempo are unchanged. Castling layers two placement recordings 90 ms apart. Check
adds no separate tone. Game end uses the board-reset recording.

Retrieved 2026-11-07. No attribution is required by CC0, but the original creator
and sources are credited here and in Settings → Licenses.
