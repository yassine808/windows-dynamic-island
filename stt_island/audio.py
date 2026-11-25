"""Microphone capture with sounddevice: 16 kHz mono float32, which Whisper wants."""
from __future__ import annotations

import logging
import threading
import time

import numpy as np

log = logging.getLogger(__name__)

SAMPLE_RATE = 16_000


class AudioError(RuntimeError):
    pass


def input_device_name() -> str:
    try:
        import sounddevice as sd

        info = sd.query_devices(kind="input")
        return str(info.get("name", "default input"))
    except Exception as exc:  # noqa: BLE001 - any PortAudio failure is the same to us
        return f"no input ({exc})"


class Recorder:
    """Start/stop a mono 16 kHz stream; keeps a running RMS level for the waveform."""

    def __init__(self, sample_rate: int = SAMPLE_RATE) -> None:
        self.sample_rate = sample_rate
        self._stream = None
        self._chunks: list[np.ndarray] = []
        self._lock = threading.Lock()
        self.level = 0.0
        self.peak = 0.0
        self.started_at = 0.0

    @property
    def active(self) -> bool:
        return self._stream is not None

    def start(self) -> None:
        if self._stream is not None:
            return
        import sounddevice as sd

        self._chunks = []
        self.level = 0.0
        self.peak = 0.0
        try:
            self._stream = sd.InputStream(
                samplerate=self.sample_rate,
                channels=1,
                dtype="float32",
                blocksize=1024,
                callback=self._callback,
            )
            self._stream.start()
        except Exception as exc:  # noqa: BLE001
            self._stream = None
            raise AudioError(f"cannot open microphone: {exc}") from exc
        self.started_at = time.monotonic()
        log.info("recording from %s", input_device_name())

    def _callback(self, indata, frames, time_info, status) -> None:  # noqa: ANN001
        if status:
            log.debug("audio status: %s", status)
        chunk = indata[:, 0].copy()
        rms = float(np.sqrt(np.mean(chunk * chunk))) if chunk.size else 0.0
        with self._lock:
            self._chunks.append(chunk)
            self.level = rms
            if rms > self.peak:
                self.peak = rms

    def elapsed(self) -> float:
        return time.monotonic() - self.started_at if self.started_at else 0.0

    def stop(self) -> np.ndarray:
        stream, self._stream = self._stream, None
        if stream is not None:
            try:
                stream.stop()
                stream.close()
            except Exception:  # noqa: BLE001
                log.debug("stream close failed", exc_info=True)
        with self._lock:
            chunks, self._chunks = self._chunks, []
        if not chunks:
            return np.zeros(0, dtype=np.float32)
        audio = np.concatenate(chunks).astype(np.float32, copy=False)
        log.info("captured %.2fs (peak %.3f)", len(audio) / self.sample_rate, self.peak)
        return audio

    def cancel(self) -> None:
        self.stop()
