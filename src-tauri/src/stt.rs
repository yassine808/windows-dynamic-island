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

/// Commands handled by the single, long-lived recognition thread. Keeping the thread (and the
/// recognizer it owns) alive removes the ~400-600 ms setup cost from every press: only the first
/// press compiles the dictation constraint, later presses just start the continuous session.
enum Cmd { Start, Stop }
static CMD: OnceLock<Sender<Cmd>> = OnceLock::new();

fn sender() -> &'static Sender<Cmd> {
    CMD.get_or_init(|| {
        let (tx, rx) = channel();
        thread::spawn(move || imp::worker(rx));
        tx
    })
}

/// Start the recognition thread at boot so its COM apartment and, when possible, the recognizer
/// are ready before the first shortcut press. A failure here is silent (no Settings popup).
pub fn warm() { let _ = sender(); }

/// Called by the UI once it has detected the browser speech engine.
#[tauri::command]
pub fn stt_engine(browser: bool) {
    BROWSER.store(browser, Ordering::Relaxed);
    // the model name lands in benchmarks/stt-latency.csv, so each row records what transcribed it
    let model = if browser { "edge-speech-ceto" } else { "windows-speech-winrt" };
    logf!("stt", "engine -> {} model={}", if browser { "browser (streaming, no key)" } else { "winrt" }, model);
}

/// The browser engine ended on its own (error, long silence): keep the Rust state honest.
#[tauri::command]
pub fn stt_ended() {
    let mut s = STATE.lock().unwrap();
    s.active = false;
    s.partial.clear();
    drop(s);
    logf!("stt", "browser engine reported the session ended");
    emit("stt://state");
}

/// Type a finished phrase into the focused window (same SendInput path the WinRT engine uses).
#[tauri::command]
pub fn stt_type(text: String) -> Result<usize, String> {
    let out = if text.ends_with(' ') { text } else { format!("{text} ") };
    let t = std::time::Instant::now();
    let n = imp::type_out(&out)?;
    // the benchmark script reads this line: keystroke cost for one finished phrase
    logf!("stt", "type: {} chars in {:.1} ms", out.chars().count(), t.elapsed().as_secs_f64() * 1000.0);
    Ok(n)
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
    if STATE.lock().unwrap().active {
        logf!("stt", "start: already listening");
        return;
    }
    // The UI shows listening right away; the worker reports a failure (and flips this back) if the
    // recognizer cannot start.
    *STATE.lock().unwrap() = SttState { active: true, ..Default::default() };
    emit("stt://state");
    logf!("stt", "start: requesting the WinRT worker (hold={})", hold());
    if sender().send(Cmd::Start).is_err() {
        let mut s = STATE.lock().unwrap();
        s.active = false;
        s.err = "Dictation worker is not running".into();
        drop(s);
        emit("stt://state");
    }
}

pub fn stop() {
    WANT.store(false, Ordering::Relaxed);
    if BROWSER.load(Ordering::Relaxed) {
        STATE.lock().unwrap().active = false;
        logf!("stt", "stop: browser engine");
        emit("stt://stop");
        return;
    }
    // Stop the UI immediately. The worker stops the WinRT session in the background; waiting for
    // StopAsync used to leave the island listening for up to 20 seconds after the shortcut was
    // released.
    let was = {
        let mut s = STATE.lock().unwrap();
        let was = s.active;
        s.active = false;
        s.partial.clear();
        was
    };
    logf!("stt", "stop requested (was_listening={was})");
    if was { emit("stt://state"); }
    if let Some(tx) = CMD.get() { let _ = tx.send(Cmd::Stop); }
}

