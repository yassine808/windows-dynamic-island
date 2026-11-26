// Real-time dictation: Windows speech recognition (WinRT), typed into the focused field with SendInput. No clipboard.
use serde::Serialize;
use std::{
    sync::{atomic::{AtomicBool, Ordering}, mpsc::{channel, Receiver, Sender}, Mutex, OnceLock},
    thread,
    time::Duration,
};
use tauri::Emitter;

#[derive(Serialize, Clone, Default)]
pub struct SttState { pub active: bool, pub partial: String, pub text: String, pub err: String }

static STATE: Mutex<SttState> = Mutex::new(SttState { active: false, partial: String::new(), text: String::new(), err: String::new() });
static STOP: Mutex<Option<Sender<()>>> = Mutex::new(None);
/// true = push-to-talk (listen while the shortcut is held), false = press once to start, again to stop
pub static HOLD: AtomicBool = AtomicBool::new(false);
pub fn hold() -> bool { HOLD.load(Ordering::Relaxed) }

/// The webview can run the browser's streaming recognizer instead of WinRT: then start/stop only
/// flip this state and notify the UI, which does the actual listening.
static BROWSER: AtomicBool = AtomicBool::new(false);
/// What the user asked for last. Hold mode presses can land while a session is still closing.
static WANT: AtomicBool = AtomicBool::new(false);
static APP: OnceLock<tauri::AppHandle> = OnceLock::new();

pub fn bind(app: tauri::AppHandle) { let _ = APP.set(app); }
fn emit(ev: &str) { if let Some(a) = APP.get() { let _ = a.emit(ev, ()); } }

/// Called by the UI once it has detected the browser speech engine.
#[tauri::command]
pub fn stt_engine(browser: bool) {
    BROWSER.store(browser, Ordering::Relaxed);
    logf!("stt", "engine -> {}", if browser { "browser (streaming, no key)" } else { "winrt" });
}

/// The browser engine ended on its own (error, long silence): keep the Rust state honest.
#[tauri::command]
pub fn stt_ended() {
    let mut s = STATE.lock().unwrap();
    s.active = false;
    s.partial.clear();
    drop(s);
    logf!("stt", "browser engine reported the session ended");
}

/// Type a finished phrase into the focused window (same SendInput path the WinRT engine uses).
#[tauri::command]
pub fn stt_type(text: String) -> Result<usize, String> {
    let out = if text.ends_with(' ') { text } else { format!("{text} ") };
    logf!("stt", "type: {} chars from the browser engine", out.chars().count());
    imp::type_out(&out)
}

#[tauri::command]
pub fn stt_state() -> SttState { STATE.lock().unwrap().clone() }

#[tauri::command]
pub fn stt_toggle() { logf!("stt", "toggle requested from the UI"); toggle() }

pub fn toggle() {
    if STATE.lock().unwrap().active {
        logf!("stt", "toggle: listening -> stop");
        stop();
    } else {
        logf!("stt", "toggle: idle -> start");
        start();
    }
}

pub fn start() {
    WANT.store(true, Ordering::Relaxed);
    if BROWSER.load(Ordering::Relaxed) {
        *STATE.lock().unwrap() = SttState { active: true, ..Default::default() };
        logf!("stt", "start: browser engine listening");
        emit("stt://start");
        return;
    }
    // In hold mode a quick re-press can land while the previous session is still closing; queue it
    // instead of dropping the press ("start ignored" used to lose the whole utterance).
    if STATE.lock().unwrap().active {
        logf!("stt", "start: previous session still closing, queued");
        thread::spawn(|| {
            for _ in 0..100 {
                if !WANT.load(Ordering::Relaxed) { logf!("stt", "start: queued press cancelled"); return; }
                if !STATE.lock().unwrap().active { break; }
                thread::sleep(Duration::from_millis(20));
            }
            start_now();
        });
        return;
    }
    start_now();
}

fn start_now() {
    if !WANT.load(Ordering::Relaxed) || STATE.lock().unwrap().active { return; }
    let (tx, rx) = channel();
    *STOP.lock().unwrap() = Some(tx);
    *STATE.lock().unwrap() = SttState { active: true, ..Default::default() };
    logf!("stt", "start: spawning the recognition worker (hold={})", hold());
    std::thread::spawn(move || {
        logf!("stt", "worker: running");
        match imp::run(rx) {
            Ok(()) => logf!("stt", "worker: recognizer closed cleanly"),
            Err(e) => {
                logf!("stt", "worker: failed: {e}");
                STATE.lock().unwrap().err = e;
            }
        }
        let mut s = STATE.lock().unwrap();
        s.active = false;
        s.partial.clear();
        logf!("stt", "worker: stopped, transcript is {} chars", s.text.len());
    });
}

