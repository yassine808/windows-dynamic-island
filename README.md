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

## Pages
Scroll over the island (or click the dots) to switch: Media, Dictation, Timer, Mixer, Stats, Tools.
- Mixer: speaker and mic volume sliders with mute buttons (default devices).
- Stats: CPU, RAM, network, and GPU (NVIDIA only, needs `nvidia-smi`; shows n/a otherwise).
- Tools: Screenshot saves all monitors to Pictures/IslandWin (the island is left out). Pick color: move over any pixel, click to copy its hex; that click also reaches the app underneath. Clipboard keeps your last copied text (password-manager copies are skipped), click an entry to copy it again, Clear wipes it. History is in memory only.
- Fullscreen games: DirectX/exclusive fullscreen can make screenshots black.

## Next
- Whisper STT: stream mic PCM to whisper.cpp, emit `stt-partial` events.