#[cfg(windows)]
mod imp {
    use super::*;
    use windows::{
        core::HSTRING,
        Foundation::{AsyncStatus, TypedEventHandler},
        Media::SpeechRecognition::{
            SpeechContinuousRecognitionResultGeneratedEventArgs as ResArgs, SpeechContinuousRecognitionSession as Session,
            SpeechRecognitionHypothesisGeneratedEventArgs as HypArgs, SpeechRecognitionResultStatus, SpeechRecognitionScenario,
            SpeechRecognitionTopicConstraint, SpeechRecognizer,
        },
        Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, VIRTUAL_KEY},
    };

    /// Stamped on every event this module injects: the shortcut recorder treats it as its own noise.
    const OURS: usize = 0x49534C57; // "ISLW"

    /// A recognizer plus its continuous session. Built once on the worker thread and reused.
    struct Engine { _rec: SpeechRecognizer, sess: Session }

    fn key(vk: u16, flags: KEYBD_EVENT_FLAGS) -> INPUT {
        INPUT { r#type: INPUT_KEYBOARD, Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: VIRTUAL_KEY(vk), wScan: 0, dwFlags: flags, time: 0, dwExtraInfo: OURS } } }
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
                    Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: VIRTUAL_KEY(0), wScan: u, dwFlags: flags, time: 0, dwExtraInfo: OURS } },
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

    fn privacy_error(e: &windows::core::Error) -> bool { e.code().0 as u32 == 0x8004_5509 }

    fn hint(e: &windows::core::Error) -> String {
        if privacy_error(e) {
            // 0x80045509: speech privacy policy not accepted -> open the right Settings page
            let _ = std::process::Command::new("cmd").args(["/C", "start", "", "ms-settings:privacy-speech"]).spawn();
            "Turn on Online speech recognition (Settings opened)".to_string()
        } else {
            format!("{e} (Settings > Privacy > Speech > Online speech recognition must be on)")
        }
    }

    fn on_hypothesis(a: Option<&HypArgs>) -> windows::core::Result<()> {
        let Some(a) = a else { logf!("stt", "hypothesis event without args"); return Ok(()) };
        match a.Hypothesis().and_then(|h| h.Text()) {
            Ok(t) => {
                let t = t.to_string();
                logf!("stt", "hypothesis: {t:?}");
                let mut s = STATE.lock().unwrap();
                if s.active { s.partial = t; }
            }
            Err(e) => logf!("stt", "hypothesis text failed: {e:?}"),
        }
        Ok(())
    }

    fn on_result(a: Option<&ResArgs>) -> windows::core::Result<()> {
        let Some(a) = a else { logf!("stt", "result event without args"); return Ok(()) };
        let Ok(r) = a.Result() else { logf!("stt", "result event without a result"); return Ok(()) };
        let status = match r.Status() { Ok(s) => format!("{s:?}"), Err(e) => format!("<error {e:?}>") };
        let confidence = match r.Confidence() { Ok(c) => format!("{c:?}"), Err(_) => "?".into() };
        logf!("stt", "result: status={status} confidence={confidence}");
        if !r.Status().map(|s| s == SpeechRecognitionResultStatus::Success).unwrap_or(false) { return Ok(()); }
        match r.Text() {
            Ok(t) => {
                let t = t.to_string();
                logf!("stt", "result text: {t:?}");
                if !t.trim().is_empty() {
                    let out = format!("{} ", t.trim());
                    let started = std::time::Instant::now();
                    match type_text(&out) {
                        Ok(n) => logf!("stt", "typed the phrase as {n} key events in {:.1} ms", started.elapsed().as_secs_f64() * 1000.0),
                        Err(e) => logf!("stt", "typing failed: {e}"),
                    }
                    let mut s = STATE.lock().unwrap();
                    s.text.push_str(&out);
                    s.partial.clear();
                    logf!("stt", "transcript: {} chars total", s.text.len());
                    drop(s);
                    // a finished phrase can be the last thing a short hold produces
                    emit("stt://state");
                } else {
                    logf!("stt", "empty result text, nothing typed");
                }
            }
            Err(e) => logf!("stt", "result text read failed: {e:?}"),
        }
        Ok(())
    }

    /// Build the recognizer and compile a dictation grammar. Without a topic constraint the
    /// continuous session starts but never matches free speech, which is why the old build logged
    /// zero hypotheses and zero results.
    fn build() -> windows::core::Result<Engine> {
        let rec = SpeechRecognizer::new()?;
        logf!("stt", "worker: recognizer created");
        let constraints = rec.Constraints()?;
        constraints.Clear()?;
        let topic = SpeechRecognitionTopicConstraint::Create(SpeechRecognitionScenario::Dictation, &HSTRING::from("dictation"))?;
        constraints.Append(&topic)?;
        logf!("stt", "worker: dictation constraint added, compiling");
        let comp = rec.CompileConstraintsAsync()?.get()?;
        let status = comp.Status()?;
        logf!("stt", "worker: constraint status {status:?}");
        if status != SpeechRecognitionResultStatus::Success { return Err(windows::core::Error::from_hresult(windows::Win32::Foundation::E_FAIL)); }
        rec.HypothesisGenerated(&TypedEventHandler::<SpeechRecognizer, HypArgs>::new(|_, a| on_hypothesis(a.as_ref())))?;
        let sess = rec.ContinuousRecognitionSession()?;
        sess.ResultGenerated(&TypedEventHandler::<Session, ResArgs>::new(|_, a| on_result(a.as_ref())))?;
        logf!("stt", "worker: recognizer ready (warm)");
        Ok(Engine { _rec: rec, sess })
    }

    /// Stop the session without blocking forever. The old `.get()` could wait ~20 s; a hold release
    /// kept the mic open the whole time. 500 ms is enough to flush the last phrase, then cancel.
    fn stop_session(sess: &Session) -> bool {
        let started = std::time::Instant::now();
        let action = match sess.StopAsync() { Ok(a) => a, Err(e) => { logf!("stt", "worker: StopAsync failed: {e:?}"); return false; } };
        loop {
            match action.Status() {
                Ok(AsyncStatus::Completed) => {
                    if let Err(e) = action.GetResults() { logf!("stt", "worker: stop results failed: {e:?}"); }
                    logf!("stt", "worker: stopped in {} ms", started.elapsed().as_millis());
                    return true;
                }
                Ok(AsyncStatus::Canceled) => { logf!("stt", "worker: stop cancelled after {} ms", started.elapsed().as_millis()); return true; }
                Ok(AsyncStatus::Error) => { logf!("stt", "worker: stop failed after {} ms", started.elapsed().as_millis()); return false; }
                _ => {}
            }
            if started.elapsed() > Duration::from_millis(500) {
                logf!("stt", "worker: stop took >500 ms, cancelling the session");
                let _ = action.Cancel();
                let _ = sess.CancelAsync();
                return false;
            }
            thread::sleep(Duration::from_millis(20));
        }
    }

    fn fail(e: String) {
        logf!("stt", "worker: {e}");
        let mut s = STATE.lock().unwrap();
        s.active = false;
        s.partial.clear();
        s.err = e;
        drop(s);
        emit("stt://state");
    }

    /// The single recognition thread: owns the recognizer and handles one command at a time.
    pub fn worker(rx: Receiver<Cmd>) {
        // this worker thread needs its own WinRT apartment before touching SpeechRecognizer
        let hr = unsafe { windows::Win32::System::WinRT::RoInitialize(windows::Win32::System::WinRT::RO_INIT_MULTITHREADED) };
        logf!("stt", "worker: RoInitialize -> {hr:?}");
        // Pre-warm quietly: if speech is not set up yet, wait for the first real press so the
        // Settings popup only appears when the user actually asks for dictation.
        let mut engine = match build() {
            Ok(e) => Some(e),
            Err(e) => { logf!("stt", "worker: pre-warm skipped: {e:?}"); None }
        };
        // A quick tap can send Start then Stop before the session ever runs. Stopping an idle
        // session fails and used to drop the warm recognizer, so track the real state.
        let mut listening = false;
        loop {
            match rx.recv() {
                Ok(Cmd::Start) => {
                    if !WANT.load(Ordering::Relaxed) { logf!("stt", "worker: queued start cancelled (shortcut already released)"); continue; }
                    if listening { logf!("stt", "worker: start ignored, already listening"); continue; }
                    if engine.is_none() {
                        match build() {
                            Ok(e) => engine = Some(e),
                            Err(e) => { fail(hint(&e)); continue; }
                        }
                    }
                    let Some(e) = engine.as_ref() else { continue };
                    if !WANT.load(Ordering::Relaxed) { logf!("stt", "worker: start cancelled before listening"); continue; }
                    logf!("stt", "worker: starting the continuous session");
                    match e.sess.StartAsync().and_then(|a| a.get()) {
                        Ok(()) => { listening = true; logf!("stt", "worker: listening"); emit("stt://state"); }
                        Err(err) => { fail(hint(&err)); engine = None; }
                    }
                }
                Ok(Cmd::Stop) => {
                    if !listening { logf!("stt", "worker: stop with no session running"); continue; }
                    let clean = match engine.as_ref() { Some(e) => stop_session(&e.sess), None => true };
                    listening = false;
                    if !clean {
                        // a cancelled session is not reusable: rebuild it on the next press
                        logf!("stt", "worker: dropping the recognizer after an unclean stop");
                        engine = None;
                    }
                }
                Err(_) => break,
            }
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use super::*;
    pub fn type_out(_: &str) -> Result<usize, String> { Err("Typing needs Windows".into()) }
    pub fn worker(_: Receiver<Cmd>) { logf!("stt", "worker: not on Windows"); }
}
