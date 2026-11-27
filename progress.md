# Progress — windows-dynamic-island

## Plan
- [x] P0: shortcut key blocklist, hit-rect unification, recording stops dictation, late-result guard, parked-pointer guard, transcript-on-success, useMic cancel, hold intent
- [x] P1: DPI refresh, atomic settings, STT fatal set + restart reset + stop-stuck, AltGr, thumbnail cap, art tri-state, set_box lock, hold commit, tint guard, StartAsync timeout
- [x] P2: numpad/NumLock, grab_focus on main thread, CSP, net rate, mixer errors + async, nvidia-smi timeout/path, installer autostart, StartupApproved, single-instance order, empty Run guard, fullscreen cache, migration logs, recorder pump, media deadlines, stt poll, contrast
- [x] Verify: cargo test, tsc, vite build, Tauri bundle
- [x] Commit and push
- [x] Add the Ambient light setting

## Log

### 2026-11-27 — Fix audit findings
- Goal: implement the judged fixes from the four-agent audit, then test.
- Done: all P0/P1/P2 edits across Rust and React; `cargo check --all-targets` clean; `cargo test` 8/8; `npm run build` clean.
- Verified: `cargo test` — 8 passed, 0 failed (includes bare-key rejection and shortcut round-trip). `npm run build` — tsc + vite production build succeeded.
- Next: finish the Tauri bundle build, review the diff, commit by group, push.

### 2026-11-27 — Final verification
- Goal: prove the release bundle runs under the new production CSP, then ship.
- Done: rebuilt `IslandWin_0.1.3_x64-setup.exe`; swapped the installed instance for the fresh release exe for 10 s and restored the installed app; added `stt_stop` so a hold release cannot start a failed session; moved layout re-placement to the main thread to avoid a cross-thread `SetWindowPos`/mutex deadlock; keyed the fullscreen cache on the window rect; dropped the bare-name `nvidia-smi` fallback.
- Verified: fresh exe log shows `[js] stt: browser engine available`, `[js] mic permission granted`, `[js] spawn: seeded`, `[js] fullscreen`, `[stats] GPU detected through nvidia-smi`, `[win] island box`, and no frontend listener failures. `cargo test` — 8 passed. `npm run tauri build` — bundle at `src-tauri/target/release/bundle/nsis/IslandWin_0.1.3_x64-setup.exe` (2.42 MiB). Installed app restored; HKCU Run value back to the installed path.
- Next: none.

### 2026-11-27 — Ambient light setting
- Goal: add a Settings switch for the cover-colour glow around the island.
- Done: persisted `ambient` preference (default on), gated the halo tint and opacity while keeping the waveform accent, added the Settings row, and tightened the settings row gap so six rows fit the existing 286 px page. Also committed the CSP hardening left from the previous session.
- Verified: `npm run typecheck`, `npm run build`, `node scripts/design-check.mjs` all pass. Headless-browser check at 530x446: the six-row Settings page fits, Off stores `ambient=off` and survives a reload, glow opacity stays 0; On restores opacity 0.16.
- Next: none.
