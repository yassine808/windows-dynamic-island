// Append-only log file so the island can be inspected after the fact.
// Default: %LOCALAPPDATA%\islandwin\islandwin.log — override the path with ISLANDWIN_LOG.
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
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("islandwin")
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
    print!("{line}");
}
