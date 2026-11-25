"""Short synthesised cues, so a recording is obvious without looking."""
from __future__ import annotations

import io
import math
import struct
import wave

try:
    import winsound
except ImportError:  # pragma: no cover - non-Windows
    winsound = None

_RATE = 44_100
_cache: dict[str, bytes] = {}


def _tone(freqs: list[tuple[float, float]], volume: float = 0.22) -> bytes:
    frames = bytearray()
    for freq, ms in freqs:
        count = int(_RATE * ms / 1000)
        for i in range(count):
            # fade in/out over 5 ms to avoid clicks
            edge = min(i, count - i, _RATE * 0.005)
            gain = volume * (edge / (_RATE * 0.005))
            value = int(32767 * gain * math.sin(2 * math.pi * freq * i / _RATE))
            frames += struct.pack("<h", value)
    buffer = io.BytesIO()
    with wave.open(buffer, "wb") as handle:
        handle.setnchannels(1)
        handle.setsampwidth(2)
        handle.setframerate(_RATE)
        handle.writeframes(bytes(frames))
    return buffer.getvalue()


def _play(name: str, freqs: list[tuple[float, float]]) -> None:
    if winsound is None:
        return
    data = _cache.get(name)
    if data is None:
        data = _cache[name] = _tone(freqs)
    try:
        winsound.PlaySound(data, winsound.SND_MEMORY | winsound.SND_ASYNC | winsound.SND_NODEFAULT)
    except RuntimeError:
        pass


def start() -> None:
    _play("start", [(760, 45), (1180, 55)])


def stop() -> None:
    _play("stop", [(980, 40), (620, 60)])


def done() -> None:
    _play("done", [(1320, 55)])


def error() -> None:
    _play("error", [(300, 90), (220, 110)])
