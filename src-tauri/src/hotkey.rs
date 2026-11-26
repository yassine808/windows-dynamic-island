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
/// A combo handed over by the webview (it sees the keys directly while it holds focus).
static PENDING: Mutex<Option<String>> = Mutex::new(None);

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
    let file = path(app);
    logf!("hotkey", "settings file: {}", file.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "<none>".into()));
    let mut s: Settings = file.and_then(|p| fs::read_to_string(p).ok()).and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    logf!("hotkey", "loaded shortcut={:?} hold={}", s.shortcut, s.hold);
    match register(app, &s.shortcut) {
        Ok(()) => logf!("hotkey", "registered {:?}", s.shortcut),
        Err(e) => {
            logf!("hotkey", "registering {:?} failed ({e}), falling back to {DEFAULT}", s.shortcut);
            s.shortcut = DEFAULT.into();
            if let Err(e) = register(app, DEFAULT) { logf!("hotkey", "fallback registration failed: {e}"); }
        }
    }
    crate::stt::HOLD.store(s.hold, Ordering::Relaxed);
    *CUR.lock().unwrap() = Some(s);
}

#[tauri::command]
pub fn stt_settings() -> Settings { get() }

#[tauri::command]
pub fn set_stt_hold(app: AppHandle, hold: bool) {
    logf!("hotkey", "hold mode -> {hold}");
    crate::stt::HOLD.store(hold, Ordering::Relaxed);
    let mut s = get();
    s.hold = hold;
    save(&app, &s);
}

#[tauri::command]
pub fn cancel_record_shortcut() {
    logf!("hotkey", "shortcut recording cancelled by the UI");
    CANCEL.store(true, Ordering::Relaxed);
}

/// Does this combo have something holding it down? A bare letter would be swallowed everywhere.
fn has_modifier(k: &str) -> bool {
    let parts: Vec<&str> = k.split('+').map(str::trim).collect();
    if parts.len() > 1 { return true; }
    let p = parts.first().copied().unwrap_or("");
    p.len() > 1 && p.starts_with('F') && p[1..].chars().all(|c| c.is_ascii_digit())
}

/// The webview sees keys directly while it holds focus. Taking them here as well means recording
/// works even when the foreground app is elevated and a low-level keyboard hook gets nothing.
#[tauri::command]
pub fn record_submit(key: String) -> Result<(), String> {
    let k = key.trim().to_string();
    if k.is_empty() { return Ok(()); }
    if !has_modifier(&k) {
        logf!("hotkey", "record: {k:?} needs a modifier");
        return Err("Add Ctrl, Alt or Shift (or use an F-key)".into());
    }
    if k.parse::<Shortcut>().is_err() { return Err(format!("{k} is not a shortcut")); }
    logf!("hotkey", "record: webview captured {k}");
    *PENDING.lock().unwrap() = Some(k);
    Ok(())
}

/// Wait for the user to press a key combo anywhere on the system, then make it the dictation shortcut.
#[tauri::command]
pub async fn record_shortcut(app: AppHandle) -> Result<String, String> {
    let old = get();
    CANCEL.store(false, Ordering::Relaxed);
    *PENDING.lock().unwrap() = None;
    // so pressing the current combo doesn't start dictation meanwhile
    if let Ok(sc) = old.shortcut.parse::<Shortcut>() { let _ = app.global_shortcut().unregister(sc); }
    logf!("hotkey", "recording a new shortcut (current {:?})", old.shortcut);
    // own the keyboard for a few seconds: a foreground window of our own is seen by both capture
    // paths, where a hook alone is blind to keys typed into an elevated app
    let focus = imp::grab_focus(&app);
    let got = tauri::async_runtime::spawn_blocking(|| imp::record(&CANCEL, &PENDING)).await.map_err(|e| e.to_string());
    imp::release_focus(focus);
    logf!("hotkey", "record result: {got:?}");
    let res = match got {
        Ok(Ok(k)) => register(&app, &k).map(|_| k),
        Ok(Err(e)) | Err(e) => Err(e),
    };
    match res {
        Ok(k) => { logf!("hotkey", "shortcut saved: {k}"); save(&app, &Settings { shortcut: k.clone(), hold: old.hold }); Ok(k) }
        Err(e) => { logf!("hotkey", "shortcut not changed ({e}), restoring {:?}", old.shortcut); let _ = register(&app, &old.shortcut); Err(e) }
    }
}

#[cfg(windows)]
mod imp {
    use std::{
        sync::{atomic::{AtomicBool, Ordering}, Mutex},
        thread::sleep,
        time::{Duration, Instant},
    };
    use windows::Win32::{
        Foundation::{HWND, LPARAM, LRESULT, WPARAM},
        UI::{
            Input::KeyboardAndMouse::{GetAsyncKeyState, SetFocus},
            WindowsAndMessaging::{
                CallNextHookEx, GetForegroundWindow, GetWindowLongPtrW, PeekMessageW,
                SetForegroundWindow, SetWindowLongPtrW, SetWindowsHookExW, UnhookWindowsHookEx,
                GWL_EXSTYLE, KBDLLHOOKSTRUCT, MSG, PM_REMOVE, WH_KEYBOARD_LL, WM_KEYDOWN,
                WM_SYSKEYDOWN, WS_EX_NOACTIVATE,
            },
        },
    };

