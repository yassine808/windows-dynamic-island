use base64::{engine::general_purpose::STANDARD, Engine};
use serde::Serialize;

#[derive(Serialize, Clone)]
pub struct MediaInfo {
    pub title: String,
    pub artist: String,
    pub playing: bool,
}

#[cfg(windows)]
mod imp {
    use super::*;
    use windows::Media::Control::{
        GlobalSystemMediaTransportControlsSession as Session,
        GlobalSystemMediaTransportControlsSessionManager as Manager,
        GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
    };
    use windows::Storage::Streams::DataReader;

    fn session() -> Option<Session> {
        Manager::RequestAsync().ok()?.get().ok()?.GetCurrentSession().ok()
    }

    pub fn poll() -> Option<MediaInfo> {
        let s = session()?;
        let p = s.TryGetMediaPropertiesAsync().ok()?.get().ok()?;
        let title = p.Title().ok()?.to_string();
        if title.is_empty() {
            return None;
        }
        let playing = s.GetPlaybackInfo().ok()?.PlaybackStatus().ok()? == Status::Playing;
        Some(MediaInfo { title, artist: p.Artist().map(|a| a.to_string()).unwrap_or_default(), playing })
    }

    pub fn art() -> Option<String> {
        let p = session()?.TryGetMediaPropertiesAsync().ok()?.get().ok()?;
        let stream = p.Thumbnail().ok()?.OpenReadAsync().ok()?.get().ok()?;
        let size = stream.Size().ok()? as u32;
        let reader = DataReader::CreateDataReader(&stream).ok()?;
        reader.LoadAsync(size).ok()?.get().ok()?;
        let mut buf = vec![0u8; size as usize];
        reader.ReadBytes(&mut buf).ok()?;
        Some(format!("data:image/png;base64,{}", STANDARD.encode(buf)))
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
    pub fn art() -> Option<String> { None }
    pub fn control(_: &str) {}
}

// async commands run off the main thread; WinRT .get() on the UI thread can deadlock it.
#[tauri::command]
pub async fn media_poll() -> Option<MediaInfo> {
    tauri::async_runtime::spawn_blocking(imp::poll).await.ok().flatten()
}

#[tauri::command]
pub async fn media_art() -> Option<String> {
    tauri::async_runtime::spawn_blocking(imp::art).await.ok().flatten()
}

#[tauri::command]
pub async fn media_control(action: String) {
    let _ = tauri::async_runtime::spawn_blocking(move || imp::control(&action)).await;
}
