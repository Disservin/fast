#!/usr/bin/env python3
"""Convert the bundled CC0 chess recordings to runtime PCM using libsndfile.

Run from any directory. Requires a system libsndfile with MP3 support; no Python
packages are needed. Runtime playback does not use libsndfile or an audio decoder.
"""
import ctypes as c
import ctypes.util
import math
from pathlib import Path
import struct

class Info(c.Structure):
    _fields_ = [("frames", c.c_longlong), ("samplerate", c.c_int),
                ("channels", c.c_int), ("format", c.c_int),
                ("sections", c.c_int), ("seekable", c.c_int)]

lib = c.CDLL(ctypes.util.find_library("sndfile"))
lib.sf_open.argtypes = [c.c_char_p, c.c_int, c.POINTER(Info)]
lib.sf_open.restype = c.c_void_p
lib.sf_readf_float.argtypes = [c.c_void_p, c.POINTER(c.c_float), c.c_longlong]
lib.sf_readf_float.restype = c.c_longlong
lib.sf_close.argtypes = [c.c_void_p]

for path in (Path(__file__).resolve().parents[1] / "assets/sounds/classic").glob("*.mp3"):
    info = Info()
    handle = lib.sf_open(bytes(path), 0x10, c.byref(info))
    if not handle:
        raise RuntimeError(f"Cannot decode {path}")
    buffer = (c.c_float * (info.frames * info.channels))()
    frames = lib.sf_readf_float(handle, buffer, info.frames)
    lib.sf_close(handle)
    samples = [sum(buffer[i * info.channels + j] for j in range(info.channels)) / info.channels for i in range(frames)]
    # Remove codec padding and silence without changing pitch or the recorded attack.
    peak = max(abs(value) for value in samples)
    active = [i for i, value in enumerate(samples) if abs(value) > peak * 0.003]
    samples = samples[max(0, active[0] - info.samplerate // 1000):min(len(samples), active[-1] + info.samplerate // 50)]
    output = []
    for i in range(math.floor(len(samples) * 44100 / info.samplerate)):
        source = i * info.samplerate / 44100
        left = int(source)
        fraction = source - left
        value = samples[left] * (1 - fraction) + samples[min(left + 1, len(samples) - 1)] * fraction
        output.append(value * 0.80 / peak)
    fade = min(88, len(output))
    for i in range(fade):
        output[-fade + i] *= 1 - i / fade
    path.with_suffix(".pcm").write_bytes(b"".join(struct.pack("<h", round(max(-1, min(1, value)) * 32767)) for value in output))
    print(path.name, f"{len(output) / 44100:.3f}s")
