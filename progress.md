# Progress — stt-island

## Plan
- [x] Research existing Windows dynamic-island projects
- [x] Scaffold PySide6 island window (pill → waveform → transcript)
- [x] Global hold-to-talk hotkey + mic capture
- [x] Whisper STT (faster-whisper, local, offline)
- [x] Paste transcript into focused window
- [x] Verify end-to-end (SAPI speech → transcript)
- [x] Restyle the island to ReUI's design system
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

### 2026-11-25 — ReUI design pass
- Goal: restyle the island to match ocopus-labs/ReUI.
- Done:
  - Read ReUI `DESIGN.md` + `design/tokens.json` and mapped the tokens: black `#000000` capsule,
    height 36.67 -> 37, top gap 11, concentric 24 art inset 6.33, text margin 12, bars 3 x 2.5 x 14 at 30 fps,
    compact 15/20 semibold, detail 15/20, caption 12/16, labels #EBEBF0 / #8E8D94, red #FF453A,
    green #30D158, default accent #0A84FF.
  - `island.py` rewritten: no border, shadow, gradient or glow; one line of content; round marks are
    24 px tinted circles; result shows a green check + elided transcript + "Pasted/Copied . N.Ns";
    OutBack open / OutCubic close; idle has no loops.
  - Dropped the filler hint "click to copy" (ReUI: no filler text).
  - New default accent, config migrated, docs screenshots re-shot.
- Verified: `--smoke` exit 0; real launch + simulated Right Ctrl -> recorded 1.09 s -> transcribed
  (silent mic: "Nothing heard"); sizes idle 115, listen 250, think 196, result 430, error 420, all 37 high.
- Next: commit and push.
