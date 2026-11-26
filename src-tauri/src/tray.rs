// The notification-area icon. Windows hides it behind the ^ overflow by default; the user can pin
// it to the taskbar. Left click opens the island's Settings page, right click opens this menu.
use std::sync::OnceLock;
use tauri::{
    menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Wry,
};

static AUTO: OnceLock<CheckMenuItem<Wry>> = OnceLock::new();

fn show_settings(app: &AppHandle) {
    logf!("tray", "open settings");
    let _ = app.emit("show://settings", ());
}

/// Keep the menu check mark in step with the Settings page and the registry.
pub fn sync_autostart(on: bool) {
    if let Some(item) = AUTO.get() {
        let _ = item.set_checked(on);
    }
}

pub fn init(app: &AppHandle) -> tauri::Result<()> {
    let settings = MenuItem::with_id(app, "settings", "IslandWin Settings", true, None::<&str>)?;
    let auto = CheckMenuItem::with_id(
        app,
        "autostart",
        "Start with Windows",
        true,
        crate::autostart::get(),
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(app, "quit", "Quit IslandWin", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[&settings, &auto, &PredefinedMenuItem::separator(app)?, &quit],
    )?;

    let mut builder = TrayIconBuilder::with_id("islandwin")
        .tooltip("IslandWin")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, ev| match ev.id().as_ref() {
            "settings" => show_settings(app),
            "autostart" => {
                let on = AUTO.get().and_then(|i| i.is_checked().ok()).unwrap_or(false);
                match crate::autostart::set(on) {
                    Ok(()) => {
                        logf!("tray", "start with Windows -> {on}");
                        let _ = app.emit("autostart://changed", on);
                    }
                    Err(e) => {
                        // the menu may already look toggled; put it back if the registry refused
                        sync_autostart(!on);
                        logf!("tray", "start with Windows failed: {e}");
                    }
                }
            }
            "quit" => {
                logf!("tray", "quit from the tray menu");
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, ev| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = ev
            {
                show_settings(tray.app_handle());
            }
        });

    match tauri::image::Image::from_bytes(include_bytes!("../icons/tray.png")) {
        Ok(icon) => builder = builder.icon(icon),
        Err(e) => logf!("tray", "could not load the tray icon: {e}"),
    }
    builder.build(app)?;
    let _ = AUTO.set(auto);
    logf!("tray", "icon added to the notification area");
    Ok(())
}
