use base64::{engine::general_purpose::STANDARD, Engine};
use serde::Serialize;
use tauri::Emitter;

#[derive(Serialize, Clone)]
pub struct MediaInfo {
    pub title: String,
    pub artist: String,
    pub playing: bool,
    pub position: i64,
    pub duration: i64,
}

#[derive(Serialize)]
pub struct ArtReply {
    pub key: String,
    pub art: String,
}

#[cfg(windows)]
mod imp {
    use super::*;
    use std::{
        cell::RefCell,
        time::{Duration, Instant},
    };
    use windows::{
        Foundation::{AsyncStatus, IAsyncOperation},
        Media::Control::{
            GlobalSystemMediaTransportControlsSession as Session,
            GlobalSystemMediaTransportControlsSessionManager as Manager,
            GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
        },
        Storage::Streams::DataReader,
    };

    /// Cap for one cover. A 640² JPEG is well under 1 MB; anything above this is a broken stream,
    /// and the old code allocated whatever `Size()` claimed (with a u64→u32 wrap).
    const MAX_ART: u64 = 8 * 1024 * 1024;
    /// SMTC calls are out-of-process; a player that stops servicing its session must not freeze the
    /// watcher or the UI forever.
    const TIMEOUT: Duration = Duration::from_millis(2000);

    // The manager is expensive to activate (WinRT async round trip) and the frontend polls once a
    // second; keep one per worker thread and just ask it for the current session each time.
    thread_local! {
        static MANAGER: RefCell<Option<Manager>> = const { RefCell::new(None) };
    }

