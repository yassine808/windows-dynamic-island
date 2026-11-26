#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// One line to the log file (logger.rs) and the console.
macro_rules! logf {
    ($tag:expr, $($arg:tt)*) => { crate::logger::write($tag, &format!($($arg)*)) };
}

mod autostart;
mod hotkey;
mod logger;
mod media;
mod mixer;
mod paths;
mod stats;
mod stt;
mod tray;

use std::{
    sync::{
        atomic::{AtomicIsize, Ordering},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};
use tauri::{Emitter, Manager, PhysicalPosition, PhysicalSize, State};

/// The overlay's HWND, set once at setup; dictation focuses it while recording a shortcut.
pub static OVERLAY: AtomicIsize = AtomicIsize::new(0);

#[derive(Default)]
struct Hit { r: [f64; 4], dragging: bool, p: (f64, f64, bool, bool), mons: Vec<[f64; 4]> }
type Shared = Arc<Mutex<Hit>>;

/// Pointer snapshot pushed to the webview; the UI no longer polls for it.
#[derive(Clone, serde::Serialize)]
struct Ptr { x: f64, y: f64, down: bool, inside: bool }

#[cfg(windows)]
fn lbutton() -> bool {
    unsafe { (windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState(0x01) as u16) & 0x8000 != 0 }
}
#[cfg(not(windows))]
fn lbutton() -> bool { false }

// Cursor position relative to the overlay's top-left, in physical px. The window never moves, so the
// origin is cached once instead of asking the window manager on every tick.
#[cfg(windows)]
fn cursor_at(ox: f64, oy: f64) -> Option<(f64, f64)> {
    use windows::Win32::{Foundation::POINT, UI::WindowsAndMessaging::GetCursorPos};
    unsafe {
        let mut p = POINT::default();
        GetCursorPos(&mut p).ok()?;
        Some((p.x as f64 - ox, p.y as f64 - oy))
    }
}
#[cfg(not(windows))]
fn cursor_at(_: f64, _: f64) -> Option<(f64, f64)> { None }

// Is the foreground window a real fullscreen app/game, rather than a normal maximized window?
#[cfg(windows)]
fn covers_monitor(r: windows::Win32::Foundation::RECT, m: windows::Win32::Foundation::RECT) -> bool {
    // within a couple of pixels for DPI rounding
    let tol = 2;
    r.left <= m.left + tol && r.top <= m.top + tol && r.right >= m.right - tol && r.bottom >= m.bottom - tol
}

/// A window is fullscreen when its visible frame covers the whole monitor and it is not a framed
/// (title-bar) window in the maximized state. The frame check matters on Windows 11 with an
/// auto-hiding taskbar, where a maximized normal window also reaches the screen edges.
#[cfg(windows)]
fn fullscreen_rect(maximized: bool, framed: bool, visible: windows::Win32::Foundation::RECT, monitor: windows::Win32::Foundation::RECT) -> bool {
    covers_monitor(visible, monitor) && !(maximized && framed)
}

#[cfg(windows)]
fn fullscreen_app(own: isize) -> bool {
    use windows::Win32::{
        Foundation::{HWND, RECT},
        Graphics::{
            Dwm::{DwmGetWindowAttribute, DWMWA_EXTENDED_FRAME_BOUNDS},
            Gdi::{GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST},
        },
        UI::WindowsAndMessaging::{
            GetClassNameW, GetForegroundWindow, GetWindowLongW, GetWindowPlacement, GetWindowRect,
            IsIconic, IsWindowVisible, GWL_STYLE, SW_SHOWMAXIMIZED, WINDOWPLACEMENT, WS_CAPTION,
        },
    };
    unsafe {
        let fg: HWND = GetForegroundWindow();
        if fg.0.is_null() || fg.0 as isize == own || !IsWindowVisible(fg).as_bool() || IsIconic(fg).as_bool() { return false; }
        let mut cls = [0u16; 64];
        let n = GetClassNameW(fg, &mut cls).max(0) as usize;
        if matches!(String::from_utf16_lossy(&cls[..n]).as_str(), "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd") { return false; }
        // A maximized normal window is not a fullscreen app. GetWindowRect includes its invisible
        // resize border (which reaches past the monitor edges), so testing that alone made every
        // maximized window hide the island; the DWM frame below is the visible rectangle instead.
        let mut wp = WINDOWPLACEMENT { length: std::mem::size_of::<WINDOWPLACEMENT>() as u32, ..Default::default() };
        let maximized = GetWindowPlacement(fg, &mut wp).is_ok() && wp.showCmd == SW_SHOWMAXIMIZED.0 as u32;
        let framed = (GetWindowLongW(fg, GWL_STYLE) as u32) & WS_CAPTION.0 != 0;
        let mut r = RECT::default();
        if DwmGetWindowAttribute(fg, DWMWA_EXTENDED_FRAME_BOUNDS, &mut r as *mut _ as *mut _, std::mem::size_of::<RECT>() as u32).is_err() {
            if GetWindowRect(fg, &mut r).is_err() { return false; }
        }
        let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        if !GetMonitorInfoW(MonitorFromWindow(fg, MONITOR_DEFAULTTONEAREST), &mut mi).as_bool() { return false; }
        fullscreen_rect(maximized, framed, r, mi.rcMonitor)
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

// A second copy would double every sampler and add a second full-desktop overlay: refuse to start.
#[cfg(windows)]
fn already_running() -> bool {
    use windows::core::w;
    use windows::Win32::{
        Foundation::{GetLastError, ERROR_ALREADY_EXISTS},
        System::Threading::CreateMutexW,
    };
    unsafe {
        if CreateMutexW(None, true, w!("Local\\islandwin-single-instance")).is_err() { return false; }
        GetLastError() == ERROR_ALREADY_EXISTS
    }
}
#[cfg(not(windows))]
fn already_running() -> bool { false }

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
    paths::migrate();
    logger::init();
    logf!("boot", "data folder: {}", paths::data_dir().display());
    if already_running() {
        logf!("boot", "another islandwin instance is already running, exiting");
        return;
    }
    std::panic::set_hook(Box::new(|info| {
        let bt = std::backtrace::Backtrace::force_capture();
        logf!("panic", "{info}\n{bt}");
    }));
    let shared: Shared = Arc::new(Mutex::new(Hit::default()));
    tauri::Builder::default()
        // The streaming recognizer lives in the webview and needs the microphone. Without this hook
        // WebView2 denies the request and dictation silently falls back to the slow Windows engine.
        // The recognizer also needs tauri.conf.json's browser flag that turns off the Ceto speech
        // service: that endpoint answers 403 inside WebView2, which surfaces as a "network" error.
        .on_permission_request(|_webview, kind| match kind {
            tauri::webview::PermissionKind::Microphone => tauri::webview::PermissionResponse::Allow,
            _ => tauri::webview::PermissionResponse::Default,
        })
        .manage(shared.clone())
        .invoke_handler(tauri::generate_handler![
            media::media_poll, media::media_art, media::media_control, media::media_seek, set_hit_rect, set_dragging, log, get_ptr, get_monitors, stt::stt_state, stt::stt_toggle,
            stt::stt_engine, stt::stt_ended, stt::stt_type,
            hotkey::stt_settings, hotkey::set_stt_hold, hotkey::record_shortcut, hotkey::record_submit, hotkey::cancel_record_shortcut, hotkey::reset_shortcut,
autostart::autostart_get, autostart::autostart_set,
            stats::stats, mixer::mixer_get, mixer::mixer_set_vol, mixer::mixer_set_mute
        ])
        .setup(move |app| {
            logf!("win", "setup: initialising the dictation shortcut");
            stt::bind(app.handle().clone());
            hotkey::init();
            autostart::init();
            if let Err(e) = tray::init(app.handle()) { logf!("tray", "could not add the tray icon: {e}"); }
            logf!("win", "setup: starting stats");
            stats::start();

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

            // Window stays fully click-through. Rust reads the mouse and pushes every change to the UI;
            // the old 16 ms JS poll missed fast clicks and kept a 60 Hz timer alive for nothing.
            let hwnd = win.hwnd()?.0 as isize;
            // The overlay never moves after setup, so cache its top-left for cursor math.
            let (ox, oy) = (minx as f64, miny as f64);
            OVERLAY.store(hwnd, Ordering::Relaxed);
            let winc = win.clone();
            let app = app.handle().clone();
            logf!("win", "mouse thread starting, hwnd={hwnd} scale={scale}");
            thread::spawn(move || {
                let mut interactive = false;
                let mut hidden = false;
                let mut fs = false;
                let mut next_fs = Instant::now();
                let mut fs_seen: Option<(bool, bool)> = None;
                let mut next_fs_emit = Instant::now();
                let mut sent: Option<Ptr> = None;
                let mut ticks = 0u32;
                loop {
                    let (dragging, down0) = { let g = shared.lock().unwrap(); (g.dragging, g.p.2) };
                    // 8 ms while the island can be grabbed, 32 ms otherwise, 100 ms while a game owns the mouse
                    let ms = if hidden { 100 } else if interactive || dragging || down0 { 8 } else { 32 };
                    thread::sleep(Duration::from_millis(ms));
                    ticks += 1;
                    let now = Instant::now();
                    if now >= next_fs { fs = fullscreen_app(hwnd); next_fs = now + Duration::from_millis(250); }
                    let cur = cursor_visible();
                    // fullscreen game in front AND its cursor is hidden (mouse-look gameplay): island stays visible
                    // but never takes the mouse. If the cursor is showing (menus), the island stays usable.
                    let sup = fs && !cur;
                    // a windowed/borderless fullscreen app (the cursor is still there) can ask the island to hide
                    let wfs = fs && cur;
                    let changed = fs_seen != Some((fs, wfs));
                    if changed {
                        logf!("ptr", "foreground: {}", if !fs { "no fullscreen app" } else if wfs { "windowed fullscreen app" } else { "exclusive fullscreen app" });
                    }
                    // re-send every couple of seconds so a reloaded webview recovers the state
                    if changed || now >= next_fs_emit {
                        fs_seen = Some((fs, wfs));
                        next_fs_emit = now + Duration::from_secs(2);
                        let _ = app.emit("fs", serde_json::json!({ "full": fs, "windowed": wfs }));
                    }
                    if sup != hidden {
                        hidden = sup;
                        if sup {
                            interactive = false;
                            let _ = winc.set_ignore_cursor_events(true);
                            let mut g = shared.lock().unwrap();
                            g.dragging = false;
                            g.p = (-1e6, -1e6, false, false);
                            drop(g);
                            let p = Ptr { x: -1e6, y: -1e6, down: false, inside: false };
                            let _ = app.emit("ptr", p.clone());
                            sent = Some(p);
                            logf!("ptr", "fullscreen app with a hidden cursor: island input paused");
                        } else {
                            logf!("ptr", "fullscreen app gone: island input resumed");
                        }
                    }
                    if hidden { continue; }
                    let Some((px, py)) = cursor_at(ox, oy) else { continue };
                    let (x, y) = (px / scale, py / scale);
                    let down = lbutton();
                    let (r, dragging) = { let g = shared.lock().unwrap(); (g.r, g.dragging) };
                    let pad = 10.0;
                    let inside = r[2] > 0.0 && x >= r[0] - pad && x <= r[0] + r[2] + pad && y >= r[1] - pad && y <= r[1] + r[3] + pad;
                    // Over the island the window takes the mouse, so clicks never reach what's behind it.
                    let over = r[2] > 0.0 && x >= r[0] - 1.0 && x <= r[0] + r[2] + 1.0 && y >= r[1] - 1.0 && y <= r[1] + r[3] + 1.0;
                    let want = over || dragging;
                    if want != interactive {
                        interactive = want;
                        let _ = winc.set_ignore_cursor_events(!want);
                        logf!("ptr", "click-through {} at ({x:.0},{y:.0})", if want { "off (island takes the mouse)" } else { "on" });
                    }
                    shared.lock().unwrap().p = (x, y, down, inside);
                    // Push press/release/hover edges immediately (a fast click is two ticks of the same
                    // mouse), stream while a button is held inside the island (drag and scrub), and keep a
                    // once-a-second heartbeat so a reloaded webview always recovers the state.
                    let edge = sent.as_ref().map_or(true, |p| p.down != down || p.inside != inside);
                    let moved = sent.as_ref().map_or(true, |p| (p.x - x).abs() >= 0.5 || (p.y - y).abs() >= 0.5);
                    if edge || dragging || (down && inside) || (moved && inside) || ticks % 32 == 0 {
                        let p = Ptr { x, y, down, inside };
                        let _ = app.emit("ptr", p.clone());
                        sent = Some(p);
                    }
                }
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running IslandWin");
}

#[cfg(all(test, windows))]
mod fullscreen_tests {
    use super::*;
    use windows::Win32::Foundation::RECT;

    fn rect(left: i32, top: i32, right: i32, bottom: i32) -> RECT {
        RECT { left, top, right, bottom }
    }

    #[test]
    fn monitor_cover_must_be_complete() {
        let m = rect(0, 0, 1920, 1080);
        assert!(covers_monitor(rect(0, 0, 1920, 1080), m));
        assert!(covers_monitor(rect(-1, -1, 1921, 1081), m));
        assert!(!covers_monitor(rect(0, 0, 1920, 1040), m));
        assert!(!covers_monitor(rect(0, 0, 1280, 720), m));
    }

    #[test]
    fn maximized_framed_window_is_not_fullscreen() {
        let m = rect(0, 0, 1920, 1080);
        // Windows 11 with an auto-hiding taskbar: a maximized normal window reaches every edge but
        // still has its title bar, so it must not hide the island.
        assert!(!fullscreen_rect(true, true, m, m));
        // borderless fullscreen game
        assert!(fullscreen_rect(true, false, m, m));
        assert!(fullscreen_rect(false, false, m, m));
        // ordinary window above the taskbar
        assert!(!fullscreen_rect(false, true, rect(0, 0, 1920, 1040), m));
    }
}
