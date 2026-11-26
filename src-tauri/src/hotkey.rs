// Dictation shortcut: saved to disk, re-recordable from the UI, Hold (push-to-talk) or Toggle.
// A small system-wide input hook makes every key and every mouse button usable, with or without
// modifiers. The left mouse button is never a shortcut.
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::PathBuf,
    sync::{atomic::{AtomicBool, Ordering}, Mutex, OnceLock},
};
use tauri::AppHandle;

const DEFAULT: &str = "Ctrl+Alt+Space";

#[derive(Serialize, Deserialize, Clone)]
pub struct Settings { pub shortcut: String, pub hold: bool }
impl Default for Settings { fn default() -> Self { Settings { shortcut: DEFAULT.into(), hold: false } } }

static CUR: Mutex<Option<Settings>> = Mutex::new(None);
static CANCEL: AtomicBool = AtomicBool::new(false);
/// Serialises "save the recording" with "reset to the default": whichever lands last is the intent.
static COMMIT: Mutex<()> = Mutex::new(());
/// A combo handed over by the webview (it sees the keys directly while it holds focus).
static PENDING: Mutex<Option<String>> = Mutex::new(None);
/// True while the recorder owns the input; the global trigger must stay quiet then.
static RECORDING: AtomicBool = AtomicBool::new(false);
static STARTED: OnceLock<()> = OnceLock::new();

fn get() -> Settings { CUR.lock().unwrap().clone().unwrap_or_default() }

fn path() -> PathBuf { crate::paths::data_dir().join("settings.json") }

fn save(s: &Settings) {
    *CUR.lock().unwrap() = Some(s.clone());
    let p = path();
    if let Some(d) = p.parent() { let _ = fs::create_dir_all(d); }
    let _ = fs::write(&p, serde_json::to_string_pretty(s).unwrap_or_default());
}

/// One thing the user can press: a keyboard virtual-key or a mouse button. Mouse 0x01 (left) is
/// deliberately not representable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Keyboard(u16),
    Mouse(u16),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Combo {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub sup: bool,
    pub key: Key,
}

impl Combo {
    /// Accepts `Ctrl+Alt+Space`, `KeyK`, `Mouse4`, `F13`, `VkE8`, and any other key name the
    /// recorder can produce. A modifier alone is a qualifier, not a shortcut.
    pub fn parse(s: &str) -> Result<Self, String> {
        let (mut ctrl, mut alt, mut shift, mut sup) = (false, false, false, false);
        let mut key: Option<Key> = None;
        for raw in s.split('+').map(str::trim).filter(|p| !p.is_empty()) {
            let lower = raw.to_ascii_lowercase();
            let mouse = match lower.as_str() {
                "mouseleft" | "leftclick" | "left" => return Err("Left click cannot be a shortcut".into()),
                "mouseright" | "rightclick" | "right" => Some(0x02),
                "mousemiddle" | "middleclick" | "middle" => Some(0x04),
                "mouse4" | "xbutton1" => Some(0x05),
                "mouse5" | "xbutton2" => Some(0x06),
                _ => None,
            };
            if let Some(btn) = mouse {
                if key.is_some() { return Err(format!("{s} has more than one key")); }
                key = Some(Key::Mouse(btn));
                continue;
            }
            match lower.as_str() {
                "ctrl" | "control" => ctrl = true,
                "alt" => alt = true,
                "shift" => shift = true,
                "super" | "win" | "meta" => sup = true,
                _ => {
                    if key.is_some() { return Err(format!("{s} has more than one key")); }
                    let vk = imp::vk_of_name(raw).ok_or_else(|| format!("{raw} is not a key"))?;
                    key = Some(Key::Keyboard(vk));
                }
            }
        }
        let key = key.ok_or_else(|| "Press a key, not only a modifier".to_string())?;
        Ok(Combo { ctrl, alt, shift, sup, key })
    }

