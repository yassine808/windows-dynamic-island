// Real-time dictation: Windows speech recognition (WinRT), typed into the focused field with SendInput. No clipboard.
use serde::Serialize;
use std::sync::{mpsc::{channel, Receiver, Sender}, Mutex};

#[derive(Serialize, Clone, Default)]
pub struct SttState { pub active: bool, pub partial: String, pub text: String, pub err: String }

static STATE: Mutex<SttState> = Mutex::new(SttState { active: false, partial: String::new(), text: String::new(), err: String::new() });
static STOP: Mutex<Option<Sender<()>>> = Mutex::new(None);

#[tauri::command]
pub fn stt_state() -> SttState { STATE.lock().unwrap().clone() }

#[tauri::command]
pub fn stt_toggle() { toggle() }

pub fn toggle() {
    let mut stop = STOP.lock().unwrap();
    if let Some(tx) = stop.take() {
        if tx.send(()).is_ok() { return; }
    }
    let (tx, rx) = channel();
    *stop = Some(tx);
    *STATE.lock().unwrap() = SttState { active: true, ..Default::default() };
    std::thread::spawn(move || {
        if let Err(e) = imp::run(rx) { STATE.lock().unwrap().err = e; }
        let mut s = STATE.lock().unwrap();
        s.active = false;
        s.partial.clear();
    });
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
        Win32::UI::Input::KeyboardAndMouse::{SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, VIRTUAL_KEY},
    };

    fn type_text(s: &str) {
        let mut v: Vec<INPUT> = Vec::new();
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
    }

    pub fn run(rx: Receiver<()>) -> Result<(), String> {
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