    /// Wait for a WinRT operation with a deadline instead of `.get()`, which waits forever.
    fn wait<T: windows::core::RuntimeType + 'static>(op: &IAsyncOperation<T>, timeout: Duration) -> Option<T> {
        let start = Instant::now();
        loop {
            match op.Status() {
                Ok(AsyncStatus::Completed) => return op.GetResults().ok(),
                Ok(AsyncStatus::Canceled) | Ok(AsyncStatus::Error) => return None,
                _ => {}
            }
            if start.elapsed() > timeout {
                logf!("media", "WinRT call took >{} ms, cancelling", timeout.as_millis());
                let _ = op.Cancel();
                return None;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// Same deadline for `DataReader::LoadAsync`, which returns its own operation type rather than
    /// a bare `IAsyncOperation<u32>`.
    fn wait_load(op: &windows::Storage::Streams::DataReaderLoadOperation, timeout: Duration) -> Option<u32> {
        let start = Instant::now();
        loop {
            match op.Status() {
                Ok(AsyncStatus::Completed) => return op.GetResults().ok(),
                Ok(AsyncStatus::Canceled) | Ok(AsyncStatus::Error) => return None,
                _ => {}
            }
            if start.elapsed() > timeout {
                logf!("media", "thumbnail load took >{} ms, cancelling", timeout.as_millis());
                let _ = op.Cancel();
                return None;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn manager() -> Option<Manager> {
        MANAGER.with(|m| {
            let mut slot = m.borrow_mut();
            if slot.is_none() {
                *slot = Manager::RequestAsync().ok().and_then(|a| wait(&a, TIMEOUT));
            }
            slot.clone()
        })
    }

    fn session() -> Option<Session> {
        manager()?.GetCurrentSession().ok()
    }

    /// Playback status only: one cheap call the watcher can make four times a second.
    pub fn playing() -> Option<bool> {
        let s = session()?;
        Some(s.GetPlaybackInfo().ok()?.PlaybackStatus().ok()? == Status::Playing)
    }

    pub fn poll() -> Option<MediaInfo> {
        let s = session()?;
        let p = wait(&s.TryGetMediaPropertiesAsync().ok()?, TIMEOUT)?;
        let title = p.Title().ok()?.to_string();
        if title.is_empty() {
            return None;
        }
        let playing = s.GetPlaybackInfo().ok()?.PlaybackStatus().ok()? == Status::Playing;
        let ms = |d: windows::Foundation::TimeSpan| d.Duration / 10_000;
        let (position, duration) = s.GetTimelineProperties().ok().map(|t| {
            let st = t.StartTime().map(ms).unwrap_or(0);
            (t.Position().map(ms).unwrap_or(0) - st, t.EndTime().map(ms).unwrap_or(0) - st)
        }).unwrap_or((0, 0));
        Some(MediaInfo { title, artist: p.Artist().map(|a| a.to_string()).unwrap_or_default(), playing, position, duration })
    }

    /// Fetch the thumbnail for exactly the song the UI asked for. `None` means "ask again" (the
    /// session moved to another song); `Some` with an empty `art` means "this song has no cover",
    /// so the UI stops retrying a track that will never have one.
    pub fn art_for(key: &str) -> Option<ArtReply> {
        let p = wait(&session()?.TryGetMediaPropertiesAsync().ok()?, TIMEOUT)?;
        let title = p.Title().ok()?.to_string();
        let artist = p.Artist().map(|a| a.to_string()).unwrap_or_default();
        if format!("{title}|{artist}") != key {
            return None;
        }
        let empty = || Some(ArtReply { key: key.to_string(), art: String::new() });
        let Some(stream) = p.Thumbnail().ok().and_then(|t| t.OpenReadAsync().ok()).and_then(|a| wait(&a, TIMEOUT)) else {
            return empty();
        };
        let Ok(size) = stream.Size() else { return empty() };
        if size == 0 || size > MAX_ART {
            logf!("media", "thumbnail size {size} is not usable, showing no cover");
            return empty();
        }
        let Ok(reader) = DataReader::CreateDataReader(&stream) else { return empty() };
        let Some(loaded) = reader.LoadAsync(size as u32).ok().and_then(|a| wait_load(&a, TIMEOUT)) else { return empty() };
        if loaded as u64 != size { return empty(); }
        let mut buf = vec![0u8; size as usize];
        if reader.ReadBytes(&mut buf).is_err() { return empty(); }
        Some(ArtReply { key: key.to_string(), art: format!("data:image/png;base64,{}", STANDARD.encode(buf)) })
    }

    pub fn seek(ms: i64) {
        let Some(s) = session() else { return };
        let st = s.GetTimelineProperties().ok().and_then(|t| t.StartTime().ok()).map(|d| d.Duration).unwrap_or(0);
        if let Ok(op) = s.TryChangePlaybackPositionAsync(st + ms * 10_000) {
            let _ = wait(&op, TIMEOUT);
        }
    }

    pub fn control(action: &str) {
        let Some(s) = session() else { return };
        let op = match action {
            "toggle" => s.TryTogglePlayPauseAsync(),
            "next" => s.TrySkipNextAsync(),
            "prev" => s.TrySkipPreviousAsync(),
            _ => return,
        };
        if let Ok(op) = op {
            let _ = wait(&op, TIMEOUT);
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use super::*;
    pub fn playing() -> Option<bool> { None }
    pub fn poll() -> Option<MediaInfo> { None }
    pub fn art_for(_: &str) -> Option<ArtReply> { None }
    pub fn control(_: &str) {}
    pub fn seek(_: i64) {}
}

/// Push a new song, artist or play/pause state to the webview as soon as the media session reports
/// it. The webview still polls once a second for the playback position; this only removes the wait
/// for the metadata that made a song appear up to two seconds late.
///
/// Playback status is cheap and in-process, so it is read every 250 ms. The full metadata read is a
/// cross-process call to the player: once a second, plus immediately when play/pause flips (which
/// usually means a new track), instead of four times a second forever.
pub fn watch(app: tauri::AppHandle) {
    std::thread::spawn(move || {
        let mut last: Option<(String, String, bool)> = None;
        let mut cached: Option<MediaInfo> = None;
        let mut sampled = false;
        let mut last_meta = std::time::Instant::now();
        let mut last_playing: Option<bool> = None;
        loop {
            let playing = imp::playing();
            let now = std::time::Instant::now();
            if !sampled || playing != last_playing || now.duration_since(last_meta) >= std::time::Duration::from_millis(1000) {
                sampled = true;
                last_meta = now;
                last_playing = playing;
                cached = imp::poll();
            }
            let key = cached.as_ref().map(|x| (x.title.clone(), x.artist.clone(), x.playing));
            if key != last {
                match &cached {
                    Some(x) => logf!("media", "session: {} - {} ({})", x.title, x.artist, if x.playing { "playing" } else { "paused" }),
                    None => logf!("media", "session: none"),
                }
                last = key;
                let _ = app.emit("media", &cached);
            }
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
    });
}

// async commands run off the main thread; WinRT .get() on the UI thread can deadlock it.
#[tauri::command]
pub async fn media_poll() -> Option<MediaInfo> {
    tauri::async_runtime::spawn_blocking(imp::poll).await.ok().flatten()
}

#[tauri::command]
pub async fn media_art(key: String) -> Option<ArtReply> {
    tauri::async_runtime::spawn_blocking(move || imp::art_for(&key)).await.ok().flatten()
}

#[tauri::command]
pub async fn media_control(action: String) {
    logf!("media", "control: {action}");
    let _ = tauri::async_runtime::spawn_blocking(move || imp::control(&action)).await;
}

#[tauri::command]
pub async fn media_seek(ms: i64) {
    logf!("media", "seek: {ms} ms");
    let _ = tauri::async_runtime::spawn_blocking(move || imp::seek(ms)).await;
}