    /// Canonical stored form: modifiers first, then the key name the recorder uses.
    pub fn label(self) -> String {
        let mut parts: Vec<&str> = Vec::new();
        if self.ctrl { parts.push("Ctrl"); }
        if self.alt { parts.push("Alt"); }
        if self.shift { parts.push("Shift"); }
        if self.sup { parts.push("Super"); }
        let name = match self.key {
            Key::Keyboard(vk) => imp::name(vk).unwrap_or_else(|| format!("Vk{vk:02X}")),
            Key::Mouse(0x02) => "MouseRight".into(),
            Key::Mouse(0x04) => "MouseMiddle".into(),
            Key::Mouse(0x05) => "Mouse4".into(),
            Key::Mouse(0x06) => "Mouse5".into(),
            Key::Mouse(_) => "Mouse".into(),
        };
        let mut out = parts.join("+");
        if !out.is_empty() { out.push('+'); }
        out.push_str(&name);
        out
    }
}

/// Load saved settings and start the global input hook. Any unreadable/old combo falls back to the
/// default instead of leaving the app with a shortcut that can never fire.
pub fn init() {
    let file = path();
    logf!("hotkey", "settings file: {}", file.display());
    let mut s: Settings = fs::read_to_string(&file).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    if let Err(e) = Combo::parse(&s.shortcut) {
        logf!("hotkey", "saved shortcut {:?} is not usable ({e}), falling back to {DEFAULT}", s.shortcut);
        s.shortcut = DEFAULT.into();
        save(&s);
    }
    logf!("hotkey", "loaded shortcut={:?} hold={}", s.shortcut, s.hold);
    crate::stt::HOLD.store(s.hold, Ordering::Relaxed);
    *CUR.lock().unwrap() = Some(s);
    if STARTED.set(()).is_ok() { imp::start(); }
}

#[tauri::command]
pub fn stt_settings() -> Settings { get() }

#[tauri::command]
pub fn set_stt_hold(hold: bool) {
    logf!("hotkey", "hold mode -> {hold}");
    crate::stt::HOLD.store(hold, Ordering::Relaxed);
    let mut s = get();
    s.hold = hold;
    save(&s);
}

#[tauri::command]
pub fn cancel_record_shortcut() {
    logf!("hotkey", "shortcut recording cancelled by the UI");
    CANCEL.store(true, Ordering::Relaxed);
}

/// The webview sees keys directly while it holds focus. Taking them here as well means recording
/// works even when the foreground app is elevated and a low-level hook gets nothing.
#[tauri::command]
pub fn record_submit(key: String) -> Result<(), String> {
    let k = key.trim();
    if k.is_empty() { return Ok(()); }
    let combo = Combo::parse(k)?;
    let k = combo.label();
    logf!("hotkey", "record: webview captured {k}");
    *PENDING.lock().unwrap() = Some(k);
    Ok(())
}

/// Put the factory shortcut back. Safe to call while the recorder is armed: it cancels the capture
/// first so the two commands can never race over the saved file.
#[tauri::command]
pub fn reset_shortcut() -> Result<String, String> {
    CANCEL.store(true, Ordering::Relaxed);
    let _commit = COMMIT.lock().unwrap();
    let mut s = get();
    s.shortcut = DEFAULT.into();
    save(&s);
    logf!("hotkey", "shortcut reset to {DEFAULT}");
    Ok(DEFAULT.into())
}

/// Wait for any key or mouse button anywhere on the system, then make it the dictation shortcut.
#[tauri::command]
pub async fn record_shortcut(app: AppHandle) -> Result<String, String> {
    if RECORDING.swap(true, Ordering::Relaxed) {
        return Err("Already recording a shortcut".into());
    }
    let old = get();
    CANCEL.store(false, Ordering::Relaxed);
    *PENDING.lock().unwrap() = None;
    logf!("hotkey", "recording a new shortcut (current {:?})", old.shortcut);
    // own the keyboard for a few seconds: a foreground window of our own is seen by both capture
    // paths, where a hook alone is blind to keys typed into an elevated app
    let focus = imp::grab_focus(&app);
    let got = tauri::async_runtime::spawn_blocking(|| imp::record(&CANCEL, &PENDING)).await.map_err(|e| e.to_string());
    imp::release_focus(focus);
    RECORDING.store(false, Ordering::Relaxed);
    logf!("hotkey", "record result: {got:?}");
    match got {
        Ok(Ok(k)) => {
            let combo = Combo::parse(&k).map_err(|e| format!("{k}: {e}"))?;
            let k = combo.label();
            let _commit = COMMIT.lock().unwrap();
            // a reset clicked while the key was still coming up must win over the capture
            if CANCEL.load(Ordering::Relaxed) {
                logf!("hotkey", "record: cancelled before saving {k}");
                return Err("cancelled".into());
            }
            save(&Settings { shortcut: k.clone(), hold: old.hold });
            logf!("hotkey", "shortcut saved: {k}");
            Ok(k)
        }
        Ok(Err(e)) | Err(e) => {
            logf!("hotkey", "shortcut not changed ({e}), keeping {:?}", old.shortcut);
            Err(e)
        }
    }
}

