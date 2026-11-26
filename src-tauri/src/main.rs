#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// One line to the log file (logger.rs) and the console.
macro_rules! logf {
    ($tag:expr, $($arg:tt)*) => { crate::logger::write($tag, &format!($($arg)*)) };
}

mod hotkey;
mod logger;
mod media;
mod mixer;
mod stats;
mod stt;
mod tools;

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

// Is the foreground window a fullscreen app/game (covers its whole monitor, not the desktop or taskbar)?
#[cfg(windows)]
fn fullscreen_app(own: isize) -> bool {
    use windows::Win32::{
        Foundation::{HWND, RECT},
        Graphics::Gdi::{GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST},
        UI::WindowsAndMessaging::{GetClassNameW, GetForegroundWindow, GetWindowRect, IsIconic, IsWindowVisible},
    };
    unsafe {
        let fg: HWND = GetForegroundWindow();
        if fg.0.is_null() || fg.0 as isize == own || !IsWindowVisible(fg).as_bool() || IsIconic(fg).as_bool() { return false; }
        let mut cls = [0u16; 64];
        let n = GetClassNameW(fg, &mut cls).max(0) as usize;
        if matches!(String::from_utf16_lossy(&cls[..n]).as_str(), "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd") { return false; }
        let mut r = RECT::default();
        if GetWindowRect(fg, &mut r).is_err() { return false; }
        let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        if !GetMonitorInfoW(MonitorFromWindow(fg, MONITOR_DEFAULTTONEAREST), &mut mi).as_bool() { return false; }
        let m = mi.rcMonitor;
        r.left <= m.left && r.top <= m.top && r.right >= m.right && r.bottom >= m.bottom
    }
}
// Is the system cursor currently visible? (Games hide it during gameplay and show it in menus.)
#[cfg(windows)]
fn cursor_visible() -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{GetCursorInfo, CURSORINFO, CURSOR_SHOWING};
    unsafe {
        let mut ci = CURSORINFO { cbSize: std::mem::size_of::<CURSORINFO>() as u32, ..Default::default() };
        GetCursorInfo(&mut ci).is_ok() && (ci.flags.0 & CURSOR_SHOWING.0) != 0
    }
}
#[cfg(not(windows))]
fn cursor_visible() -> bool { true }

#[cfg(not(windows))]
fn fullscreen_app(_: isize) -> bool { false }

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
fn log(msg: String) { logf!("js", "{msg}"); }

#[tauri::command]
fn set_dragging(st: State<Shared>, on: bool) { st.lock().unwrap().dragging = on; }

