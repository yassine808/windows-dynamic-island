// Append-only log file so the island can be inspected after the fact.
// Default: %APPDATA%\dev.yassine808.islandwin\logs\islandwin.log (beside settings.json, outside the
// install folder so an upgrade keeps it). Override the path with ISLANDWIN_LOG.
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::PathBuf,
    sync::{Mutex, OnceLock},
    time::Instant,
};

static FILE: Mutex<Option<File>> = Mutex::new(None);
static START: OnceLock<Instant> = OnceLock::new();
const MAX_BYTES: u64 = 5 * 1024 * 1024;

pub fn path() -> PathBuf {
    if let Some(p) = std::env::var_os("ISLANDWIN_LOG") {
        return PathBuf::from(p);
    }
    // Beside settings.json, outside the install directory: an upgrade or uninstall keeps it.
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("dev.yassine808.islandwin")
        .join("logs")
        .join("islandwin.log")
}

#[cfg(windows)]
fn stamp() -> String {
    use windows::Win32::System::SystemInformation::GetLocalTime;
    let t = unsafe { GetLocalTime() };
    format!("{:02}:{:02}:{:02}.{:03}", t.wHour, t.wMinute, t.wSecond, t.wMilliseconds)
}

#[cfg(not(windows))]
fn stamp() -> String {
    let s = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    format!("{:02}:{:02}:{:02}.000 UTC", s / 3600 % 24, s / 60 % 60, s % 60)
}

pub fn init() {
    let p = path();
    if let Some(d) = p.parent() { let _ = fs::create_dir_all(d); }
    // carry the log over from the old per-install location once, so history is not lost
    if !p.exists() {
        if let Some(old) = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).map(|d| d.join("islandwin").join("islandwin.log")) {
            if old.exists() { let _ = fs::copy(&old, &p); }
        }
    }
    // keep one previous file once the log grows past 5 MB
    if fs::metadata(&p).map(|m| m.len() > MAX_BYTES).unwrap_or(false) {
        let _ = fs::rename(&p, p.with_extension("old.log"));
    }
    match OpenOptions::new().create(true).append(true).open(&p) {
        Ok(f) => *FILE.lock().unwrap() = Some(f),
        Err(e) => eprintln!("[log] cannot open {}: {e}", p.display()),
    }
    START.get_or_init(Instant::now);
    write("boot", &format!("islandwin {} starting", env!("CARGO_PKG_VERSION")));
    write("boot", &format!("log file: {}", p.display()));
    write("boot", &format!("pid: {} cwd: {:?}", std::process::id(), std::env::current_dir().unwrap_or_default()));
}

/// Append one line, flush it immediately, and echo it to the console.
pub fn write(tag: &str, msg: &str) {
    let t = START.get_or_init(Instant::now).elapsed().as_secs_f64();
    let line = format!("{} +{:8.3}s [{}] {}\n", stamp(), t, tag, msg);
    if let Ok(mut g) = FILE.lock() {
        if let Some(f) = g.as_mut() {
            let _ = f.write_all(line.as_bytes());
            let _ = f.flush();
        }
    }
    // A packaged app has no console, and `tauri dev` can lose one when the parent shell exits.
    // `print!` panics on a write error, which would take the whole app down just for a log line.
    let _ = std::io::stdout().write_all(line.as_bytes());
}
