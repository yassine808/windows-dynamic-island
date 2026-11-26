// Dictation shortcut: saved to disk, re-recordable from the UI, Hold (push-to-talk) or Toggle.
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::PathBuf,
    sync::{atomic::{AtomicBool, Ordering}, Mutex},
};
use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};

const DEFAULT: &str = "Ctrl+Alt+Space";

#[derive(Serialize, Deserialize, Clone)]
pub struct Settings { pub shortcut: String, pub hold: bool }
impl Default for Settings { fn default() -> Self { Settings { shortcut: DEFAULT.into(), hold: false } } }

static CUR: Mutex<Option<Settings>> = Mutex::new(None);
static CANCEL: AtomicBool = AtomicBool::new(false);

fn get() -> Settings { CUR.lock().unwrap().clone().unwrap_or_default() }

fn path(app: &AppHandle) -> Option<PathBuf> { app.path().app_config_dir().ok().map(|d| d.join("settings.json")) }

fn save(app: &AppHandle, s: &Settings) {
    *CUR.lock().unwrap() = Some(s.clone());
    if let Some(p) = path(app) {
        if let Some(d) = p.parent() { let _ = fs::create_dir_all(d); }
        let _ = fs::write(p, serde_json::to_string_pretty(s).unwrap_or_default());
    }
}

fn register(app: &AppHandle, key: &str) -> Result<(), String> {
    let sc: Shortcut = key.parse().map_err(|e| format!("{e}"))?;
    app.global_shortcut().register(sc).map_err(|e| format!("{e}"))
}

/// Load saved settings and register the shortcut. Falls back to the default if the saved one is gone or taken.
pub fn init(app: &AppHandle) {
    let mut s: Settings = path(app).and_then(|p| fs::read_to_string(p).ok()).and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    if register(app, &s.shortcut).is_err() {
        s.shortcut = DEFAULT.into();
        let _ = register(app, DEFAULT);
    }
    crate::stt::HOLD.store(s.hold, Ordering::Relaxed);
    *CUR.lock().unwrap() = Some(s);
}

#[tauri::command]
pub fn stt_settings() -> Settings { get() }

#[tauri::command]
pub fn set_stt_hold(app: AppHandle, hold: bool) {
    crate::stt::HOLD.store(hold, Ordering::Relaxed);
    let mut s = get();
    s.hold = hold;
    save(&app, &s);
}

#[tauri::command]
pub fn cancel_record_shortcut() { CANCEL.store(true, Ordering::Relaxed); }

/// Wait for the user to press a key combo anywhere on the system, then make it the dictation shortcut.
#[tauri::command]
pub async fn record_shortcut(app: AppHandle) -> Result<String, String> {
    let old = get();
    CANCEL.store(false, Ordering::Relaxed);
    // so pressing the current combo doesn't start dictation meanwhile
    if let Ok(sc) = old.shortcut.parse::<Shortcut>() { let _ = app.global_shortcut().unregister(sc); }
    println!("[hotkey] recording a new shortcut");
    let got = tauri::async_runtime::spawn_blocking(|| imp::record(&CANCEL)).await.map_err(|e| e.to_string());
    println!("[hotkey] record result: {got:?}");
    let res = match got {
        Ok(Ok(k)) => register(&app, &k).map(|_| k),
        Ok(Err(e)) | Err(e) => Err(e),
    };
    match res {
        Ok(k) => { save(&app, &Settings { shortcut: k.clone(), hold: old.hold }); Ok(k) }
        Err(e) => { let _ = register(&app, &old.shortcut); Err(e) }
    }
}

#[cfg(windows)]
mod imp {
    use std::{sync::atomic::{AtomicBool, Ordering}, thread::sleep, time::{Duration, Instant}};
    use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;

    fn down(vk: u16) -> bool { unsafe { (GetAsyncKeyState(vk as i32) as u16) & 0x8000 != 0 } }

    // Windows virtual-key -> W3C key code name (what the shortcut parser expects)
    fn name(vk: u16) -> Option<String> {
        Some(match vk {
            0x41..=0x5A => format!("Key{}", vk as u8 as char),
            0x30..=0x39 => format!("Digit{}", vk as u8 as char),
            0x70..=0x87 => format!("F{}", vk - 0x6F),
            0x60..=0x69 => format!("Numpad{}", vk - 0x60),
            0x20 => "Space".into(), 0x0D => "Enter".into(), 0x09 => "Tab".into(), 0x08 => "Backspace".into(),
            0x25 => "ArrowLeft".into(), 0x26 => "ArrowUp".into(), 0x27 => "ArrowRight".into(), 0x28 => "ArrowDown".into(),
            0x2D => "Insert".into(), 0x2E => "Delete".into(), 0x24 => "Home".into(), 0x23 => "End".into(),
            0x21 => "PageUp".into(), 0x22 => "PageDown".into(),
            0xBA => "Semicolon".into(), 0xBB => "Equal".into(), 0xBC => "Comma".into(), 0xBD => "Minus".into(),
            0xBE => "Period".into(), 0xBF => "Slash".into(), 0xC0 => "Backquote".into(), 0xDB => "BracketLeft".into(),
            0xDC => "Backslash".into(), 0xDD => "BracketRight".into(), 0xDE => "Quote".into(),
            _ => return None,
        })
    }

    pub fn record(cancel: &AtomicBool) -> Result<String, String> {
        let any = || (0x08u16..=0xDE).any(|v| name(v).is_some() && down(v));
        // let go of anything already held (e.g. the key that was pressed just before)
        let t = Instant::now();
        while any() && t.elapsed() < Duration::from_secs(2) { sleep(Duration::from_millis(15)); }
        let t = Instant::now();
        while t.elapsed() < Duration::from_secs(15) {
            if cancel.load(Ordering::Relaxed) { return Err("cancelled".into()); }
            sleep(Duration::from_millis(8));
            let Some(vk) = (0x08u16..=0xDE).find(|&v| name(v).is_some() && down(v)) else { continue };
            let mut parts: Vec<String> = Vec::new();
            if down(0x11) { parts.push("Ctrl".into()); }
            if down(0x12) { parts.push("Alt".into()); }
            if down(0x10) { parts.push("Shift".into()); }
            if down(0x5B) || down(0x5C) { parts.push("Super".into()); }
            let n = name(vk).unwrap();
            let fkey = n.starts_with('F') && n[1..].chars().all(|c| c.is_ascii_digit());
            if parts.is_empty() && !fkey { return Err("Add Ctrl, Alt or Shift (or use an F-key)".into()); }
            parts.push(n);
            // wait for release so the keys don't leak into the app underneath
            let t = Instant::now();
            while any() && t.elapsed() < Duration::from_secs(3) { sleep(Duration::from_millis(15)); }
            return Ok(parts.join("+"));
        }
        Err("cancelled".into())
    }
}

#[cfg(not(windows))]
mod imp {
    use std::sync::atomic::AtomicBool;
    pub fn record(_: &AtomicBool) -> Result<String, String> { Err("Needs Windows".into()) }
}