fn main() {
    logger::init();
    std::panic::set_hook(Box::new(|info| {
        let bt = std::backtrace::Backtrace::force_capture();
        logf!("panic", "{info}\n{bt}");
    }));
    let shared: Shared = Arc::new(Mutex::new(Hit::default()));
    tauri::Builder::default()
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|_app, _sc, ev| match ev.state() {
                    // hold = push-to-talk (listen while the key is down); otherwise press to start, press to stop
                    ShortcutState::Pressed => {
                        logf!("hotkey", "shortcut pressed (hold={})", stt::hold());
                        if stt::hold() { stt::start() } else { stt::toggle() }
                    }
                    ShortcutState::Released => {
                        if stt::hold() { logf!("hotkey", "shortcut released (hold) -> stop"); stt::stop() }
                    }
                })
                .build(),
        )
        .manage(shared.clone())
        .invoke_handler(tauri::generate_handler![
            media::media_poll, media::media_art, media::media_control, media::media_seek, set_hit_rect, set_dragging, log, get_ptr, get_monitors, stt::stt_state, stt::stt_toggle,
            hotkey::stt_settings, hotkey::set_stt_hold, hotkey::record_shortcut, hotkey::cancel_record_shortcut,
            stats::stats, mixer::mixer_get, mixer::mixer_set_vol, mixer::mixer_set_mute,
            tools::clip_list, tools::clip_copy, tools::clip_clear, tools::set_clipboard, tools::pixel_at_cursor, tools::screenshot
        ])
        .setup(move |app| {
            logf!("win", "setup: initialising the dictation shortcut");
            hotkey::init(app.handle());
            logf!("win", "setup: starting stats and tools");
            stats::start();
            tools::init();

            // Transparent overlay over the whole primary monitor; the island lives inside it.
            let win = app.get_webview_window("main").unwrap();
            // One transparent overlay spanning every monitor.
            let mons = win.available_monitors()?;
            for m in &mons {
                logf!("win", "monitor {}x{} at {},{} scale {:.2}", m.size().width, m.size().height, m.position().x, m.position().y, m.scale_factor());
            }
            let prim = win.primary_monitor()?.map(|m| (m.position().x, m.position().y));
            let minx = mons.iter().map(|m| m.position().x).min().unwrap_or(0);
            let miny = mons.iter().map(|m| m.position().y).min().unwrap_or(0);
            let maxx = mons.iter().map(|m| m.position().x + m.size().width as i32).max().unwrap_or(1920);
            let maxy = mons.iter().map(|m| m.position().y + m.size().height as i32).max().unwrap_or(1080);
            win.set_position(PhysicalPosition::new(minx, miny))?;
            win.set_size(PhysicalSize::new((maxx - minx) as u32, (maxy - miny) as u32))?;
            let scale = win.scale_factor().unwrap_or(1.0);
            logf!("win", "overlay {}x{} at {},{} (scale {:.2}, primary {prim:?})", maxx - minx, maxy - miny, minx, miny, scale);
            let mut list: Vec<([f64; 4], bool)> = mons.iter().map(|m| (
                [(m.position().x - minx) as f64 / scale, (m.position().y - miny) as f64 / scale, m.size().width as f64 / scale, m.size().height as f64 / scale],
                Some((m.position().x, m.position().y)) == prim,
            )).collect();
            list.sort_by_key(|(_, p)| !*p);
            shared.lock().unwrap().mons = list.into_iter().map(|(r, _)| r).collect();
            win.set_ignore_cursor_events(true)?;

            // Window stays fully click-through. Rust reads the mouse and forwards it to the UI.
            let hwnd = win.hwnd()?.0 as isize;
            tools::OVERLAY.store(hwnd, std::sync::atomic::Ordering::Relaxed);
            let winc = win.clone();
            logf!("win", "mouse thread starting, hwnd={hwnd} scale={scale}");
            thread::spawn(move || {
                let mut n = 0u32;
                let mut interactive = false;
                let mut hidden = false;
                let mut fs = false;
                loop {
                    thread::sleep(Duration::from_millis(8));
                    n += 1;
                    // fullscreen game in front AND its cursor is hidden (mouse-look gameplay): island stays visible
                    // but never takes the mouse. If the cursor is showing (menus), the island stays usable.
                    if n % 25 == 0 { fs = fullscreen_app(hwnd); }
                    let sup = fs && !cursor_visible();
                    if sup != hidden {
                        hidden = sup;
                        if sup {
                            interactive = false;
                            let _ = winc.set_ignore_cursor_events(true);
                            let mut g = shared.lock().unwrap();
                            g.dragging = false;
                            g.p = (-1e6, -1e6, false, false);
                            logf!("ptr", "fullscreen app with a hidden cursor: island input paused");
                        } else {
                            logf!("ptr", "fullscreen app gone: island input resumed");
                        }
                    }
                    if hidden { continue; }
                    let Some((px, py)) = cursor_rel(hwnd) else { continue };
                    let (x, y) = (px / scale, py / scale);
                    let (r, dragging) = { let g = shared.lock().unwrap(); (g.r, g.dragging) };
                    let pad = 10.0;
                    let inside = r[2] > 0.0 && x >= r[0] - pad && x <= r[0] + r[2] + pad && y >= r[1] - pad && y <= r[1] + r[3] + pad;
                    let down = lbutton();
                    shared.lock().unwrap().p = (x, y, down, inside);
                    // Over the island the window takes the mouse, so clicks never reach what's behind it.
                    let over = r[2] > 0.0 && x >= r[0] - 1.0 && x <= r[0] + r[2] + 1.0 && y >= r[1] - 1.0 && y <= r[1] + r[3] + 1.0;
                    let want = over || dragging;
                    if want != interactive {
                        interactive = want;
                        let _ = winc.set_ignore_cursor_events(!want);
                        logf!("ptr", "click-through {} at ({x:.0},{y:.0})", if want { "off (island takes the mouse)" } else { "on" });
                    }
                }
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running IslandWin");
}
