#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod media;

use std::{sync::{Arc, Mutex}, thread, time::Duration};
use tauri::{Emitter, Manager, PhysicalPosition, PhysicalSize, State};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

#[derive(Default)]
struct Hit { r: [f64; 4], dragging: bool }
type Shared = Arc<Mutex<Hit>>;

#[tauri::command]
fn set_hit_rect(st: State<Shared>, x: f64, y: f64, w: f64, h: f64) { st.lock().unwrap().r = [x, y, w, h]; }

#[tauri::command]
fn set_dragging(st: State<Shared>, on: bool) { st.lock().unwrap().dragging = on; }

fn main() {
    let shared: Shared = Arc::new(Mutex::new(Hit::default()));
    tauri::Builder::default()
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _sc, ev| {
                    if ev.state() == ShortcutState::Pressed { let _ = app.emit("toggle-listen", ()); }
                })
                .build(),
        )
        .manage(shared.clone())
        .invoke_handler(tauri::generate_handler![
            media::media_poll, media::media_art, media::media_control, set_hit_rect, set_dragging
        ])
        .setup(move |app| {
            app.global_shortcut()
                .register(Shortcut::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::Space))?;

            // Transparent overlay over the whole primary monitor; the island lives inside it.
            let win = app.get_webview_window("main").unwrap();
            if let Some(m) = win.primary_monitor()? {
                win.set_position(PhysicalPosition::new(m.position().x, m.position().y))?;
                win.set_size(PhysicalSize::new(m.size().width, m.size().height))?;
            }
            win.set_ignore_cursor_events(true)?;

            // Click-through everywhere except over the island. Also drives hover.
            let handle = app.handle().clone();
            thread::spawn(move || {
                let (mut interactive, mut hovered) = (false, false);
                loop {
                    thread::sleep(Duration::from_millis(16));
                    let (Ok(c), Ok(p)) = (win.cursor_position(), win.outer_position()) else { continue };
                    let s = win.scale_factor().unwrap_or(1.0);
                    let (x, y) = ((c.x - p.x as f64) / s, (c.y - p.y as f64) / s);
                    let (r, dragging) = { let g = shared.lock().unwrap(); (g.r, g.dragging) };
                    let pad = 10.0;
                    let inside = r[2] > 0.0 && x >= r[0] - pad && x <= r[0] + r[2] + pad && y >= r[1] - pad && y <= r[1] + r[3] + pad;
                    let want = inside || dragging;
                    if want != interactive { interactive = want; let _ = win.set_ignore_cursor_events(!want); }
                    if inside != hovered { hovered = inside; let _ = handle.emit("hover", inside); }
                }
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running IslandWin");
}
