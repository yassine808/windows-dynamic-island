// Real-time dictation: Windows speech recognition (WinRT), typed into the focused field with SendInput. No clipboard.
use serde::Serialize;
use std::sync::{atomic::{AtomicBool, Ordering}, mpsc::{channel, Receiver, Sender}, Mutex};

#[derive(Serialize, Clone, Default)]
pub struct SttState { pub active: bool, pub partial: String, pub text: String, pub err: String }

static STATE: Mutex<SttState> = Mutex::new(SttState { active: false, partial: String::new(), text: String::new(), err: String::new() });
static STOP: Mutex<Option<Sender<()>>> = Mutex::new(None);
/// true = push-to-talk (listen while the shortcut is held), false = press once to start, again to stop
pub static HOLD: AtomicBool = AtomicBool::new(false);
pub fn hold() -> bool { HOLD.load(Ordering::Relaxed) }

#[tauri::command]
pub fn stt_state() -> SttState { STATE.lock().unwrap().clone() }

#[tauri::command]
pub fn stt_toggle() { toggle() }

pub fn toggle() {
    if STATE.lock().unwrap().active { stop() } else { start() }
}

pub fn start() {
    if STATE.lock().unwrap().active { return; }
    let (tx, rx) = channel();
    *STOP.lock().unwrap() = Some(tx);
    *STATE.lock().unwrap() = SttState { active: true, ..Default::default() };
    std::thread::spawn(move || {
        if let Err(e) = imp::run(rx) { STATE.lock().unwrap().err = e; }
        let mut s = STATE.lock().unwrap();
        s.active = false;
        s.partial.clear();
    });
}

pub fn stop() {
    if let Some(tx) = STOP.lock().unwrap().take() { let _ = tx.send(()); }
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

    fn type_text(s: &str) {
        // Hold-to-talk keeps Ctrl/Alt/... physically down: lift them while typing so the text isn't read as shortcuts, then put them back.
        let held: Vec<u16> = [0x11u16, 0x12, 0x10, 0x5B, 0x5C].into_iter().filter(|&k| unsafe { (GetAsyncKeyState(k as i32) as u16) & 0x8000 != 0 }).collect();
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
        unsafe { SendInput(&v, std::mem::size_of::<INPUT>() as i32); }
        // put back only what is still physically held (never leave a modifier stuck)
        let back: Vec<INPUT> = held.iter().filter(|&&k| unsafe { (GetAsyncKeyState(k as i32) as u16) & 0x8000 != 0 }).map(|&k| key(k, KEYBD_EVENT_FLAGS(0))).collect();
        if !back.is_empty() { unsafe { SendInput(&back, std::mem::size_of::<INPUT>() as i32); } }
    }

    pub fn run(rx: Receiver<()>) -> Result<(), String> {
        // this worker thread needs its own WinRT apartment before touching SpeechRecognizer
        unsafe { let _ = windows::Win32::System::WinRT::RoInitialize(windows::Win32::System::WinRT::RO_INIT_MULTITHREADED); }
        let hint = |x: windows::core::Error| format!("{} (Settings > Privacy > Speech > Online speech recognition must be on)", x);
        let rec = SpeechRecognizer::new().map_err(hint)?;
        let comp = rec.CompileConstraintsAsync().map_err(hint)?.get().map_err(hint)?;
        if comp.Status().map_err(hint)? != SpeechRecognitionResultStatus::Success {
            return Err("Speech setup failed".into());
        }
        rec.HypothesisGenerated(&TypedEventHandler::<SpeechRecognizer, HypArgs>::new(|_, a| {
            if let Some(a) = a.as_ref() {
                if let Ok(t) = a.Hypothesis().and_then(|h| h.Text()) { STATE.lock().unwrap().partial = t.to_string(); }
            }
            Ok(())
        })).map_err(hint)?;
        let sess = rec.ContinuousRecognitionSession().map_err(hint)?;
        sess.ResultGenerated(&TypedEventHandler::<Session, ResArgs>::new(|_, a| {
            if let Some(a) = a.as_ref() {
                if let Ok(r) = a.Result() {
                    if r.Status().map(|s| s == SpeechRecognitionResultStatus::Success).unwrap_or(false) {
                        if let Ok(t) = r.Text() {
                            let t = t.to_string();
                            if !t.trim().is_empty() {
                                let out = format!("{} ", t.trim());
                                type_text(&out); // finished phrase goes into the focused field
                                let mut s = STATE.lock().unwrap();
                                s.text.push_str(&out);
                                s.partial.clear();
                            }
                        }
                    }
                }
            }
            Ok(())
        })).map_err(hint)?;
        sess.StartAsync().map_err(hint)?.get().map_err(hint)?;
        let _ = rx.recv();
        let _ = sess.StopAsync().and_then(|a| a.get());
        Ok(())
    }
}

#[cfg(not(windows))]
mod imp {
    use super::*;
    pub fn run(_: Receiver<()>) -> Result<(), String> { Err("Dictation needs Windows".into()) }
}
