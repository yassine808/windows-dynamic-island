"""End-to-end check without a microphone: Windows TTS speaks a sentence, Whisper reads it back.

    python tools/selftest.py --model small
"""
from __future__ import annotations

import argparse
import subprocess
import sys
import tempfile
import time
import wave
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from stt_island.transcribe import load_model, transcribe_audio  # noqa: E402

SENTENCE = (
    "This is a test of the speech to text island. "
    "Hold the key, speak clearly, and the words appear where the cursor is."
)


def speak_to_wav(path: Path, text: str) -> None:
    script = f"""Add-Type -AssemblyName System.Speech
$voice = New-Object System.Speech.Synthesis.SpeechSynthesizer
$voice.Rate = 0
$voice.SetOutputToWaveFile("{path}")
$voice.Speak("{text}")
$voice.Dispose()
"""
    subprocess.run(
        ["powershell", "-NoProfile", "-NonInteractive", "-Command", script],
        check=True,
        capture_output=True,
    )


def read_wav(path: Path) -> tuple[np.ndarray, int]:
    with wave.open(str(path), "rb") as handle:
        rate = handle.getframerate()
        channels = handle.getnchannels()
        width = handle.getsampwidth()
        raw = handle.readframes(handle.getnframes())
    if width != 2:
        raise SystemExit(f"unexpected sample width {width}")
    audio = np.frombuffer(raw, dtype=np.int16).astype(np.float32) / 32768.0
    if channels > 1:
        audio = audio.reshape(-1, channels).mean(axis=1)
    return audio, rate


def resample(audio: np.ndarray, rate: int, target: int = 16_000) -> np.ndarray:
    if rate == target:
        return audio
    count = int(round(len(audio) * target / rate))
    return np.interp(np.linspace(0, len(audio) - 1, count), np.arange(len(audio)), audio).astype(np.float32)


def words(text: str) -> list[str]:
    return [w for w in "".join(c.lower() if c.isalnum() else " " for c in text).split() if w]


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--model", default="small")
    parser.add_argument("--wav", default="", help="use an existing wav instead of Windows TTS")
    args = parser.parse_args()

    with tempfile.TemporaryDirectory() as tmp:
        wav = Path(args.wav) if args.wav else Path(tmp) / "speech.wav"
        if not args.wav:
            print(f"[1/3] synthesising speech with Windows TTS -> {wav}")
            speak_to_wav(wav, SENTENCE)
        audio, rate = read_wav(wav)
        audio = resample(audio, rate)
        print(f"[2/3] audio {len(audio) / 16_000:.2f}s at 16 kHz, loading '{args.model}'")

        started = time.monotonic()
        model = load_model(args.model)
        load_time = time.monotonic() - started
        started = time.monotonic()
        text = transcribe_audio(model, audio)
        run_time = time.monotonic() - started

    print(f"[3/3] model {load_time:.1f}s, transcription {run_time:.1f}s")
    print(f"expected: {SENTENCE}")
    print(f"got     : {text}")

    expected, got = words(SENTENCE), words(text)
    if not got:
        print("FAIL: empty transcript")
        return 1
    hits = sum(1 for w in expected if w in got)
    score = hits / len(expected)
    print(f"word match: {hits}/{len(expected)} = {score:.0%}")
    if score < 0.70:
        print("FAIL: transcript too far from the spoken sentence")
        return 1
    print("PASS")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