pub fn stop() {
    WANT.store(false, Ordering::Relaxed);
    if BROWSER.load(Ordering::Relaxed) {
        STATE.lock().unwrap().active = false;
        logf!("stt", "stop: browser engine");
        emit("stt://stop");
        return;
    }
    let had = STOP.lock().unwrap().take();
    logf!("stt", "stop requested ({})", if had.is_some() { "session running" } else { "nothing running" });
    if let Some(tx) = had { let _ = tx.send(()); }
}

#[cfg(windows)]
mod imp {
    use super::*;
    use windows::{
        Foundation::TypedEventHandler,
        Media::SpeechRecognition::{
            SpeechContinuousRecognitionResultGeneratedEventArgs as ResArgs, SpeechContinuousRecognitionSession as Session,
            SpeechRecognitionHypothesisGeneratedEventArgs as HypArgs, SpeechRecognitionResultStatus, SpeechRecognizer,
        },
        Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, VIRTUAL_KEY},
    };

    fn key(vk: u16, flags: KEYBD_EVENT_FLAGS) -> INPUT {
        INPUT { r#type: INPUT_KEYBOARD, Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: VIRTUAL_KEY(vk), wScan: 0, dwFlags: flags, time: 0, dwExtraInfo: 0 } } }
    }

    /// Type `s` into whatever window has focus. Returns how many key events were injected.
    fn type_text(s: &str) -> Result<usize, String> {
        // Hold-to-talk keeps Ctrl/Alt/... physically down: lift them while typing so the text isn't read as shortcuts, then put them back.
        let held: Vec<u16> = [0x11u16, 0x12, 0x10, 0x5B, 0x5C].into_iter().filter(|&k| unsafe { (GetAsyncKeyState(k as i32) as u16) & 0x8000 != 0 }).collect();
        logf!("stt", "type: {} chars, modifiers to lift: {held:?}", s.chars().count());
        let mut v: Vec<INPUT> = held.iter().map(|&k| key(k, KEYEVENTF_KEYUP)).collect();
        for u in s.encode_utf16() {
            for up in [false, true] {
                let flags = if up { KEYEVENTF_UNICODE | KEYEVENTF_KEYUP } else { KEYEVENTF_UNICODE };
                v.push(INPUT {
                    r#type: INPUT_KEYBOARD,
                    Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: VIRTUAL_KEY(0), wScan: u, dwFlags: flags, time: 0, dwExtraInfo: 0 } },
                });
            }
        }
        let sent = unsafe { SendInput(&v, std::mem::size_of::<INPUT>() as i32) };
        logf!("stt", "type: SendInput inserted {sent}/{} events", v.len());
        // put back only what is still physically held (never leave a modifier stuck)
        let back: Vec<INPUT> = held.iter().filter(|&&k| unsafe { (GetAsyncKeyState(k as i32) as u16) & 0x8000 != 0 }).map(|&k| key(k, KEYBD_EVENT_FLAGS(0))).collect();
        if !back.is_empty() {
            let n = unsafe { SendInput(&back, std::mem::size_of::<INPUT>() as i32) };
            logf!("stt", "type: restored {n}/{} held modifiers", back.len());
        }
        if sent as usize != v.len() {
            let e = unsafe { windows::Win32::Foundation::GetLastError() };
            return Err(format!("SendInput inserted {sent} of {} events (GetLastError {e:?})", v.len()));
        }
        Ok(sent as usize)
    }

    /// Public wrapper so the browser engine can reuse the same SendInput path.
    pub fn type_out(s: &str) -> Result<usize, String> { type_text(s) }

    pub fn run(rx: Receiver<()>) -> Result<(), String> {
        // this worker thread needs its own WinRT apartment before touching SpeechRecognizer
        let hr = unsafe { windows::Win32::System::WinRT::RoInitialize(windows::Win32::System::WinRT::RO_INIT_MULTITHREADED) };
        logf!("stt", "run: RoInitialize -> {hr:?}");
        let hint = |x: windows::core::Error| {
            // 0x80045509: speech privacy policy not accepted -> open the right Settings page for the user
            if x.code().0 as u32 == 0x8004_5509 {
                logf!("stt", "run: speech privacy not accepted, opening Settings > Privacy > Speech");
                let _ = std::process::Command::new("cmd").args(["/C", "start", "", "ms-settings:privacy-speech"]).spawn();
                "Turn on Online speech recognition (Settings opened)".to_string()
            } else {
                format!("{x} (Settings > Privacy > Speech > Online speech recognition must be on)")
            }
        };
        logf!("stt", "run: creating the Windows speech recognizer");
        let rec = SpeechRecognizer::new().map_err(|e| { logf!("stt", "run: SpeechRecognizer::new failed: {e:?}"); hint(e) })?;
        logf!("stt", "run: recognizer created");
        logf!("stt", "run: compiling constraints");
        let comp = rec.CompileConstraintsAsync()
            .map_err(|e| { logf!("stt", "run: CompileConstraintsAsync failed: {e:?}"); hint(e) })?
            .get()
            .map_err(|e| { logf!("stt", "run: constraint compilation failed: {e:?}"); hint(e) })?;
        let status = comp.Status().map_err(|e| { logf!("stt", "run: reading the constraint status failed: {e:?}"); hint(e) })?;
        logf!("stt", "run: constraint status {status:?}");
        if status != SpeechRecognitionResultStatus::Success {
            logf!("stt", "run: constraints are not usable, aborting");
            return Err("Speech setup failed".into());
        }
        rec.HypothesisGenerated(&TypedEventHandler::<SpeechRecognizer, HypArgs>::new(|_, a| {
            match a.as_ref() {
                Some(a) => match a.Hypothesis().and_then(|h| h.Text()) {
                    Ok(t) => {
                        let t = t.to_string();
                        logf!("stt", "hypothesis: {t:?}");
                        STATE.lock().unwrap().partial = t;
                    }
                    Err(e) => logf!("stt", "hypothesis text failed: {e:?}"),
                },
                None => logf!("stt", "hypothesis event without args"),
            }
            Ok(())
        })).map_err(|e| { logf!("stt", "run: HypothesisGenerated hook failed: {e:?}"); hint(e) })?;
        let sess = rec.ContinuousRecognitionSession().map_err(|e| { logf!("stt", "run: ContinuousRecognitionSession failed: {e:?}"); hint(e) })?;
        logf!("stt", "run: continuous session created");
        sess.ResultGenerated(&TypedEventHandler::<Session, ResArgs>::new(|_, a| {
            let Some(a) = a.as_ref() else { logf!("stt", "result event without args"); return Ok(()) };
            let Ok(r) = a.Result() else { logf!("stt", "result event without a result"); return Ok(()) };
            let status = match r.Status() { Ok(s) => format!("{s:?}"), Err(e) => format!("<error {e:?}>") };
            let confidence = match r.Confidence() { Ok(c) => format!("{c:?}"), Err(_) => "?".into() };
            logf!("stt", "result: status={status} confidence={confidence}");
            if r.Status().map(|s| s == SpeechRecognitionResultStatus::Success).unwrap_or(false) {
                match r.Text() {
                    Ok(t) => {
                        let t = t.to_string();
                        logf!("stt", "result text: {t:?}");
                        if !t.trim().is_empty() {
                            let out = format!("{} ", t.trim());
                            match type_text(&out) {
                                Ok(n) => logf!("stt", "typed the phrase as {n} key events"),
                                Err(e) => logf!("stt", "typing failed: {e}"),
                            }
                            let mut s = STATE.lock().unwrap();
                            s.text.push_str(&out);
                            s.partial.clear();
                            logf!("stt", "transcript: {} chars total", s.text.len());
                        } else {
                            logf!("stt", "empty result text, nothing typed");
                        }
                    }
                    Err(e) => logf!("stt", "result text read failed: {e:?}"),
                }
            }
            Ok(())
        })).map_err(|e| { logf!("stt", "run: ResultGenerated hook failed: {e:?}"); hint(e) })?;
        logf!("stt", "run: starting the continuous session");
        sess.StartAsync()
            .map_err(|e| { logf!("stt", "run: StartAsync failed: {e:?}"); hint(e) })?
            .get()
            .map_err(|e| { logf!("stt", "run: session start failed: {e:?}"); hint(e) })?;
        logf!("stt", "run: listening");
        let _ = rx.recv();
        logf!("stt", "run: stop signal received");
        match sess.StopAsync().and_then(|a| a.get()) {
            Ok(()) => logf!("stt", "run: session stopped"),
            Err(e) => logf!("stt", "run: session stop failed: {e:?}"),
        }
        Ok(())
    }
}

#[cfg(not(windows))]
mod imp {
    use super::*;
    pub fn type_out(_: &str) -> Result<usize, String> { Err("Typing needs Windows".into()) }
    pub fn run(_: Receiver<()>) -> Result<(), String> { logf!("stt", "run: not on Windows"); Err("Dictation needs Windows".into()) }
}
