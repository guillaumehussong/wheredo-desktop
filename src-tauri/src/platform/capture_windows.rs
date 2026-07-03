//! Windows: capture the foreground window (xcap wraps GetForegroundWindow
//! in Window::is_focused).
#![cfg(target_os = "windows")]

use super::capture::{CaptureContext, CaptureResult, CaptureSource};

pub fn capture_foreground_window() -> Option<CaptureResult> {
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
