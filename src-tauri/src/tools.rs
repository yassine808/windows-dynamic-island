// Clipboard history, screenshot and color picker.
use serde::Serialize;
use std::sync::{atomic::{AtomicIsize, Ordering}, Mutex};

/// The overlay's HWND (set from main) so a screenshot can leave the island out of the picture.
pub static OVERLAY: AtomicIsize = AtomicIsize::new(0);
static CLIPS: Mutex<Vec<String>> = Mutex::new(Vec::new());
const MAX_CLIPS: usize = 6;

#[derive(Serialize)]
pub struct Px { pub hex: String, pub r: u8, pub g: u8, pub b: u8 }

#[cfg(windows)]
mod imp {
    use super::*;
    use windows::{
        core::w,
        Win32::{
            Foundation::{HANDLE, HGLOBAL, HWND, POINT},
            Graphics::Gdi::{BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits, GetPixel, ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, SRCCOPY},
            System::{
                DataExchange::{CloseClipboard, EmptyClipboard, GetClipboardData, GetClipboardSequenceNumber, IsClipboardFormatAvailable, OpenClipboard, RegisterClipboardFormatW, SetClipboardData},
                Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE},
            },
            UI::WindowsAndMessaging::{GetCursorPos, GetSystemMetrics, SetWindowDisplayAffinity, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, WINDOW_DISPLAY_AFFINITY},
        },
    };

    const CF_UNICODETEXT: u32 = 13;

    fn read_clip() -> Option<String> {
        unsafe {
            // password managers mark their copies with this format: never record those
            let ex = RegisterClipboardFormatW(w!("ExcludeClipboardContentFromMonitorProcessing"));
            if OpenClipboard(HWND::default()).is_err() { return None; }
            let mut out = None;
            if ex == 0 || IsClipboardFormatAvailable(ex).is_err() {
                if let Ok(h) = GetClipboardData(CF_UNICODETEXT) {
                    let g = HGLOBAL(h.0);
                    let p = GlobalLock(g) as *const u16;
                    if !p.is_null() {
                        let mut n = 0usize;
                        while *p.add(n) != 0 && n < 20_000 { n += 1; }
                        out = Some(String::from_utf16_lossy(std::slice::from_raw_parts(p, n)));
                        let _ = GlobalUnlock(g);
                    }
                }
            }
            let _ = CloseClipboard();
            out
        }
    }

    pub fn set_clip(text: &str) {
        unsafe {
            if OpenClipboard(HWND::default()).is_err() { return; }
            let _ = EmptyClipboard();
            let v: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
            if let Ok(g) = GlobalAlloc(GMEM_MOVEABLE, v.len() * 2) {
                let p = GlobalLock(g) as *mut u16;
                if !p.is_null() {
                    std::ptr::copy_nonoverlapping(v.as_ptr(), p, v.len());
                    let _ = GlobalUnlock(g);
                    let _ = SetClipboardData(CF_UNICODETEXT, HANDLE(g.0));
                }
            }
            let _ = CloseClipboard();
        }
    }

    pub fn watch() {
        std::thread::spawn(|| {
            let mut last = unsafe { GetClipboardSequenceNumber() };
            loop {
                std::thread::sleep(std::time::Duration::from_millis(400));
                let seq = unsafe { GetClipboardSequenceNumber() };
                if seq == last { continue; }
                if let Some(t) = read_clip() {
                    last = seq; // only advance once it was readable, otherwise retry next tick
                    let t = t.trim().to_string();
                    if !t.is_empty() {
                        let n = t.chars().count();
                        let mut c = CLIPS.lock().unwrap();
                        c.retain(|x| *x != t);
                        c.insert(0, t);
                        c.truncate(MAX_CLIPS);
                        logf!("tools", "clipboard captured {n} chars ({} entries)", c.len());
                    }
                } else if seq.wrapping_sub(last) > 50 { last = seq; }
            }
        });
    }

    pub fn pixel() -> Option<Px> {
        unsafe {
            let mut p = POINT::default();
            GetCursorPos(&mut p).ok()?;
            let dc = GetDC(HWND::default());
            let c = GetPixel(dc, p.x, p.y).0;
            ReleaseDC(HWND::default(), dc);
            if c == 0xFFFF_FFFF { return None; }
            let (r, g, b) = ((c & 0xFF) as u8, ((c >> 8) & 0xFF) as u8, ((c >> 16) & 0xFF) as u8);
            Some(Px { hex: format!("#{r:02X}{g:02X}{b:02X}"), r, g, b })
        }
    }

    /// Grab every monitor as RGBA. The island is excluded from the capture while it runs.
    pub fn grab() -> Result<(u32, u32, Vec<u8>), String> {
        unsafe {
            let own = HWND(super::OVERLAY.load(Ordering::Relaxed) as *mut _);
            let have = !own.0.is_null();
            if have { let _ = SetWindowDisplayAffinity(own, WINDOW_DISPLAY_AFFINITY(0x11)); } // WDA_EXCLUDEFROMCAPTURE
            std::thread::sleep(std::time::Duration::from_millis(150));
            let (x, y) = (GetSystemMetrics(SM_XVIRTUALSCREEN), GetSystemMetrics(SM_YVIRTUALSCREEN));
            let (w, h) = (GetSystemMetrics(SM_CXVIRTUALSCREEN), GetSystemMetrics(SM_CYVIRTUALSCREEN));
            let sdc = GetDC(HWND::default());
            let mdc = CreateCompatibleDC(sdc);
            let bmp = CreateCompatibleBitmap(sdc, w, h);
            let old = SelectObject(mdc, bmp);
            let ok = BitBlt(mdc, 0, 0, w, h, sdc, x, y, SRCCOPY).is_ok();
            let mut bi = BITMAPINFO { bmiHeader: BITMAPINFOHEADER { biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32, biWidth: w, biHeight: -h, biPlanes: 1, biBitCount: 32, biCompression: BI_RGB.0, ..Default::default() }, ..Default::default() };
            let mut buf = vec![0u8; (w as usize) * (h as usize) * 4];
            let lines = GetDIBits(mdc, bmp, 0, h as u32, Some(buf.as_mut_ptr() as *mut _), &mut bi, DIB_RGB_COLORS);
            SelectObject(mdc, old);
            let _ = DeleteObject(bmp);
            let _ = DeleteDC(mdc);
            ReleaseDC(HWND::default(), sdc);
            if have { let _ = SetWindowDisplayAffinity(own, WINDOW_DISPLAY_AFFINITY(0)); }
            if !ok || lines == 0 { return Err("Capture failed".into()); }
            for px in buf.chunks_exact_mut(4) { px.swap(0, 2); px[3] = 255; } // BGRA -> RGBA
            Ok((w as u32, h as u32, buf))
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use super::*;
    pub fn watch() {}
    pub fn set_clip(_: &str) {}
    pub fn pixel() -> Option<Px> { None }
    pub fn grab() -> Result<(u32, u32, Vec<u8>), String> { Err("Needs Windows".into()) }
}

pub fn init() {
    logf!("tools", "clipboard watcher starting");
    imp::watch();
}

#[tauri::command]
pub fn clip_list() -> Vec<String> { CLIPS.lock().unwrap().clone() }

#[tauri::command]
pub fn clip_copy(i: usize) {
    let t = CLIPS.lock().unwrap().get(i).cloned();
    logf!("tools", "clip_copy {i} ({})", if t.is_some() { "found" } else { "empty slot" });
    if let Some(t) = t { imp::set_clip(&t); }
}

#[tauri::command]
pub fn clip_clear() {
    let n = CLIPS.lock().unwrap().len();
    CLIPS.lock().unwrap().clear();
    logf!("tools", "clip_clear ({n} entries)");
}

#[tauri::command]
pub fn set_clipboard(text: String) {
    logf!("tools", "set_clipboard {} chars", text.chars().count());
    imp::set_clip(&text);
}

#[tauri::command]
pub fn pixel_at_cursor() -> Option<Px> { imp::pixel() }

/// Save all monitors as a PNG in Pictures\IslandWin and return its path.
#[tauri::command]
pub async fn screenshot() -> Result<String, String> {
    logf!("tools", "screenshot requested");
    tauri::async_runtime::spawn_blocking(|| {
        let (w, h, rgba) = imp::grab()?;
        let base = std::env::var("USERPROFILE").map(|p| std::path::PathBuf::from(p).join("Pictures")).unwrap_or_else(|_| std::env::temp_dir());
        let dir = base.join("IslandWin");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let path = dir.join(format!("screenshot-{secs}.png"));
        let f = std::io::BufWriter::new(std::fs::File::create(&path).map_err(|e| e.to_string())?);
        let mut enc = png::Encoder::new(f, w, h);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_compression(png::Compression::Fast);
        enc.write_header().map_err(|e| e.to_string())?.write_image_data(&rgba).map_err(|e| e.to_string())?;
        logf!("tools", "screenshot saved {w}x{h} -> {}", path.display());
        Ok(path.to_string_lossy().to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}