#[cfg(windows)]
mod imp {
    use super::{Combo, Key, RECORDING};
    use std::{
        sync::{
            atomic::{AtomicU32, Ordering},
            mpsc::{channel, Sender},
            Mutex, OnceLock,
        },
        thread::sleep,
        time::{Duration, Instant},
    };
    use windows::Win32::{
        Foundation::{HWND, LPARAM, LRESULT, WPARAM},
        UI::{
            Input::KeyboardAndMouse::{GetAsyncKeyState, SetFocus},
            WindowsAndMessaging::{
                CallNextHookEx, DispatchMessageW, GetForegroundWindow, GetMessageW, GetWindowLongPtrW,
                PeekMessageW, SetForegroundWindow, SetWindowLongPtrW, SetWindowsHookExW,
                TranslateMessage, UnhookWindowsHookEx, GWL_EXSTYLE, KBDLLHOOKSTRUCT, LLKHF_UP,
                MSG, MSLLHOOKSTRUCT, PM_REMOVE, WH_KEYBOARD_LL, WH_MOUSE_LL, WM_KEYDOWN,
                WM_MBUTTONDOWN, WM_MBUTTONUP, WM_RBUTTONDOWN, WM_RBUTTONUP, WM_SYSKEYDOWN,
                WM_XBUTTONDOWN, WM_XBUTTONUP, WS_EX_NOACTIVATE,
            },
        },
    };

    /// dwExtraInfo we stamp on every event the dictation typer injects, so hooks ignore them.
    const OURS: usize = 0x49534C57; // "ISLW"

    fn down(vk: u16) -> bool { unsafe { (GetAsyncKeyState(vk as i32) as u16) & 0x8000 != 0 } }

    pub(super) fn name(vk: u16) -> Option<String> {
        if modifier(vk) { return None; }
        Some(match vk {
            0x08 => "Backspace".into(), 0x09 => "Tab".into(), 0x0C => "Clear".into(), 0x0D => "Enter".into(),
            0x13 => "Pause".into(), 0x14 => "CapsLock".into(), 0x1B => "Escape".into(), 0x20 => "Space".into(),
            0x21 => "PageUp".into(), 0x22 => "PageDown".into(), 0x23 => "End".into(), 0x24 => "Home".into(),
            0x25 => "ArrowLeft".into(), 0x26 => "ArrowUp".into(), 0x27 => "ArrowRight".into(), 0x28 => "ArrowDown".into(),
            0x2C => "PrintScreen".into(), 0x2D => "Insert".into(), 0x2E => "Delete".into(), 0x2F => "Help".into(),
            0x30..=0x39 => format!("Digit{}", vk as u8 as char),
            0x41..=0x5A => format!("Key{}", vk as u8 as char),
            0x5D => "ContextMenu".into(),
            0x60..=0x69 => format!("Numpad{}", vk - 0x60),
            0x6A => "NumpadMultiply".into(), 0x6B => "NumpadAdd".into(), 0x6C => "NumpadSeparator".into(),
            0x6D => "NumpadSubtract".into(), 0x6E => "NumpadDecimal".into(), 0x6F => "NumpadDivide".into(),
            0x70..=0x87 => format!("F{}", vk - 0x6F),
            0x90 => "NumLock".into(), 0x91 => "ScrollLock".into(),
            0xA6 => "BrowserBack".into(), 0xA7 => "BrowserForward".into(), 0xA8 => "BrowserRefresh".into(),
            0xA9 => "BrowserStop".into(), 0xAA => "BrowserSearch".into(), 0xAB => "BrowserFavorites".into(),
            0xAC => "BrowserHome".into(), 0xAD => "VolumeMute".into(), 0xAE => "VolumeDown".into(), 0xAF => "VolumeUp".into(),
            0xB0 => "MediaNextTrack".into(), 0xB1 => "MediaPrevTrack".into(), 0xB2 => "MediaStop".into(),
            0xB3 => "MediaPlayPause".into(), 0xB4 => "LaunchMail".into(), 0xB5 => "LaunchMediaSelect".into(),
            0xB6 => "LaunchApp1".into(), 0xB7 => "LaunchApp2".into(),
            0xBA => "Semicolon".into(), 0xBB => "Equal".into(), 0xBC => "Comma".into(), 0xBD => "Minus".into(),
            0xBE => "Period".into(), 0xBF => "Slash".into(), 0xC0 => "Backquote".into(), 0xDB => "BracketLeft".into(),
            0xDC => "Backslash".into(), 0xDD => "BracketRight".into(), 0xDE => "Quote".into(),
            0xE2 => "IntlBackslash".into(),
            // Every remaining virtual key still gets a stable name, so "any key" really means any key.
            _ => format!("Vk{vk:02X}"),
        })
    }

