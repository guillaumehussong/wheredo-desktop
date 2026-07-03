//! Red guide-cursor overlay (port of GuideCursor.swift).
//!
//! Design: one small transparent, click-through, always-on-top Tauri window
//! per marker, loading overlay.html which draws the pulsing red cursor + label.
//! Coordinates are GLOBAL screen pixels, top-left origin (no flip needed).

use std::sync::atomic::{AtomicU64, Ordering};

use tauri::{AppHandle, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindowBuilder};

use crate::core::{config, feedback};

/// Panel size around the marker: room for the pulse ring and the label bubble.
const PANEL_W: u32 = 320;
const PANEL_H: u32 = 110;
/// Where the cursor tip sits INSIDE the panel.
const TIP_X: i32 = 24;
const TIP_Y: i32 = 24;

/// Monotonic generation: an old auto-dismiss timer must not tear down a newer
/// overlay shown in the meantime (same trick as the Swift version).
static GENERATION: AtomicU64 = AtomicU64::new(0);

pub struct Marker {
    /// Global screen position (pixels, top-left origin) the cursor points at.
    pub x: i32,
    pub y: i32,
    pub label: String,
}

/// Show markers. Must be called from the main thread (Tauri window creation).
pub fn show_markers(app: &AppHandle, markers: Vec<Marker>) {
    dismiss_all(app);
    if !config::show_guide_cursor() || markers.is_empty() {
        return;
    }

    let generation = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;

    for (i, marker) in markers.iter().enumerate() {
        let label_window = format!("overlay-{generation}-{i}");
        let encoded_label: String =
            url_encode(&marker.label.chars().take(60).collect::<String>());
        let url = format!("overlay.html?label={encoded_label}");

        let builder = WebviewWindowBuilder::new(app, &label_window, WebviewUrl::App(url.into()))
            .title("Wheredo guide")
            .transparent(true)
            .decorations(false)
            .resizable(false)
            .always_on_top(true)
            .skip_taskbar(true)
            .focused(false)
            .shadow(false)
            .visible(true);

        match builder.build() {
            Ok(window) => {
                let _ = window.set_size(PhysicalSize::new(PANEL_W, PANEL_H));
                let _ = window.set_position(PhysicalPosition::new(
                    marker.x - TIP_X,
                    marker.y - TIP_Y,
                ));
                // Click-through: the user must be able to click the control
                // UNDER the marker.
                let _ = window.set_ignore_cursor_events(true);
                feedback::log(&format!(
                    "🔴 Guide cursor: {} at ({}, {})",
                    if marker.label.is_empty() { "target" } else { &marker.label },
                    marker.x,
                    marker.y
                ));
            }
            Err(e) => feedback::error("Overlay", &format!("could not create window: {e}")),
        }
    }

    let duration = config::guide_cursor_duration();
    if duration > 0.0 {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs_f64(duration)).await;
            if GENERATION.load(Ordering::SeqCst) == generation {
                let handle = app.clone();
                let _ = app.run_on_main_thread(move || dismiss_all(&handle));
            }
        });
    }
}

/// Close every overlay window. Main thread only.
pub fn dismiss_all(app: &AppHandle) {
    for (label, window) in app.webview_windows() {
        if label.starts_with("overlay-") {
            let _ = window.close();
        }
    }
}

fn url_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}
