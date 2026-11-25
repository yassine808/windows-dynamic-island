#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod media;

use std::{sync::{Arc, Mutex}, thread, time::Duration};
use tauri::{Emitter, Manager, PhysicalPosition, PhysicalSize, State};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

#[derive(Default)]
struct Hit { r: [f64; 4], dragging: bool, p: (f64, f64, bool, bool) }
type Shared = Arc<Mutex<Hit>>;

#[cfg(windows)]
fn lbutton() -> bool {
    unsafe { (windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState(0x01) as u16) & 0x8000 != 0 }
}
#[cfg(not(windows))]
fn lbutton() -> bool { false }

// Cursor position relative to the window's top-left, in physical px. Pure Win32: never waits on the UI thread.
#[cfg(windows)]
fn cursor_rel(h: isize) -> Option<(f64, f64)> {
    use windows::Win32::{Foundation::{HWND, POINT, RECT}, UI::WindowsAndMessaging::{GetCursorPos, GetWindowRect}};
    unsafe {
        let (mut p, mut r) = (POINT::default(), RECT::default());
        GetCursorPos(&mut p).ok()?;
        GetWindowRect(HWND(h as *mut _), &mut r).ok()?;
        Some(((p.x - r.left) as f64, (p.y - r.top) as f64))
    }
}
#[cfg(not(windows))]
fn cursor_rel(_: isize) -> Option<(f64, f64)> { None }

#[tauri::command]
fn set_hit_rect(st: State<Shared>, x: f64, y: f64, w: f64, h: f64) { st.lock().unwrap().r = [x, y, w, h]; }

#[tauri::command]
async fn get_ptr(st: State<'_, Shared>) -> Result<serde_json::Value, ()> {
    let p = st.lock().unwrap().p;
    Ok(serde_json::json!({ "x": p.0, "y": p.1, "down": p.2, "inside": p.3 }))
}

#[tauri::command]
fn log(msg: String) { println!("[js] {msg}"); }

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
            media::media_poll, media::media_art, media::media_control, set_hit_rect, set_dragging, log, get_ptr
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

            // Window stays fully click-through. Rust reads the mouse and forwards it to the UI.
            let hwnd = win.hwnd()?.0 as isize;
            let scale = win.scale_factor().unwrap_or(1.0);
            println!("[rust] mouse thread starting, hwnd={hwnd} scale={scale}");
            thread::spawn(move || {
                let mut n = 0u32;
                loop {
                    thread::sleep(Duration::from_millis(8));
                    n += 1;
                    let Some((px, py)) = cursor_rel(hwnd) else {
                        if n % 120 == 0 { println!("[rust] cursor read failed"); }
                        continue;
                    };
                    let (x, y) = (px / scale, py / scale);
                    let r = shared.lock().unwrap().r;
                    let pad = 10.0;
                    let inside = r[2] > 0.0 && x >= r[0] - pad && x <= r[0] + r[2] + pad && y >= r[1] - pad && y <= r[1] + r[3] + pad;
                    let down = lbutton();
                    shared.lock().unwrap().p = (x, y, down, inside);
                    if n % 120 == 0 { println!("[rust] cursor=({:.0},{:.0}) rect={:?} inside={} down={}", x, y, r, inside, down); }
                }
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running IslandWin");
}
