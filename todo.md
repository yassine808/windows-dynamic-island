# TODO — islandwin

## Interaction / windows

- [ ] **Show on every monitor** — the island only appears on the primary screen. It must be draggable onto any monitor and stay where it is dropped.
  - Now: the Tauri window is sized and positioned to `primary_monitor()` only (`main.rs` setup), so anything outside 1920×1080 is off-window.
  - Fix: cover the whole virtual desktop (bounding box of all monitors) and convert island coords accordingly.

- [ ] **Stick flush to the top when dragged there** — snapping to the top currently leaves a 10px gap. When dropped at the top, the island should sit at `y = 0`, no gap.
  - Now: `GAP = 10` is applied to every edge in `target()` (`src/App.tsx`).
  - Fix: use `0` for the top edge (keep the gap on the other edges unless asked otherwise).

- [ ] **Remove the ambient light around the island** — no glow/halo bleeding around the pill.
  - Now: `.glow` / `.halo` (blurred conic gradient) and the outer `box-shadow` in `src/styles.css`.
  - Fix: drop the halo layer and the ambient shadow; keep only the island's own shape.

- [ ] **Clicks on the island must not reach the app behind it** — clicking/dragging the island currently passes the click through to whatever window is underneath.
  - Now: the window is permanently click-through (`set_ignore_cursor_events(true)` in `main.rs`), so the OS never routes input to it.
  - Fix: toggle click-through off while the cursor is inside the island's hit rect (and on while outside), so the island swallows its own clicks.

## Known blocker (affects all four)

- [ ] **Pointer input reaches the UI at all** — hover, drag and media buttons do nothing today. `main.rs` updates a shared `Hit` struct that the UI polls via `get_ptr`, but nothing is being acted on. Must be confirmed working before the items above can be tested.
