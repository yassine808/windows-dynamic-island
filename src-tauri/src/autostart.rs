// Start with Windows. The setup enables it, and the same setting can be flipped any time from the
// Settings page. One HKCU Run value, so no administrator rights are needed.
#[tauri::command]
pub fn autostart_get() -> bool { get() }

#[tauri::command]
pub fn autostart_set(on: bool) -> Result<(), String> { set(on) }

pub fn get() -> bool { imp::enabled() }

pub fn set(on: bool) -> Result<(), String> {
    logf!("autostart", "{}", if on { "enabling" } else { "disabling" });
    imp::set(on)?;
    crate::tray::sync_autostart(on);
    Ok(())
}

/// Keep the entry pointing at the exe that is actually running (a reinstall can move it).
pub fn init() {
    let exe = imp::current_exe();
    match imp::get() {
        Some(v) if v == exe => logf!("autostart", "on, {exe}"),
        Some(_) => { logf!("autostart", "on, refreshing path -> {exe}"); let _ = imp::write(&exe); }
        None => logf!("autostart", "off"),
    }
}

#[cfg(windows)]
mod imp {
    use std::ffi::c_void;
    use windows::{
        core::PCWSTR,
        Win32::{
            Foundation::ERROR_SUCCESS,
            System::Registry::{
                RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ,
                RRF_RT_REG_SZ,
            },
        },
    };

    const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const VALUE: &str = "IslandWin";

    pub fn current_exe() -> String {
        std::env::current_exe().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default()
    }

    fn wide(s: &str) -> Vec<u16> { s.encode_utf16().chain(std::iter::once(0)).collect() }

    pub fn get() -> Option<String> {
        let (sub, val) = (wide(RUN), wide(VALUE));
        let mut size = 0u32;
        unsafe {
            let _ = RegGetValueW(
                HKEY_CURRENT_USER, PCWSTR(sub.as_ptr()), PCWSTR(val.as_ptr()), RRF_RT_REG_SZ,
                None, None, Some(&mut size),
            );
            if size < 2 { return None; }
            let mut buf = vec![0u16; (size / 2) as usize];
            let mut got = size;
            let r = RegGetValueW(
                HKEY_CURRENT_USER, PCWSTR(sub.as_ptr()), PCWSTR(val.as_ptr()), RRF_RT_REG_SZ,
                None, Some(buf.as_mut_ptr() as *mut c_void), Some(&mut got),
            );
            if r != ERROR_SUCCESS { return None; }
            let n = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
            let s = String::from_utf16_lossy(&buf[..n]);
            // the Run value is stored quoted so paths with spaces run as one argument
            let s = s.trim_matches('"');
            (!s.is_empty()).then(|| s.to_string())
        }
    }

    pub fn enabled() -> bool { get().is_some() }

    pub fn write(exe: &str) -> Result<(), String> {
        let (sub, val) = (wide(RUN), wide(VALUE));
        // quoted so a path with spaces still runs as one argument
        let cmd = format!("\"{}\"", exe);
        let data: Vec<u16> = cmd.encode_utf16().chain(std::iter::once(0)).collect();
        let r = unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER, PCWSTR(sub.as_ptr()), PCWSTR(val.as_ptr()), REG_SZ.0,
                Some(data.as_ptr() as *const c_void), (data.len() * 2) as u32,
            )
        };
        (r == ERROR_SUCCESS).then_some(()).ok_or_else(|| format!("registry write failed ({r:?})"))
    }

    pub fn delete() -> Result<(), String> {
        let (sub, val) = (wide(RUN), wide(VALUE));
        let r = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, PCWSTR(sub.as_ptr()), PCWSTR(val.as_ptr())) };
        // already absent is a success
        (r == ERROR_SUCCESS || r == windows::Win32::Foundation::ERROR_FILE_NOT_FOUND)
            .then_some(())
            .ok_or_else(|| format!("registry delete failed ({r:?})"))
    }

    pub fn set(on: bool) -> Result<(), String> {
        if on { write(&current_exe()) } else { delete() }
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn current_exe() -> String { String::new() }
    pub fn get() -> Option<String> { None }
    pub fn enabled() -> bool { false }
    pub fn write(_: &str) -> Result<(), String> { Err("Needs Windows".into()) }
    pub fn delete() -> Result<(), String> { Err("Needs Windows".into()) }
    pub fn set(_: bool) -> Result<(), String> { Err("Needs Windows".into()) }
}

#[cfg(all(test, windows))]
mod tests {
    // Exercises the real HKCU Run value. Opt in locally with ISLANDWIN_E2E=1 (CI has no desktop,
    // and the test must not leave an autostart behind on a shared machine either).
    #[test]
    fn autostart_round_trip() {
        if std::env::var_os("ISLANDWIN_E2E").is_none() {
            eprintln!("skipped: set ISLANDWIN_E2E=1 to touch the real Run key");
            return;
        }
        let saved = super::imp::get();
        let before = super::imp::enabled();
        super::imp::set(false).unwrap();
        assert!(!super::imp::enabled());
        super::imp::set(true).unwrap();
        assert_eq!(super::imp::get(), Some(super::imp::current_exe()));
        match saved {
            Some(v) => super::imp::write(&v).unwrap(),
            None => super::imp::set(false).unwrap(),
        }
        assert_eq!(super::imp::enabled(), before);
    }
}
