# TODO - IslandWin

## Previous round (done)

- [x] Show live dictation words as you speak
- [x] Harden the shortcut recorder: capture over fullscreen and elevated windows, keep the chip armed
- [x] Auto-hide when a windowed fullscreen app is in front
- [x] Endless scroll: a long scroll reaches the last page
- [x] Start with Windows switch on the Settings page
- [x] NSIS `setup.exe` installer
- [x] CI: typecheck, frontend build, Apple-design check, Rust check + tests, secret scan
- [x] README with screenshots
- [x] Public GitHub repo and the `v0.1.0` release
- [x] Install the app

## This round

- [x] Rename the app data folder to `Windows Dynamic Island` and migrate the old settings + log
- [x] Settings: rename "Fullscreen" so it clearly means auto-hide when a fullscreen app is open
- [x] Shortcut recorder: accept every keyboard key and every mouse button, bare keys included, never left click
- [x] Dictation shortcut: add a red Reset button next to the record chip (back to `Ctrl+Alt+Space`)
- [x] Mic report: fix why it does not show in the app, and show it with an iOS-style design like the screenshots
- [x] Player bar: fix the minus and the inaccurate time
- [x] Give every page the same height, without compacting anything
- [x] Add the app icon to the Windows notification area (bottom-right system tray): left click opens
      the Settings page, right click opens a menu with Settings, Start with Windows and Quit
- [x] Build the app
- [x] Update README and screenshots
- [x] Run typecheck, frontend build, Apple-design check, Rust check + tests, then commit and push

## Requests, as written

- [x] Rename the AppData folder to `Windows Dynamic Island`.
- [x] Settings currently says "Fullscreen"; rename it so it clearly means auto-hide when a fullscreen app is open.
- [x] Record any keyboard key and any mouse button, with no restrictions (bare keys allowed); never use left click.
- [x] Add a red reset button next to the record button; restore the default shortcut.
- [x] Fix the bug where the mic report does not show in the app; show it like the screenshots, with an
  iOS-like design.
- [x] Fix a bug in the player bar: it shows a minus and it's not accurate.
- [x] Give all the pages the same height. Do not compact anything. Keep the design good and iOS-like.
- [x] Add the bar icon you can see in the bottom right of Windows (the system tray). Left click it and
  the island shows Settings; right click it and there is a menu, maybe with some other settings.
- [x] Build the app when done.
- [x] Write everything from this prompt and the previous one in `todo.md`, exactly as written, with check
  boxes. When finished, check the boxes and push to git.

## Notes

- Settings live in `%APPDATA%\Windows Dynamic Island\settings.json`; the old
  `%APPDATA%\dev.yassine808.islandwin` folder is copied over on first launch.
- Logs live in `%APPDATA%\Windows Dynamic Island\logs\islandwin.log`.
- UI preferences stay in the webview's local storage.
- Autostart is `HKCU\Software\Microsoft\Windows\CurrentVersion\Run\IslandWin`.
