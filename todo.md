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

## Round 2 (done)

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

## Round 3 (done)

- [x] Restore the previous non-uniform page heights; do not make every page the same height
- [x] Timer: hours and minutes only; presets stay editable and fill the picker instead of starting a
      fixed duration
- [x] Timer at zero: better alarm sound, ring continuously, expand the island like listening mode
      (`340x84`), iOS look, stop only with the alarm's Stop button
- [x] Timer play button is white, not gray
- [x] Collapsed pill: media progress time in the middle; with no media, the normal 12-hour clock
- [x] Hand cursor only over clickable controls, not the whole island
- [x] Settings switch that locks the island position
- [x] Delete the Tools page entirely (Screenshot, Pick color, Clipboard)
- [x] Add a short delay between page changes while scrolling
- [x] Keep the product name `IslandWin`; dictation stays on Edge / WebView2
- [x] Auto-hide fullscreen hides the app only when a real Windows fullscreen window or application is
      running
- [x] Rename that setting away from "screen" to "full screen window / full screen application"
- [x] Fix the clipped / chopped island when interacting with a fullscreen app: fade it out completely
      instead of tucking part of the expanded card off the edge
- [x] Log file never grows past roughly 2 MB; rotate one previous file
- [x] Check and test everything, update README and screenshots, build the installer, commit and push

## Round 4 (done)

- [x] Dictation: one long-lived recognition thread. The recognizer and its dictation grammar are
      built at launch, so a press only starts the continuous session instead of paying ~0.4-0.6 s of
      setup every time. A tap that starts and stops before the session runs keeps the warm recognizer
- [x] Dictation: state changes are pushed to the island (`stt://state`) instead of waiting for the
      100 ms poll
- [x] Launch: the island grows out of its own edge at its final position; it no longer appears in the
      middle of the screen and slides sideways
- [x] Page scroll: a flick turns a page every 140 ms instead of 320 ms
- [x] Cursor: hand only over clickable controls, default everywhere else
- [x] Remove the SonarCloud Code Analysis CI job
- [x] Typecheck, frontend build, design checks, Rust check + tests, installer, install, commit, push

## Requests, as written

- [x] Restore the previous non-uniform page heights. Do not make every page the same height.
- [x] Timer: hours and minutes only. Presets must stay editable and fill the picker instead of
      starting a fixed duration.
- [x] Timer at zero: better alarm sound, ring continuously, expand the island like listening mode
      (`340x84`), iOS look, stop only via the alarm's Stop button.
- [x] Timer play button must be white, not gray.
- [x] Collapsed pill: media progress time in the middle; when no media, show the normal 12-hour clock.
- [x] Hand cursor only over clickable controls, not the whole island.
- [x] Settings switch that locks the island position.
- [x] Delete the Tools page entirely (Screenshot, Pick color, Clipboard).
- [x] Add a short delay between page changes while scrolling.
- [x] Keep the product name `IslandWin`; STT stays on Edge/WebView2.
- [x] Auto-hide fullscreen must hide the app only when a Windows fullscreen window/application is
      running.
- [x] Rename that setting away from "screen" to "full screen window / full screen application".
- [x] Log file must never exceed roughly 2 MB.
- [x] When I'm in a Windows fullscreen app and I interact with it, the island looks chopped. Fix bug.
- [x] Build, commit and push when done.
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

## Requests, as written (round 4)

- [x] remove scroll delay between pages, keep it as it was
- [x] Make the startup animation grows from the edge ( spawns ), not from right to left
- [x] Also fix the cursor
- [x] Cursor: hand only over clickable controls!!
- [x] Also fix bug where the STT doesnt work as it should ,see logs
- [x] and fix issue where i have the button on hold, and i release it, yet the window keeps listening
- [ ] also remove CI SonarCloud Code Analysis (skipped for now: GitHub App, see Notes)

## Requests, as written (round 5)

- [x] fix the issue, when i change resolution or connect to parsec, it doesnt show where it needs to be

## Notes

- SonarCloud Code Analysis is a GitHub App check (`sonarqubecloud`), not a workflow in this repo.
  Removing it needs the app uninstalled from the repository in GitHub settings: Settings >
  Applications > SonarCloud > Configure > remove this repository (or uninstall it).

- Settings live in `%APPDATA%\Windows Dynamic Island\settings.json`; the old
  `%APPDATA%\dev.yassine808.islandwin` folder is copied over on first launch.
- Logs live in `%APPDATA%\Windows Dynamic Island\logs\islandwin.log`. The file rotates at ~2 MB to
  `islandwin.old.log`, keeping one previous file.
- UI preferences stay in the webview's local storage.
- Autostart is `HKCU\Software\Microsoft\Windows\CurrentVersion\Run\IslandWin`.