    pub(super) fn vk_of_name(raw: &str) -> Option<u16> {
        let low = raw.to_ascii_lowercase();
        if let Some(hex) = low.strip_prefix("vk") {
            if let Ok(vk) = u16::from_str_radix(hex, 16) {
                if (0x08..=0xFE).contains(&vk) && !modifier(vk) { return Some(vk); }
            }
        }
        (0x08u16..=0xFE).find(|&vk| name(vk).is_some_and(|n| n.eq_ignore_ascii_case(raw)))
    }

    fn modifier(vk: u16) -> bool {
        matches!(vk, 0x10 | 0x11 | 0x12 | 0x5B | 0x5C | 0xA0..=0xA5)
    }

    #[cfg(test)]
    pub(super) use name as key_name;

    fn mods() -> (bool, bool, bool, bool) {
        (down(0x11) || down(0xA2) || down(0xA3), down(0x12), down(0x10), down(0x5B) || down(0x5C))
    }

    fn mods_match(c: &Combo) -> bool {
        let (ctrl, alt, shift, sup) = mods();
        c.ctrl == ctrl && c.alt == alt && c.shift == shift && c.sup == sup
    }

    fn combo_now(ctrl: bool, alt: bool, shift: bool, sup: bool, key: Key) -> Combo {
        Combo { ctrl, alt, shift, sup, key }
    }

    fn combo_for(vk: u16) -> Option<String> {
        name(vk)?;
        let (ctrl, alt, shift, sup) = mods();
        Some(combo_now(ctrl, alt, shift, sup, Key::Keyboard(vk)).label())
    }

    fn combo_for_mouse(btn: u16) -> String {
        let (ctrl, alt, shift, sup) = mods();
        combo_now(ctrl, alt, shift, sup, Key::Mouse(btn)).label()
    }

    /// First real key or mouse button pressed while the recorder is armed.
    static HIT: Mutex<Option<String>> = Mutex::new(None);

    unsafe extern "system" fn record_keyboard(code: i32, w: WPARAM, l: LPARAM) -> LRESULT {
        if code >= 0 && (w.0 as u32 == WM_KEYDOWN || w.0 as u32 == WM_SYSKEYDOWN) {
            let kb = &*(l.0 as *const KBDLLHOOKSTRUCT);
            let vk = kb.vkCode as u16;
            if !modifier(vk) && kb.dwExtraInfo != OURS {
                if let Ok(mut h) = HIT.lock() {
                    if h.is_none() {
                        *h = combo_for(vk);
                        logf!("hotkey", "record: saw key 0x{vk:02X}");
                    }
                }
            }
        }
        CallNextHookEx(None, code, w, l)
    }

