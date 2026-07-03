//! Linux: focused-window capture on X11 (best effort), monitor fallback.
//! On Wayland, window enumeration is blocked by design — the monitor fallback
//! in capture.rs goes through the xdg-desktop-portal / pipewire path instead.
#![cfg(target_os = "linux")]

use super::capture::{CaptureContext, CaptureResult, CaptureSource};

pub fn capture_focused_window() -> Option<CaptureResult> {
    // Only meaningful under X11; Wayland compositors do not expose windows.
    if std::env::var("WAYLAND_DISPLAY").is_ok() {
        return None;
    }

    let windows = xcap::Window::all().ok()?;
    let window = windows.into_iter().find(|w| w.is_focused().unwrap_or(false))?;
    if window.is_minimized().unwrap_or(false) {
        return None;
    }

    let image = window.capture_image().ok()?;
    let context = CaptureContext {
        x: window.x().unwrap_or(0),
        y: window.y().unwrap_or(0),
        width: image.width(),
        height: image.height(),
        source: CaptureSource::Window,
    };
    let app_name = window
        .app_name()
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| window.title().ok())
        .unwrap_or_else(|| "unknown".into());

    Some(CaptureResult { image, context, app_name })
}
