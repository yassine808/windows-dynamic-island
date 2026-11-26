# TODO — IslandWin

## Now

- [x] Auto-hide setting for windowed fullscreen apps (Rust foreground detection + Off/On switch)
- [x] Shortcut recorder: keyboard hook + webview capture, island takes the keyboard while recording,
      Escape cancels, a taken combo keeps the chip armed, live status in the chip
- [x] Live dictation words from the first session (no stale text, complete interim preview)
- [x] Endless page scroll: long scrolls reach the last page, no cooldown cap
- [x] Drop the local STT experiment, keep the Edge engine only
- [x] Settings page: Start with Windows switch (HKCU Run key, persisted)
- [x] Data outside the install dir: settings and logs live in `%APPDATA%\dev.yassine808.islandwin`
- [x] Installer: setup.exe (NSIS) with autostart on by default, data kept on upgrade/uninstall
- [x] Log writer never panics when its console disappears
- [x] CI: typecheck, frontend build, Rust check + tests, Apple-design check, secret scan
- [ ] Screenshots of the island only in README.md
- [ ] Make the repository public
- [ ] Final verification: CI green on GitHub, setup.exe built, record button assigns a shortcut,
      settings survive a restart

## Notes

- Settings live in `%APPDATA%\dev.yassine808.islandwin\settings.json` and UI prefs in the webview's
  localStorage, so an install over an old version keeps them.
- Logs live in `%APPDATA%\dev.yassine808.islandwin\logs\islandwin.log`; the old copy in
  `%LOCALAPPDATA%\islandwin` is migrated on first launch.
- The NSIS template has no custom checkbox page, so the setup asks one Yes/No question with Yes
  pre-selected (and silent installs take that default). Result: autostart is on out of the box.