    unsafe extern "system" fn record_mouse(code: i32, w: WPARAM, l: LPARAM) -> LRESULT {
        if code >= 0 {
            let msg = w.0 as u32;
            if matches!(msg, WM_RBUTTONDOWN | WM_MBUTTONDOWN | WM_XBUTTONDOWN) {
                let ms = &*(l.0 as *const MSLLHOOKSTRUCT);
                if ms.dwExtraInfo != OURS {
                    let btn = match msg {
                        WM_RBUTTONDOWN => 0x02,
                        WM_MBUTTONDOWN => 0x04,
                        _ => if (ms.mouseData >> 16) as u16 == 1 { 0x05 } else { 0x06 },
                    };
                    if let Ok(mut h) = HIT.lock() {
                        if h.is_none() {
                            *h = Some(combo_for_mouse(btn));
                            logf!("hotkey", "record: saw mouse button 0x{btn:02X}");
                        }
                    }
                }
            }
        }
        CallNextHookEx(None, code, w, l)
    }

    /// Fallback for machines where no low-level hook callback arrives: a key that is still down a
    /// few ms later is a real press, not one of the typer's back-to-back injected characters.
    fn poll() -> Option<u16> {
        let vk = (0x08u16..=0xFE).find(|&v| name(v).is_some() && down(v))?;
        sleep(Duration::from_millis(24));
        down(vk).then_some(vk)
    }

    fn released(key: Key) -> bool {
        let key_up = match key {
            Key::Keyboard(vk) => !down(vk),
            Key::Mouse(btn) => !down(btn),
        };
        key_up && !down(0x10) && !down(0x11) && !down(0x12) && !down(0x5B) && !down(0x5C)
    }

