# IslandWin

Dynamic Island for Windows. Media controls + speech-to-text.

## Run
1. Install Node, Rust, and WebView2 (preinstalled on Windows 11).
2. `npm install`
3. `npm run tauri icon path/to/logo.png` (once, makes real icons)
4. `npm run tauri dev`

## Use
- Play music in Spotify or a browser: the island shows it.
- Hover to expand, use prev / play / next.
- `Ctrl+Alt+Space` toggles listening.

## Next
- Whisper STT: stream mic PCM to whisper.cpp, emit `stt-partial` events.
- Click-through when idle.
- Live mic level bars.
