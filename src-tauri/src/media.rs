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
    use std::cell::RefCell;
    use windows::Media::Control::{
        GlobalSystemMediaTransportControlsSession as Session,
        GlobalSystemMediaTransportControlsSessionManager as Manager,
        GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
    };
    use windows::Storage::Streams::DataReader;

    // The manager is expensive to activate (WinRT async round trip) and the frontend polls once a
    // second; keep one per worker thread and just ask it for the current session each time.
    thread_local! {
        static MANAGER: RefCell<Option<Manager>> = const { RefCell::new(None) };
    }

    fn manager() -> Option<Manager> {
        MANAGER.with(|m| {
            let mut slot = m.borrow_mut();
            if slot.is_none() {
                *slot = Manager::RequestAsync().ok().and_then(|a| a.get().ok());
            }
            slot.clone()
        })
    }

    fn session() -> Option<Session> {
        manager()?.GetCurrentSession().ok()
    }

    pub fn poll() -> Option<MediaInfo> {
        let s = session()?;
        let p = s.TryGetMediaPropertiesAsync().ok()?.get().ok()?;
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

    /// Fetch the thumbnail for exactly the song the UI asked for. The session can switch between
    /// the metadata poll and this call; returning the new song's cover for the old key is what left
    /// the pill showing the previous image, so a mismatched key returns nothing and the UI asks
    /// again for the song that is actually current.
    pub fn art_for(key: &str) -> Option<ArtReply> {
        let p = session()?.TryGetMediaPropertiesAsync().ok()?.get().ok()?;
        let title = p.Title().ok()?.to_string();
        let artist = p.Artist().map(|a| a.to_string()).unwrap_or_default();
        if format!("{title}|{artist}") != key {
            return None;
        }
        let stream = p.Thumbnail().ok()?.OpenReadAsync().ok()?.get().ok()?;
        let size = stream.Size().ok()? as u32;
        let reader = DataReader::CreateDataReader(&stream).ok()?;
        reader.LoadAsync(size).ok()?.get().ok()?;
        let mut buf = vec![0u8; size as usize];
        reader.ReadBytes(&mut buf).ok()?;
        Some(ArtReply { key: key.to_string(), art: format!("data:image/png;base64,{}", STANDARD.encode(buf)) })
    }

    pub fn seek(ms: i64) {
        if let Some(s) = session() {
            let st = s.GetTimelineProperties().ok().and_then(|t| t.StartTime().ok()).map(|d| d.Duration).unwrap_or(0);
            let _ = s.TryChangePlaybackPositionAsync(st + ms * 10_000).and_then(|a| a.get());
        }
    }

    pub fn control(action: &str) {
        if let Some(s) = session() {
            let _ = match action {
                "toggle" => s.TryTogglePlayPauseAsync().and_then(|a| a.get()),
                "next" => s.TrySkipNextAsync().and_then(|a| a.get()),
                "prev" => s.TrySkipPreviousAsync().and_then(|a| a.get()),
                _ => Ok(false),
            };
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use super::*;
    pub fn poll() -> Option<MediaInfo> { None }
    pub fn art_for(_: &str) -> Option<ArtReply> { None }
    pub fn control(_: &str) {}
    pub fn seek(_: i64) {}
}

/// Push a new song, artist or play/pause state to the webview as soon as the media session reports
/// it. The webview still polls once a second for the playback position; this only removes the wait
/// for the metadata that made a song appear up to two seconds late.
pub fn watch(app: tauri::AppHandle) {
    std::thread::spawn(move || {
        let mut last: Option<(String, String, bool)> = None;
        loop {
            let m = imp::poll();
            let key = m.as_ref().map(|x| (x.title.clone(), x.artist.clone(), x.playing));
            if key != last {
                match &m {
                    Some(x) => logf!("media", "session: {} - {} ({})", x.title, x.artist, if x.playing { "playing" } else { "paused" }),
                    None => logf!("media", "session: none"),
                }
                last = key;
                let _ = app.emit("media", &m);
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