    fn down(vk: u16) -> bool { unsafe { (GetAsyncKeyState(vk as i32) as u16) & 0x8000 != 0 } }

    /// dwExtraInfo we stamp on every key event the dictation typer injects, so recording can ignore them.
    const OURS: usize = 0x49534C57; // "ISLW"

    // Windows virtual-key -> W3C key code name (what the shortcut parser expects)
    pub(super) fn name(vk: u16) -> Option<String> {
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

    fn modifier(vk: u16) -> bool {
        matches!(vk, 0x10 | 0x11 | 0x12 | 0x5B | 0x5C | 0x5D | 0xA0..=0xA5)
    }

    #[cfg(test)]
    pub(super) use name as key_name;

    /// First real key pressed while the hook is installed; the hook runs on the recording thread.
    static HIT: Mutex<Option<u16>> = Mutex::new(None);

    unsafe extern "system" fn hook(code: i32, w: WPARAM, l: LPARAM) -> LRESULT {
        if code >= 0 && (w.0 as u32 == WM_KEYDOWN || w.0 as u32 == WM_SYSKEYDOWN) {
            let kb = &*(l.0 as *const KBDLLHOOKSTRUCT);
            let vk = kb.vkCode as u16;
            // skip modifiers (they only qualify the combo) and our own typing (stamped OURS)
            if !modifier(vk) && kb.dwExtraInfo != OURS {
                if let Ok(mut h) = HIT.lock() {
                    if h.is_none() {
                        *h = Some(vk);
                        logf!("hotkey", "record: saw key 0x{vk:02X}");
                    }
                }
            }
        }
        CallNextHookEx(None, code, w, l)
    }

    /// Fallback for machines where no low-level hook callback arrives: a key that is still down a
    /// few ms later is a real press, not one of the typer's back-to-back injected characters.
    fn poll() -> Option<u16> {
        let vk = (0x01u16..=0xFE).find(|&v| name(v).is_some() && down(v))?;
        sleep(Duration::from_millis(24));
        down(vk).then_some(vk)
    }

    fn released(vk: u16) -> bool {
        !down(vk) && !down(0x10) && !down(0x11) && !down(0x12) && !down(0x5B) && !down(0x5C)
    }

    pub fn grab_focus(app: &tauri::AppHandle) -> (isize, isize, isize) {
        let hwnd = crate::tools::OVERLAY.load(Ordering::Relaxed);
        if hwnd == 0 { return (0, 0, 0); }
        // the overlay is normally WS_EX_NOACTIVATE; make it activatable for the recording only,
        // so keystrokes land in our own window instead of the app underneath
        let h = HWND(hwnd as _);
        let (prev, ex) = unsafe {
            let prev = GetForegroundWindow();
            let ex = GetWindowLongPtrW(h, GWL_EXSTYLE);
            SetWindowLongPtrW(h, GWL_EXSTYLE, ex & !(WS_EX_NOACTIVATE.0 as isize));
            let _ = SetForegroundWindow(h);
            let _ = SetFocus(h);
            (prev, ex)
        };
        if let Some(w) = tauri::Manager::get_webview_window(app, "main") { let _ = w.set_focus(); }
        (hwnd, ex, prev.0 as isize)
    }

    pub fn release_focus(f: (isize, isize, isize)) {
        let (hwnd, ex, prev) = f;
        if hwnd == 0 { return; }
        unsafe {
            SetWindowLongPtrW(HWND(hwnd as _), GWL_EXSTYLE, ex);
            if prev != 0 { let _ = SetForegroundWindow(HWND(prev as _)); }
        }
    }

    pub fn record(cancel: &AtomicBool, pending: &Mutex<Option<String>>) -> Result<String, String> {
        *HIT.lock().unwrap() = None;
        let handle = unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook), None, 0) }
            .map_err(|e| { logf!("hotkey", "record: keyboard hook failed: {e}"); format!("keyboard hook failed: {e}") })?;
        logf!("hotkey", "record: hook installed, waiting for a key combo");
        let deadline = Instant::now() + Duration::from_secs(30);
        let mut picked: Option<Result<String, String>> = None;
        while Instant::now() < deadline {
            // a low-level hook is only called while its thread pumps messages
            let mut msg = MSG::default();
            while unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE) }.as_bool() {}
            if cancel.load(Ordering::Relaxed) { logf!("hotkey", "record: cancelled"); break; }
            // the webview hands over a combo it saw itself (works even for elevated foreground apps)
            if let Some(k) = pending.lock().unwrap().take() {
                sleep(Duration::from_millis(200)); // let the keys come up before the shortcut goes live
                picked = Some(Ok(k));
                break;
            }
            let Some(vk) = HIT.lock().unwrap().take().or_else(poll) else {
                sleep(Duration::from_millis(8));
                continue;
            };
            if vk == 0x1B { logf!("hotkey", "record: Escape, cancelled"); picked = Some(Err("cancelled".into())); break; }
            let Some(n) = name(vk) else {
                logf!("hotkey", "record: key 0x{vk:02X} cannot be a shortcut");
                picked = Some(Err(format!("That key (0x{vk:02X}) cannot be a shortcut")));
                break;
            };
            let mut parts: Vec<String> = Vec::new();
            if down(0x11) { parts.push("Ctrl".into()); }
            if down(0x12) { parts.push("Alt".into()); }
            if down(0x10) { parts.push("Shift".into()); }
            if down(0x5B) || down(0x5C) { parts.push("Super".into()); }
            let fkey = n.starts_with('F') && n[1..].chars().all(|c| c.is_ascii_digit());
            if parts.is_empty() && !fkey {
                logf!("hotkey", "record: {n} alone is not enough");
                picked = Some(Err("Add Ctrl, Alt or Shift (or use an F-key)".into()));
                break;
            }
            parts.push(n);
            logf!("hotkey", "record: captured {}", parts.join("+"));
            // let go of the combo before it is registered, so it does not fire the moment it goes live
            let t = Instant::now();
            while !released(vk) && t.elapsed() < Duration::from_secs(3) { sleep(Duration::from_millis(10)); }
            picked = Some(Ok(parts.join("+")));
            break;
        }
        unsafe { let _ = UnhookWindowsHookEx(handle); }
        match picked {
            Some(r) => r,
            None => {
                logf!("hotkey", "record: nothing pressed within 30s");
                Err("Timed out, click the chip and press a key".into())
            }
        }
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::{sync::atomic::AtomicBool, thread::sleep, time::Duration};

    // Every key the recorder can emit must also be one the shortcut parser accepts, otherwise a
    // captured combo would be rejected right after the user pressed it.
    #[test]
    fn every_key_name_is_a_registrable_shortcut() {
        let mut vks: Vec<u16> = (0x41..=0x5A).chain(0x30..=0x39).chain(0x70..=0x87).chain(0x60..=0x69).collect();
        vks.extend([0x20, 0x0D, 0x09, 0x08, 0x25, 0x26, 0x27, 0x28, 0x2D, 0x2E, 0x24, 0x23, 0x21, 0x22]);
        vks.extend(0xBA..=0xDE);
        let mut checked = 0;
        for vk in vks {
            let Some(n) = imp::key_name(vk) else { continue };
            let combo = format!("Ctrl+Alt+{n}");
            assert!(combo.parse::<Shortcut>().is_ok(), "{combo} does not parse");
            checked += 1;
        }
        assert!(checked > 80, "only {checked} names checked");
    }

    #[test]
    fn a_bare_key_is_not_a_shortcut() {
        assert!(!has_modifier("KeyK"));
        assert!(has_modifier("Ctrl+KeyK"));
        assert!(has_modifier("F13"));
        assert!(has_modifier("Shift+Digit1"));
        assert!(record_submit("KeyK".into()).is_err());
    }

    // The recorder must see keys injected by other programs too (keyboard remappers, tools), so a
    // synthetic combo is a faithful test. Nothing is typed and the combo is inert in every app.
    // Skipped by default: a CI runner has no interactive desktop to receive synthetic input.
    #[test]
    fn captures_a_synthetic_combo() {
        if std::env::var("ISLANDWIN_E2E").is_err() { return; } // local: ISLANDWIN_E2E=1 cargo test
        let pending = Mutex::new(None);
        let t = std::thread::spawn(move || imp::record(&AtomicBool::new(false), &pending));
        sleep(Duration::from_millis(300));
        unsafe {
            use windows::Win32::UI::Input::KeyboardAndMouse::{
                SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, VIRTUAL_KEY,
            };
            let ev = |vk: u16, up: bool| INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 { ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(vk),
                    wScan: 0,
                    dwFlags: KEYBD_EVENT_FLAGS(if up { 2 } else { 0 }),
                    time: 0,
                    dwExtraInfo: 0,
                } },
            };
            for (vk, up) in [(0x11, false), (0x12, false), (0x7C, false), (0x7C, true), (0x12, true), (0x11, true)] {
                let one = [ev(vk, up)];
                SendInput(&one, std::mem::size_of::<INPUT>() as i32);
                sleep(Duration::from_millis(20));
            }
        }
        let got = t.join().unwrap();
        assert_eq!(got.as_deref(), Ok("Ctrl+Alt+F13"), "recorder captured the wrong combo");
    }
}

#[cfg(not(windows))]
mod imp {
    use std::sync::{atomic::AtomicBool, Mutex};
    pub fn grab_focus(_: &tauri::AppHandle) -> (isize, isize, isize) { (0, 0, 0) }
    pub fn release_focus(_: (isize, isize, isize)) {}
    pub fn record(_: &AtomicBool, _: &Mutex<Option<String>>) -> Result<String, String> { Err("Needs Windows".into()) }
}
