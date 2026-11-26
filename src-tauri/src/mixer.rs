// Speaker + microphone volume and mute through the Windows Core Audio endpoint API.
use serde::Serialize;

#[derive(Serialize, Default)]
pub struct Mix { pub vol: f32, pub mute: bool, pub mic: f32, pub mic_mute: bool, pub ok: bool }

#[cfg(windows)]
mod imp {
    use super::Mix;
    use std::cell::RefCell;
    use windows::{
        core::Result,
        Win32::{
            Media::Audio::{eCapture, eConsole, eRender, Endpoints::IAudioEndpointVolume, EDataFlow, IMMDeviceEnumerator, MMDeviceEnumerator},
            System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED},
        },
    };

    // The frontend polls the mixer every 500 ms while its page is open. Keep the device enumerator
    // alive per worker thread instead of activating COM for it on every read; the default endpoint
    // is still looked up fresh, so switching output/input devices keeps working.
    thread_local! {
        static ENUM: RefCell<Option<IMMDeviceEnumerator>> = const { RefCell::new(None) };
    }

    fn enumerator() -> Result<IMMDeviceEnumerator> {
        let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }; // no-op if this thread already has COM
        ENUM.with(|e| {
            let mut slot = e.borrow_mut();
            if slot.is_none() {
                *slot = Some(unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)? });
            }
            Ok(slot.clone().unwrap())
        })
    }

    fn ep(flow: EDataFlow) -> Result<IAudioEndpointVolume> {
        let en = enumerator()?;
        let dev = unsafe { en.GetDefaultAudioEndpoint(flow, eConsole)? };
        unsafe { dev.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None) }
    }
    fn read(flow: EDataFlow) -> Option<(f32, bool)> {
        let e = ep(flow).ok()?;
        unsafe { Some((e.GetMasterVolumeLevelScalar().ok()?, e.GetMute().ok()?.as_bool())) }
    }
    pub fn get() -> Mix {
        let o = read(eRender);
        let m = read(eCapture);
        Mix { vol: o.map(|x| x.0).unwrap_or(0.0), mute: o.map(|x| x.1).unwrap_or(false), mic: m.map(|x| x.0).unwrap_or(0.0), mic_mute: m.map(|x| x.1).unwrap_or(false), ok: o.is_some() }
    }
    pub fn set_vol(mic: bool, v: f32) {
        if let Ok(e) = ep(if mic { eCapture } else { eRender }) {
            unsafe {
                let _ = e.SetMasterVolumeLevelScalar(v.clamp(0.0, 1.0), std::ptr::null());
                if v > 0.0 { let _ = e.SetMute(false, std::ptr::null()); }
            }
        }
    }
    pub fn set_mute(mic: bool, on: bool) {
        if let Ok(e) = ep(if mic { eCapture } else { eRender }) { unsafe { let _ = e.SetMute(on, std::ptr::null()); } }
    }
}
#[cfg(not(windows))]
mod imp {
    use super::Mix;
    pub fn get() -> Mix { Mix::default() }
    pub fn set_vol(_: bool, _: f32) {}
    pub fn set_mute(_: bool, _: bool) {}
}

#[tauri::command]
pub fn mixer_get() -> Mix { imp::get() }
#[tauri::command]
pub fn mixer_set_vol(mic: bool, v: f32) {
    logf!("mixer", "set_vol {} -> {:.2}", if mic { "mic" } else { "speaker" }, v);
    imp::set_vol(mic, v)
}
#[tauri::command]
pub fn mixer_set_mute(mic: bool, on: bool) {
    logf!("mixer", "set_mute {} -> {on}", if mic { "mic" } else { "speaker" });
    imp::set_mute(mic, on)
}
