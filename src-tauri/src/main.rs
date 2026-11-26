#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod hotkey;
mod media;
mod stt;

use std::{sync::{Arc, Mutex}, thread, time::Duration};
use tauri::{Manager, PhysicalPosition, PhysicalSize, State};
use tauri_plugin_global_shortcut::ShortcutState;

#[derive(Default)]
struct Hit { r: [f64; 4], dragging: bool, p: (f64, f64, bool, bool), mons: Vec<[f64; 4]> }
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
async fn get_monitors(st: State<'_, Shared>) -> Result<Vec<[f64; 4]>, ()> { Ok(st.lock().unwrap().mons.clone()) }

#[tauri::command]
fn log(msg: String) { println!("[js] {msg}"); }

#[tauri::command]
fn set_dragging(st: State<Shared>, on: bool) { st.lock().unwrap().dragging = on; }

fn main() {
    let shared: Shared = Arc::new(Mutex::new(Hit::default()));
    tauri::Builder::default()
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|_app, _sc, ev| match ev.state() {
                    // hold = push-to-talk (listen while the key is down); otherwise press to start, press to stop
                    ShortcutState::Pressed => if stt::hold() { stt::start() } else { stt::toggle() },
                    ShortcutState::Released => if stt::hold() { stt::stop() },
                })
                .build(),
        )
        .manage(shared.clone())
        .invoke_handler(tauri::generate_handler![
            media::media_poll, media::media_art, media::media_control, media::media_seek, set_hit_rect, set_dragging, log, get_ptr, get_monitors, stt::stt_state, stt::stt_toggle,
            hotkey::stt_settings, hotkey::set_stt_hold, hotkey::record_shortcut, hotkey::cancel_record_shortcut
        ])
        .setup(move |app| {
            hotkey::init(app.handle());

            // Transparent overlay over the whole primary monitor; the island lives inside it.
            let win = app.get_webview_window("main").unwrap();
            // One transparent overlay spanning every monitor.
            let mons = win.available_monitors()?;
            let prim = win.primary_monitor()?.map(|m| (m.position().x, m.position().y));
            let minx = mons.iter().map(|m| m.position().x).min().unwrap_or(0);
            let miny = mons.iter().map(|m| m.position().y).min().unwrap_or(0);
            let maxx = mons.iter().map(|m| m.position().x + m.size().width as i32).max().unwrap_or(1920);
            let maxy = mons.iter().map(|m| m.position().y + m.size().height as i32).max().unwrap_or(1080);
            win.set_position(PhysicalPosition::new(minx, miny))?;
            win.set_size(PhysicalSize::new((maxx - minx) as u32, (maxy - miny) as u32))?;
            let scale = win.scale_factor().unwrap_or(1.0);
            let mut list: Vec<([f64; 4], bool)> = mons.iter().map(|m| (
                [(m.position().x - minx) as f64 / scale, (m.position().y - miny) as f64 / scale, m.size().width as f64 / scale, m.size().height as f64 / scale],
                Some((m.position().x, m.position().y)) == prim,
            )).collect();
            list.sort_by_key(|(_, p)| !*p);
            shared.lock().unwrap().mons = list.into_iter().map(|(r, _)| r).collect();
            win.set_ignore_cursor_events(true)?;

            // Window stays fully click-through. Rust reads the mouse and forwards it to the UI.
            let hwnd = win.hwnd()?.0 as isize;
            let winc = win.clone();
            println!("[rust] mouse thread starting, hwnd={hwnd} scale={scale}");
            thread::spawn(move || {
                let mut n = 0u32;
                let mut interactive = false;
                loop {
                    thread::sleep(Duration::from_millis(8));
                    n += 1;
                    let Some((px, py)) = cursor_rel(hwnd) else {
                        if n % 120 == 0 { println!("[rust] cursor read failed"); }
                        continue;
                    };
                    let (x, y) = (px / scale, py / scale);
                    let (r, dragging) = { let g = shared.lock().unwrap(); (g.r, g.dragging) };
                    let pad = 10.0;
                    let inside = r[2] > 0.0 && x >= r[0] - pad && x <= r[0] + r[2] + pad && y >= r[1] - pad && y <= r[1] + r[3] + pad;
                    let down = lbutton();
                    shared.lock().unwrap().p = (x, y, down, inside);
                    // Over the island the window takes the mouse, so clicks never reach what's behind it.
                    let over = r[2] > 0.0 && x >= r[0] - 1.0 && x <= r[0] + r[2] + 1.0 && y >= r[1] - 1.0 && y <= r[1] + r[3] + 1.0;
                    let want = over || dragging;
                    if want != interactive { interactive = want; let _ = winc.set_ignore_cursor_events(!want); }
                    if n % 120 == 0 { println!("[rust] cursor=({:.0},{:.0}) rect={:?} inside={} down={}", x, y, r, inside, down); }
                }
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running IslandWin");
}
