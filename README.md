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
- Drag the island: it flicks and snaps to the nearest screen edge. Near the middle of the screen it sticks to the center (like Word's guides) until you pull away. On left/right/bottom it turns vertical and tucks away when idle.
- Art-colored glow, Siri-style glow + live mic bars while listening.
- Dictation shortcut defaults to `Ctrl+Alt+Space`. On page 2, click the shortcut chip and press your new keys to change it; the Hold / Toggle switch picks push-to-talk or press-to-start/stop. Saved to your app config folder. (Also the mic button on page 2.)
- Click or drag the progress bar to seek. Finished phrases are typed into whatever text field has focus. Needs Settings > Privacy > Speech > Online speech recognition.
- Scroll the mouse wheel over the island to switch pages (Media / Dictation / Timer). Timer: pick 5, 15 or 25 min (starts right away), pause with the button; it keeps running in the collapsed island and beeps when done. Words appear live on the Dictation page as you speak.
- Cursor stays the normal arrow while hovering; it only becomes a grabbing hand during an actual drag.
- Set `SCRUBBER = false` in `src/App.tsx` to hide the elapsed/remaining bar (bare art + title + controls layout).

## Next
- Whisper STT: stream mic PCM to whisper.cpp, emit `stt-partial` events.
