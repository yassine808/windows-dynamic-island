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
- Drag the island: it flicks and snaps to the nearest screen edge (magnetic at center). On left/right/bottom it turns vertical and tucks away when idle.
- Art-colored glow, Siri-style glow + live mic bars while listening.
- `Ctrl+Alt+Space` toggles dictation (also the mic button on page 2). Finished phrases are typed into whatever text field has focus. Needs Settings > Privacy > Speech > Online speech recognition.
- Scroll the mouse wheel over the island to switch pages (Media / Dictation). Words appear live on the Dictation page as you speak.
- Cursor stays the normal arrow while hovering; it only becomes a grabbing hand during an actual drag.
- Set `SCRUBBER = false` in `src/App.tsx` to hide the elapsed/remaining bar (bare art + title + controls layout).

## Next
- Whisper STT: stream mic PCM to whisper.cpp, emit `stt-partial` events.
