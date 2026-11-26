// Append-only log file so the island can be inspected after the fact.
// Default: %APPDATA%\Windows Dynamic Island\logs\islandwin.log (beside settings.json, outside the
// install folder so an upgrade keeps it). Override the path with ISLANDWIN_LOG.
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::PathBuf,
    sync::{Mutex, OnceLock},
    time::Instant,
};

static FILE: Mutex<Option<File>> = Mutex::new(None);
static BYTES: Mutex<u64> = Mutex::new(0);
static START: OnceLock<Instant> = OnceLock::new();
// Hard cap for the live file. When the next line would pass it, the file rotates to
// islandwin.old.log (one previous file) and a fresh one starts, so neither file exceeds 2 MB.
const MAX_BYTES: u64 = 2 * 1024 * 1024;

pub fn path() -> PathBuf {
    if let Some(p) = std::env::var_os("ISLANDWIN_LOG") {
        return PathBuf::from(p);
    }
    // Beside settings.json, outside the install directory: an upgrade or uninstall keeps it.
    crate::paths::data_dir().join("logs").join("islandwin.log")
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

fn open(p: &PathBuf) -> Option<File> {
    OpenOptions::new().create(true).append(true).open(p).ok()
}

/// Rename the live file to islandwin.old.log and start a new one. Only one previous file is kept.
fn rotate(p: &PathBuf) -> Option<File> {
    let old = p.with_extension("old.log");
    let _ = fs::remove_file(&old);
    let _ = fs::rename(p, &old);
    open(p)
}

pub fn init() {
    let p = path();
    if let Some(d) = p.parent() { let _ = fs::create_dir_all(d); }
    // old settings and logs were copied by paths::migrate() before this runs
    // a file left over the cap by an older build is rotated once at startup
    if fs::metadata(&p).map(|m| m.len() > MAX_BYTES).unwrap_or(false) {
        let _ = fs::remove_file(p.with_extension("old.log"));
        let _ = fs::rename(&p, p.with_extension("old.log"));
    }
    match open(&p) {
        Some(f) => *FILE.lock().unwrap() = Some(f),
        None => eprintln!("[log] cannot open {}", p.display()),
    }
    if let Ok(mut n) = BYTES.lock() { *n = fs::metadata(&p).map(|m| m.len()).unwrap_or(0); }
    START.get_or_init(Instant::now);
    write("boot", &format!("islandwin {} starting", env!("CARGO_PKG_VERSION")));
    write("boot", &format!("log file: {}", p.display()));
    write("boot", &format!("pid: {} cwd: {:?}", std::process::id(), std::env::current_dir().unwrap_or_default()));
}

/// Append one line, flush it immediately, and echo it to the console.
pub fn write(tag: &str, msg: &str) {
    let t = START.get_or_init(Instant::now).elapsed().as_secs_f64();
    let mut line = format!("{} +{:8.3}s [{}] {}\n", stamp(), t, tag, msg);
    // a panic backtrace can be huge: one line must never pass the cap on its own
    if line.len() as u64 > MAX_BYTES {
        let mut cut = MAX_BYTES as usize - 1;
        while cut > 0 && !line.is_char_boundary(cut) { cut -= 1; }
        line.truncate(cut);
        line.push('\n');
    }
    if let Ok(mut g) = FILE.lock() {
        if let Ok(mut n) = BYTES.lock() {
            if g.is_some() && *n + line.len() as u64 > MAX_BYTES {
                let p = path();
                *g = None;
                *g = rotate(&p);
                *n = 0;
            }
            if let Some(f) = g.as_mut() {
                let _ = f.write_all(line.as_bytes());
                let _ = f.flush();
                *n += line.len() as u64;
            }
        }
    }
    // A packaged app has no console, and `tauri dev` can lose one when the parent shell exits.
    // `print!` panics on a write error, which would take the whole app down just for a log line.
    let _ = std::io::stdout().write_all(line.as_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_rotates_before_passing_the_cap() {
        let p = std::env::temp_dir().join(format!("islandwin-log-test-{}.log", std::process::id()));
        let old = p.with_extension("old.log");
        let _ = fs::remove_file(&p);
        let _ = fs::remove_file(&old);
        std::env::set_var("ISLANDWIN_LOG", &p);
        init();
        // ~3 MB through a 2 MB cap: the live file must rotate, and every step must stay under it.
        for _ in 0..48 {
            write("test", &"x".repeat(64 * 1024));
            let len = fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
            assert!(len <= MAX_BYTES, "live log grew to {len} bytes");
        }
        let old_len = fs::metadata(&old).map(|m| m.len()).unwrap_or(0);
        assert!(old_len > 0, "no rotated file was kept");
        assert!(old_len <= MAX_BYTES, "rotated log grew to {old_len} bytes");
        *FILE.lock().unwrap() = None;
        let _ = fs::remove_file(&p);
        let _ = fs::remove_file(&old);
    }
}
