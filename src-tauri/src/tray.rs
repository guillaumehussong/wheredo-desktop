//! System tray icon with status states (port of MenuBarController/MenuBarIcon).

use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::TrayIconBuilder;
use tauri::AppHandle;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Status {
    Ready,
    Listening,
    Busy,
    Error,
}

impl Status {
    fn label(self) -> &'static str {
        match self {
            Status::Ready => "Ready — press hotkey to speak",
            Status::Listening => "Listening…",
            Status::Busy => "Working…",
            Status::Error => "Error — see log",
        }
    }

    fn icon_bytes(self) -> &'static [u8] {
        match self {
            Status::Ready => include_bytes!("../icons/tray-ready.png"),
            Status::Listening => include_bytes!("../icons/tray-listening.png"),
            Status::Busy => include_bytes!("../icons/tray-busy.png"),
            Status::Error => include_bytes!("../icons/tray-error.png"),
        }
    }
}

const TRAY_ID: &str = "wheredo-tray";

pub fn install(app: &AppHandle, on_speak: impl Fn(&AppHandle) + Send + Sync + 'static) -> tauri::Result<()> {
    let speak = MenuItemBuilder::with_id("speak", "Speak now").build(app)?;
    let status = MenuItemBuilder::with_id("status", Status::Ready.label())
        .enabled(false)
        .build(app)?;
    let logs = MenuItemBuilder::with_id("logs", "Open log file").build(app)?;
    let quit = MenuItemBuilder::with_id("quit", "Quit Wheredo").build(app)?;
    let menu = MenuBuilder::new(app)
        .item(&status)
        .separator()
        .item(&speak)
        .item(&logs)
        .separator()
        .item(&quit)
        .build()?;

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(tauri::image::Image::from_bytes(Status::Ready.icon_bytes())?)
        .tooltip("Wheredo — Ready")
        .menu(&menu)
        .on_menu_event(move |app, event| match event.id().as_ref() {
            "speak" => on_speak(app),
            "logs" => {
                let _ = open::that(crate::core::feedback::log_file_url());
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;
    Ok(())
}

/// Update tray icon + tooltip + status menu row. Safe to call from any thread.
pub fn set_status(app: &AppHandle, status: Status) {
    let app = app.clone();
    let _ = app.clone().run_on_main_thread(move || {
        if let Some(tray) = app.tray_by_id(TRAY_ID) {
            if let Ok(icon) = tauri::image::Image::from_bytes(status.icon_bytes()) {
                let _ = tray.set_icon(Some(icon));
            }
            let _ = tray.set_tooltip(Some(format!("Wheredo — {}", status.label())));
        }
    });
}
