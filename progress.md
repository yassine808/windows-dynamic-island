# Progress — stt-island

## Plan
- [x] Research existing Windows dynamic-island projects
- [x] Scaffold PySide6 island window (pill → waveform → transcript)
- [x] Global hold-to-talk hotkey + mic capture
- [x] Whisper STT (faster-whisper, local, offline)
- [x] Paste transcript into focused window
- [x] Verify end-to-end (SAPI speech → transcript)
- [ ] Commit and push

## Log

### 2026-11-25 — bootstrap
- Goal: repo + folder, then a hold-to-talk STT island for Windows.
- Done: created `D:\Projects\stt-island`, git repo, GitHub private repo, README.
- Verified: `gh repo view` → PRIVATE, main pushed.
- Next: build the app.

### 2026-11-25 — hold-to-talk STT island
- Goal: a Dynamic Island for Windows that records while a key is held and pastes the transcript.
- Done:
  - `stt_island/island.py` — frameless top-centre pill: idle / listening (pulse + waveform + timer) /
    transcribing / result / error, animated resize, drag to move, click to copy.
  - `stt_island/hotkey.py` — push-to-talk via `GetAsyncKeyState` polling (no hook, no admin).
  - `stt_island/audio.py` — sounddevice 16 kHz mono capture with RMS level.
  - `stt_island/transcribe.py` — faster-whisper int8 CPU, worker threads, model warm-up.
  - `stt_island/inject.py` — clipboard + `SendInput` Ctrl+V.
  - `stt_island/settings_ui.py`, `sounds.py`, `config.py`, tray menu, autostart, single instance.
  - `tools/selftest.py` — SAPI speech → WAV → Whisper check.
- Verified:
  - `python main.py --smoke` → idle → listen → think → result, exit 0.
  - `python tools/selftest.py --model small` → 23/23 words, PASS.
  - Real launch: model ready in 2.2 s; simulated Right Ctrl hold produced `captured 1.28s` and a
    `no transcript: Nothing heard` on a silent mic — press/release/record/transcribe path works.
  - Screenshots of idle / listening / result captured into `docs/`.
- Next: commit and push.