    pub fn grab_focus(app: &tauri::AppHandle) -> (isize, isize, isize) {
        let hwnd = crate::OVERLAY.load(Ordering::Relaxed);
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

    pub fn record(cancel: &std::sync::atomic::AtomicBool, pending: &Mutex<Option<String>>) -> Result<String, String> {
        *HIT.lock().unwrap() = None;
        let kb = unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(record_keyboard), None, 0) }
            .map_err(|e| { logf!("hotkey", "record: keyboard hook failed: {e}"); format!("keyboard hook failed: {e}") })?;
        let ms = unsafe { SetWindowsHookExW(WH_MOUSE_LL, Some(record_mouse), None, 0) }.ok();
        if ms.is_none() { logf!("hotkey", "record: mouse hook failed, keys still work"); }
        logf!("hotkey", "record: hooks installed, waiting for a key or mouse button");
        let deadline = Instant::now() + Duration::from_secs(30);
        let mut picked: Option<Result<String, String>> = None;
        let mut cancelled = false;
        while Instant::now() < deadline {
            // a low-level hook is only called while its thread pumps messages
            let mut msg = MSG::default();
            while unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE) }.as_bool() {}
            if cancel.load(Ordering::Relaxed) {
                logf!("hotkey", "record: cancelled");
                cancelled = true;
                break;
            }
            // the webview hands over a combo it saw itself (works even for elevated foreground apps)
            if let Some(k) = pending.lock().unwrap().take() {
                if let Ok(combo) = Combo::parse(&k) {
                    let k = combo.label();
                    // let the user let go before the shortcut goes live, or the key's auto-repeat
                    // would fire dictation the moment recording stops
                    let t = Instant::now();
                    while !released(combo.key) && t.elapsed() < Duration::from_secs(3) {
                        sleep(Duration::from_millis(10));
                    }
                    sleep(Duration::from_millis(120));
                    picked = Some(Ok(k));
                    break;
                }
            }
            let hit = HIT.lock().unwrap().take();
            let Some(k) = hit.or_else(|| poll().and_then(combo_for)) else {
                sleep(Duration::from_millis(8));
                continue;
            };
            let Ok(combo) = Combo::parse(&k) else {
                logf!("hotkey", "record: {k:?} is not usable");
                picked = Some(Err("That input is not usable as a shortcut".into()));
                break;
            };
            logf!("hotkey", "record: captured {}", combo.label());
            // let go of the combo before it goes live, so it does not fire the moment recording stops
            let t = Instant::now();
            while !released(combo.key) && t.elapsed() < Duration::from_secs(3) { sleep(Duration::from_millis(10)); }
            picked = Some(Ok(combo.label()));
            break;
        }
        if let Some(h) = ms { unsafe { let _ = UnhookWindowsHookEx(h); } }
        unsafe { let _ = UnhookWindowsHookEx(kb); }
        match picked {
            Some(r) => r,
            None if cancelled => Err("cancelled".into()),
            None => {
                logf!("hotkey", "record: nothing pressed within 30s");
                Err("Timed out, click the chip and press a key or mouse button".into())
            }
        }
    }

    // ---- global trigger ---------------------------------------------------------------------

    #[derive(Clone, Copy, Debug)]
    enum Event { Down(Key), Up(Key) }

    static EVENTS: OnceLock<Sender<Event>> = OnceLock::new();
    /// Keyboard/mouse input currently swallowed because it matched the shortcut.
    static SWALLOW_KEY: AtomicU32 = AtomicU32::new(0);
    static SWALLOW_MOUSE: AtomicU32 = AtomicU32::new(0);

    fn current() -> Option<Combo> { Combo::parse(&super::get().shortcut).ok() }

    fn should_swallow(key: Key) -> bool {
        if RECORDING.load(Ordering::Relaxed) { return false; }
        current().is_some_and(|c| c.key == key && mods_match(&c))
    }

    fn swallow(key: Key, on: bool) -> bool {
        let slot = match key { Key::Keyboard(_) => &SWALLOW_KEY, Key::Mouse(_) => &SWALLOW_MOUSE };
        let code = match key { Key::Keyboard(vk) | Key::Mouse(vk) => vk as u32 };
        if on {
            if should_swallow(key) { slot.store(code, Ordering::Relaxed); return true; }
            false
        } else {
            slot.swap(0, Ordering::Relaxed) == code
        }
    }

    unsafe extern "system" fn trigger_keyboard(code: i32, w: WPARAM, l: LPARAM) -> LRESULT {
        if code >= 0 {
            let kb = &*(l.0 as *const KBDLLHOOKSTRUCT);
            let vk = kb.vkCode as u16;
            if !modifier(vk) && kb.dwExtraInfo != OURS {
                let is_down = (kb.flags.0 & LLKHF_UP.0) == 0;
                let key = Key::Keyboard(vk);
                let matched = if is_down {
                    swallow(key, true)
                } else {
                    swallow(key, false)
                };
                if matched {
                    if let Some(tx) = EVENTS.get() { let _ = tx.send(if is_down { Event::Down(key) } else { Event::Up(key) }); }
                    return LRESULT(1);
                }
            }
        }
        CallNextHookEx(None, code, w, l)
    }

    unsafe extern "system" fn trigger_mouse(code: i32, w: WPARAM, l: LPARAM) -> LRESULT {
        if code >= 0 {
            let msg = w.0 as u32;
            let btn = match msg {
                WM_RBUTTONDOWN | WM_RBUTTONUP => 0x02,
                WM_MBUTTONDOWN | WM_MBUTTONUP => 0x04,
                WM_XBUTTONDOWN | WM_XBUTTONUP => {
                    let ms = &*(l.0 as *const MSLLHOOKSTRUCT);
                    if (ms.mouseData >> 16) as u16 == 1 { 0x05 } else { 0x06 }
                }
                _ => 0,
            };
            if btn != 0 {
                let is_down = matches!(msg, WM_RBUTTONDOWN | WM_MBUTTONDOWN | WM_XBUTTONDOWN);
                let key = Key::Mouse(btn);
                let matched = if is_down { swallow(key, true) } else { swallow(key, false) };
                if matched {
                    if let Some(tx) = EVENTS.get() { let _ = tx.send(if is_down { Event::Down(key) } else { Event::Up(key) }); }
                    return LRESULT(1);
                }
            }
        }
        CallNextHookEx(None, code, w, l)
    }

    fn listener(rx: std::sync::mpsc::Receiver<Event>) {
        let mut held: Option<Key> = None;
        let mut started_hold = false;
        for ev in rx {
            if RECORDING.load(Ordering::Relaxed) { held = None; started_hold = false; continue; }
            let Some(combo) = current() else { held = None; started_hold = false; continue; };
            if let Some(h) = held {
                if h != combo.key {
                    if started_hold { crate::stt::stop(); }
                    held = None;
                    started_hold = false;
                }
            }
            match ev {
                Event::Down(key) if key == combo.key => {
                    if held == Some(key) { continue; } // auto-repeat
                    held = Some(key);
                    started_hold = crate::stt::hold();
                    logf!("hotkey", "shortcut pressed (hold={})", started_hold);
                    if started_hold { crate::stt::start() } else { crate::stt::toggle() }
                }
                Event::Up(key) if held == Some(key) => {
                    held = None;
                    if started_hold {
                        logf!("hotkey", "shortcut released (hold) -> stop");
                        crate::stt::stop();
                    }
                    started_hold = false;
                }
                _ => {}
            }
        }
    }

    pub fn start() {
        std::thread::spawn(|| {
            let (tx, rx) = channel();
            if EVENTS.set(tx).is_err() { return; }
            let kb = unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(trigger_keyboard), None, 0) };
            let ms = unsafe { SetWindowsHookExW(WH_MOUSE_LL, Some(trigger_mouse), None, 0) };
            match (&kb, &ms) {
                (Err(e), _) => logf!("hotkey", "trigger: keyboard hook failed: {e}"),
                (_, Err(e)) => logf!("hotkey", "trigger: mouse hook failed: {e}"),
                _ => logf!("hotkey", "trigger: hooks installed"),
            }
            if kb.is_err() && ms.is_err() { return; }
            std::thread::spawn(move || listener(rx));
            // low-level hooks only fire while this thread pumps messages
            let mut msg = MSG::default();
            while unsafe { GetMessageW(&mut msg, None, 0, 0) }.as_bool() {
                unsafe { let _ = TranslateMessage(&msg); DispatchMessageW(&msg); }
            }
            if let Ok(h) = kb { unsafe { let _ = UnhookWindowsHookEx(h); } }
            if let Ok(h) = ms { unsafe { let _ = UnhookWindowsHookEx(h); } }
        });
    }
}

