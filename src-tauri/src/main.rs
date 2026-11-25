#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod media;

use tauri::{Emitter, Manager, PhysicalPosition};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

fn main() {
    tauri::Builder::default()
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _sc, ev| {
                    if ev.state() == ShortcutState::Pressed {
                        let _ = app.emit("toggle-listen", ());
                    }
                })
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            media::media_poll,
            media::media_art,
            media::media_control
        ])
        .setup(|app| {
            // Ctrl+Alt+Space toggles speech-to-text
            app.global_shortcut()
                .register(Shortcut::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::Space))?;

            // Center at top of the primary monitor
            let win = app.get_webview_window("main").unwrap();
            if let Some(m) = win.primary_monitor()? {
                let scale = m.scale_factor();
                let w = (420.0 * scale) as i32;
                win.set_position(PhysicalPosition::new(
                    m.position().x + (m.size().width as i32 - w) / 2,
                    m.position().y,
                ))?;
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running IslandWin");
}
