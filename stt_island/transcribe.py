"""Local speech-to-text with faster-whisper (CTranslate2). Runs on the CPU, offline."""
from __future__ import annotations

import logging
import re
import threading
import time

import numpy as np
from PySide6.QtCore import QObject, Signal

from .config import MODEL_DIR

log = logging.getLogger(__name__)

WHISPER_SAMPLE_RATE = 16_000


def load_model(name: str):
    from faster_whisper import WhisperModel

    MODEL_DIR.mkdir(parents=True, exist_ok=True)
    started = time.monotonic()
    model = WhisperModel(name, device="cpu", compute_type="int8", download_root=str(MODEL_DIR))
    log.info("whisper '%s' loaded in %.1fs", name, time.monotonic() - started)
    return model


def _normalize(audio: np.ndarray) -> np.ndarray:
    audio = np.asarray(audio, dtype=np.float32).reshape(-1)
    peak = float(np.max(np.abs(audio))) if audio.size else 0.0
    if 0.0 < peak < 0.30:
        audio = audio * (0.90 / peak)
    return np.clip(audio, -1.0, 1.0)


def _clean(text: str) -> str:
    text = re.sub(r"\s+", " ", text or "").strip()
    text = re.sub(r"\s+([,.!?;:])", r"\1", text)
    return text


def transcribe_audio(model, audio: np.ndarray, language: str = "", beam_size: int = 1) -> str:
    segments, _info = model.transcribe(
        _normalize(audio),
        language=language or None,
        beam_size=max(1, int(beam_size)),
        vad_filter=True,
        vad_parameters={"min_silence_duration_ms": 300},
        condition_on_previous_text=False,
        without_timestamps=True,
    )
    parts = [seg.text.strip() for seg in segments if seg.no_speech_prob < 0.70]
    return _clean(" ".join(p for p in parts if p))


class Transcriber(QObject):
    """Owns one Whisper model and runs transcriptions off the UI thread."""

    loaded = Signal(str)
    load_failed = Signal(str)
    finished = Signal(str, float)
    failed = Signal(str)

    def __init__(self, model_name: str, language: str = "", beam_size: int = 1, parent=None) -> None:
        super().__init__(parent)
        self.model_name = model_name
        self.language = language
        self.beam_size = beam_size
        self._model = None
        self._loading = False
        self._busy = False
        self._lock = threading.Lock()

    @property
    def ready(self) -> bool:
        return self._model is not None

    @property
    def busy(self) -> bool:
        return self._busy

    def load_async(self) -> None:
        if self._loading or self._model is not None:
            return
        self._loading = True

        def work() -> None:
            try:
                self._model = load_model(self.model_name)
                self.loaded.emit(self.model_name)
            except Exception as exc:  # noqa: BLE001 - network, disk, missing dll
                log.exception("model load failed")
                self.load_failed.emit(str(exc))
            finally:
                self._loading = False

        threading.Thread(target=work, name="whisper-load", daemon=True).start()

    def switch_model(self, name: str) -> None:
        if name == self.model_name and self._model is not None:
            return
        self.model_name = name
        self._model = None
        self.load_async()

    def transcribe(self, audio: np.ndarray) -> None:
        if self._busy:
            self.failed.emit("Still transcribing the last one")
            return
        if audio is None or audio.size < WHISPER_SAMPLE_RATE * 0.15:
            self.failed.emit("Nothing heard")
            return
        if not self.ready:
            self.failed.emit("Model is still loading")
            return
        model = self._model
        with self._lock:
            self._busy = True
        started = time.monotonic()

        def work() -> None:
            try:
                text = transcribe_audio(model, audio, self.language, self.beam_size)
                took = time.monotonic() - started
                if text:
                    self.finished.emit(text, took)
                else:
                    self.failed.emit("Nothing heard")
            except Exception as exc:  # noqa: BLE001
                log.exception("transcription failed")
                self.failed.emit(str(exc))
            finally:
                with self._lock:
                    self._busy = False

        threading.Thread(target=work, name="whisper-run", daemon=True).start()