#[cfg(not(windows))]
mod imp {
    use std::sync::{atomic::AtomicBool, Mutex};
    pub(super) fn name(_: u16) -> Option<String> { None }
    pub(super) fn vk_of_name(_: &str) -> Option<u16> { None }
    pub fn grab_focus(_: &tauri::AppHandle) -> (isize, isize, isize) { (0, 0, 0) }
    pub fn release_focus(_: (isize, isize, isize)) {}
    pub fn record(_: &AtomicBool, _: &Mutex<Option<String>>) -> Result<String, String> { Err("Needs Windows".into()) }
    pub fn start() {}
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::{sync::atomic::AtomicBool, thread::sleep, time::Duration};

    // Every key the recorder can emit must parse back to the same virtual key, otherwise a captured
    // combo would be rejected right after the user pressed it.
    #[test]
    fn every_key_name_round_trips() {
        let mut checked = 0;
        for vk in 0x08u16..=0xFE {
            let Some(n) = imp::key_name(vk) else { continue };
            let combo = Combo::parse(&format!("Ctrl+Alt+{n}")).unwrap_or_else(|e| panic!("{n}: {e}"));
            assert_eq!(combo.key, Key::Keyboard(vk), "{n} parsed to the wrong key");
            checked += 1;
        }
        assert!(checked > 180, "only {checked} names checked");
    }

    #[test]
    fn bare_keys_and_mouse_buttons_are_allowed() {
        assert_eq!(Combo::parse("KeyK").unwrap().key, Key::Keyboard(0x4B));
        assert_eq!(Combo::parse("Space").unwrap().key, Key::Keyboard(0x20));
        assert_eq!(Combo::parse("Mouse4").unwrap().key, Key::Mouse(0x05));
        assert_eq!(Combo::parse("Shift+MouseRight").unwrap().label(), "Shift+MouseRight");
        assert!(Combo::parse("MouseLeft").is_err(), "left click must never be a shortcut");
        assert!(Combo::parse("Ctrl").is_err(), "a modifier alone is not a key");
        assert!(record_submit("KeyK".into()).is_ok());
        *PENDING.lock().unwrap() = None;
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
